//! Unit weights: a managed process whose profile sets `cpu_weight` or
//! `io_weight` claims its systemd unit. The first live claimant holds it;
//! the unit's original weights are restored when no one does. A weight the
//! unit already sets itself (KDE ships baloo at CPUWeight=1) is never
//! overridden, the same way scdr leaves values other tools set on threads.

use super::{Service, State};
use crate::config::NamedProfile;
use crate::procfs::ProcInfo;
use crate::units::{self, UnitId, Weights};
use serde::{Deserialize, Serialize};

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
    #[serde(skip)]
    cgroup: String,
}

impl Service {
    /// Called after a process was (re)evaluated.
    pub(super) fn sync_unit(&mut self, pid: u32, info: &ProcInfo, profile: Option<&NamedProfile>) {
        let profile = profile.filter(|p| p.profile.has_unit_weights());
        let Some(profile) = profile else {
            self.release_units_of(pid);
            return;
        };
        let Some(id) = UnitId::from_cgroup(&info.cgroup) else {
            return;
        };

        if let Some(st) = self.units.get(&id) {
            if st.owner != pid && self.holds_claim(st.owner) {
                return; // someone else in this unit already set its weights
            }
        }

        let original = match self.units.get(&id) {
            Some(st) => st.original,
            None => match units::get(&id) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("{id}: can't read weights: {e}");
                    return;
                }
            },
        };
        // Only fill in weights the unit leaves at systemd's default.
        let pick = |own: Option<u64>, profile: Option<u64>| own.or(profile);
        let want = Weights {
            cpu: pick(original.cpu, profile.profile.cpu_weight),
            io: pick(original.io, profile.profile.io_weight),
        };
        let kept = Weights {
            cpu: original.cpu.filter(|_| profile.profile.cpu_weight.is_some()),
            io: original.io.filter(|_| profile.profile.io_weight.is_some()),
        };
        let current = self.units.get(&id).map(|s| s.applied).unwrap_or(original);

        if want != current {
            if self.dry_run {
                tracing::info!("{id}: would set {want} (was {original})");
            } else if let Err(e) = units::set(&id, &want, &current) {
                tracing::warn!("{id}: can't set {want}: {e}");
                return;
            } else {
                tracing::info!("{id}: set {want} for {} (was {original})", profile.name);
            }
        }
        self.units.insert(
            id,
            UnitState {
                original,
                applied: want,
                owner: pid,
                profile: profile.name.clone(),
                kept,
                cgroup: info.cgroup.clone(),
            },
        );
    }

    fn holds_claim(&self, pid: u32) -> bool {
        self.procs.get(&pid).is_some_and(|e| match &e.state {
            State::Managed(m) => self.config.profiles.get(&m.profile).is_some_and(|p| p.profile.has_unit_weights()),
            _ => false,
        })
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
        // Only undo our own change; leave it if someone set something else since.
        match units::get(id) {
            Ok(now) if now == st.applied => match units::set(id, &st.original, &now) {
                Ok(()) => tracing::info!("{id}: restored {}", st.original),
                Err(e) => tracing::warn!("{id}: can't restore {}: {e}", st.original),
            },
            Ok(now) => tracing::info!("{id}: left as {now} (changed by something else)"),
            Err(_) => {} // unit is gone
        }
    }

    /// The unit claim covering `pid`'s unit, for `explain`.
    pub(super) fn unit_for(&self, cgroup: &str) -> Option<(UnitId, &UnitState)> {
        let id = UnitId::from_cgroup(cgroup)?;
        let st = self.units.get(&id)?;
        Some((id, st))
    }

    /// Units whose weights scdr actually changed.
    pub(super) fn unit_count(&self) -> usize {
        self.units.values().filter(|s| s.applied != s.original).count()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn stopped_services_are_restored_not_forgotten() {
        assert!(super::forget_when_stopped("app-firefox-1234.scope"));
        assert!(!super::forget_when_stopped("pamac-daemon.service"));
        assert!(!super::forget_when_stopped("app-org.telegram.desktop@autostart.service"));
    }
}
