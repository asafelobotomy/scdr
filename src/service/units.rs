//! Unit weights: a managed process whose profile sets `cpu_weight` or
//! `io_weight` claims its systemd unit. The first live claimant holds it;
//! the unit's original weights are restored when no one does. A weight the
//! unit already sets itself (KDE ships baloo at CPUWeight=1) is never
//! overridden, the same way scdr leaves values other tools set on threads.
//!
//! The `systemctl` work happens on a worker thread (see `unit_jobs`): this
//! module decides, queues jobs and records their results.

use super::unit_jobs::{self, UnitDone, UnitJob};
use super::{Service, State};
use crate::config::NamedProfile;
use crate::procfs::ProcInfo;
use crate::units::{self, UnitId, Weights};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// How long to leave a unit alone after `systemctl` failed on it, so a
/// broken unit isn't retried on every scan and exec of its processes.
const FAILED_BACKOFF: Duration = Duration::from_secs(60);

/// Scopes are transient: once their processes exit, the unit is gone.
fn forget_when_stopped(unit: &str) -> bool {
    unit.ends_with(".scope")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitState {
    pub original: Weights,
    pub applied: Weights,
    /// The process holding the claim, and the profile it got it from.
    pub owner: u32,
    pub profile: String,
    /// Weights the unit sets itself, which scdr left alone.
    pub kept: Weights,
    /// The unit's own cgroup (not the claimant's, which may be nested).
    #[serde(skip)]
    cgroup: String,
}

/// Unit jobs waiting to be sent, in flight, or backing off after a failure.
#[derive(Debug, Default)]
pub struct UnitQueue {
    outbox: Vec<UnitJob>,
    in_flight: std::collections::BTreeSet<UnitId>,
    failed: std::collections::BTreeMap<UnitId, Instant>,
}

impl Service {
    /// Called after a process was (re)evaluated.
    pub(super) fn sync_unit(&mut self, pid: u32, info: &ProcInfo, profile: Option<&NamedProfile>) {
        let profile = profile.filter(|p| p.profile.has_unit_weights());
        let Some(profile) = profile else {
            self.release_units_of(pid);
            return;
        };
        let (Some(id), Some(cgroup)) = (
            UnitId::from_cgroup(&info.cgroup),
            units::unit_cgroup(&info.cgroup),
        ) else {
            return;
        };
        if self.unit_queue.in_flight.contains(&id) {
            return;
        }
        if let Some(&until) = self.unit_queue.failed.get(&id) {
            if Instant::now() < until {
                return;
            }
            self.unit_queue.failed.remove(&id);
        }
        let (cpu, io) = (profile.profile.cpu_weight, profile.profile.io_weight);
        let known = match self.units.get_mut(&id) {
            Some(st)
                if st.owner != pid
                    && self
                        .procs
                        .get(&st.owner)
                        .is_some_and(|e| claims(&self.config, &e.state)) =>
            {
                return; // someone else in this unit already set its weights
            }
            Some(st) => {
                let (want, kept) = unit_jobs::plan(st.original, cpu, io);
                if want == st.applied {
                    st.owner = pid;
                    st.profile = profile.name.clone();
                    st.kept = kept;
                    return;
                }
                Some((st.original, st.applied))
            }
            None => None,
        };
        self.queue_unit_job(UnitJob::Claim {
            id,
            owner: pid,
            profile: profile.name.clone(),
            cgroup,
            cpu,
            io,
            known,
            dry_run: self.dry_run,
        });
    }

    fn queue_unit_job(&mut self, job: UnitJob) {
        let id = match &job {
            UnitJob::Claim { id, .. } | UnitJob::Restore { id, .. } => id.clone(),
        };
        self.unit_queue.in_flight.insert(id);
        self.unit_queue.outbox.push(job);
    }

    /// Jobs for the worker; the main loop sends them once the lock is released.
    pub fn take_unit_jobs(&mut self) -> Vec<UnitJob> {
        std::mem::take(&mut self.unit_queue.outbox)
    }

    /// Runs queued jobs here and now (`--once`, and shutdown).
    pub fn run_unit_jobs_now(&mut self) {
        while !self.unit_queue.outbox.is_empty() {
            for job in self.take_unit_jobs() {
                self.unit_done(unit_jobs::run(job));
            }
        }
    }

    /// A job's result, from the worker.
    pub fn unit_done(&mut self, done: UnitDone) {
        match done {
            UnitDone::Claimed {
                id,
                owner,
                profile,
                cgroup,
                original,
                applied,
                kept,
            } => {
                self.unit_queue.in_flight.remove(&id);
                self.units.insert(
                    id.clone(),
                    UnitState {
                        original,
                        applied,
                        owner,
                        profile,
                        kept,
                        cgroup,
                    },
                );
                // The claimant may have exited or lost its profile meanwhile.
                if !self.holds_claim(owner) {
                    self.restore_unit(&id);
                }
            }
            UnitDone::Failed { id, error } => {
                tracing::warn!("{id}: {error}; retrying in {}s", FAILED_BACKOFF.as_secs());
                self.unit_queue.in_flight.remove(&id);
                self.unit_queue
                    .failed
                    .insert(id, Instant::now() + FAILED_BACKOFF);
            }
            UnitDone::Restored { id } => {
                self.unit_queue.in_flight.remove(&id);
            }
        }
    }

    fn holds_claim(&self, pid: u32) -> bool {
        self.procs
            .get(&pid)
            .is_some_and(|e| claims(&self.config, &e.state))
    }

    /// `pid` no longer wants unit weights: restore any unit it held.
    pub(super) fn release_units_of(&mut self, pid: u32) {
        let held: Vec<UnitId> = self
            .units
            .iter()
            .filter(|(_, s)| s.owner == pid)
            .map(|(id, _)| id.clone())
            .collect();
        for id in held {
            self.restore_unit(&id);
        }
    }

    /// After a rescan: forget units that no longer exist, restore units
    /// whose claimant is gone.
    pub(super) fn sweep_units(&mut self) {
        let now = Instant::now();
        self.unit_queue.failed.retain(|_, until| *until > now);
        let ids: Vec<UnitId> = self.units.keys().cloned().collect();
        for id in ids {
            let st = &self.units[&id];
            let running = std::path::Path::new(&format!("/sys/fs/cgroup{}", st.cgroup)).exists();
            match (running, forget_when_stopped(&id.unit)) {
                // A transient scope that ended is gone, weights and all.
                (false, true) => {
                    self.units.remove(&id);
                }
                // A stopped service keeps its runtime weights (an on-demand
                // daemon starts again with them), so restore them now.
                (false, false) => self.restore_unit(&id),
                (true, _) if !self.holds_claim(st.owner) => self.restore_unit(&id),
                (true, _) => {}
            }
        }
    }

    pub(super) fn restore_units(&mut self) {
        let ids: Vec<UnitId> = self.units.keys().cloned().collect();
        for id in ids {
            self.restore_unit(&id);
        }
    }

    fn restore_unit(&mut self, id: &UnitId) {
        let Some(st) = self.units.remove(id) else {
            return;
        };
        if self.dry_run || st.applied == st.original {
            return;
        }
        self.queue_unit_job(UnitJob::Restore {
            id: id.clone(),
            original: st.original,
            applied: st.applied,
            dry_run: self.dry_run,
        });
    }

    /// The unit claim covering `pid`'s unit, for `explain`.
    pub(super) fn unit_for(&self, cgroup: &str) -> Option<(UnitId, &UnitState)> {
        let id = UnitId::from_cgroup(cgroup)?;
        let st = self.units.get(&id)?;
        Some((id, st))
    }

    /// Units whose weights scdr actually changed.
    pub(super) fn unit_count(&self) -> usize {
        self.units
            .values()
            .filter(|s| s.applied != s.original)
            .count()
    }
}

/// Whether a process in `state` holds unit weights.
fn claims(config: &crate::config::Config, state: &State) -> bool {
    match state {
        State::Managed(m) => config
            .profiles
            .get(&m.profile)
            .is_some_and(|p| p.profile.has_unit_weights()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn stopped_services_are_restored_not_forgotten() {
        assert!(super::forget_when_stopped("app-firefox-1234.scope"));
        assert!(!super::forget_when_stopped("pamac-daemon.service"));
        assert!(!super::forget_when_stopped(
            "app-org.telegram.desktop@autostart.service"
        ));
    }
}
