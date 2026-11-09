//! Unit weights are read and set through `systemctl`, which can take
//! seconds (a busy manager, `--machine=USER@` round trips) and used to run
//! on the main loop with the service locked: process events, D-Bus calls
//! and signals all waited. Now the service only queues jobs; a worker
//! thread runs them and hands the results back to the main loop.

use crate::units::{self, UnitId, Weights};

#[derive(Debug, Clone)]
pub enum UnitJob {
    /// Give `id` the profile's weights for `owner`. `known` is the unit's
    /// original and currently applied weights, if scdr already holds it.
    Claim {
        id: UnitId,
        owner: u32,
        profile: String,
        cgroup: String,
        cpu: Option<u64>,
        io: Option<u64>,
        known: Option<(Weights, Weights)>,
        dry_run: bool,
    },
    /// Put `original` back, unless someone changed `applied` since.
    Restore {
        id: UnitId,
        original: Weights,
        applied: Weights,
        dry_run: bool,
    },
}

#[derive(Debug, Clone)]
pub enum UnitDone {
    Claimed {
        id: UnitId,
        owner: u32,
        profile: String,
        cgroup: String,
        original: Weights,
        applied: Weights,
        kept: Weights,
    },
    Failed {
        id: UnitId,
        error: String,
    },
    Restored {
        id: UnitId,
    },
}

/// What to ask for, and which of the unit's own weights scdr leaves alone:
/// only weights the unit leaves at systemd's default are filled in.
pub fn plan(original: Weights, cpu: Option<u64>, io: Option<u64>) -> (Weights, Weights) {
    let want = Weights {
        cpu: original.cpu.or(cpu),
        io: original.io.or(io),
    };
    let kept = Weights {
        cpu: original.cpu.filter(|_| cpu.is_some()),
        io: original.io.filter(|_| io.is_some()),
    };
    (want, kept)
}

/// Runs one job. Blocks on `systemctl`.
pub fn run(job: UnitJob) -> UnitDone {
    match job {
        UnitJob::Claim {
            id,
            owner,
            profile,
            cgroup,
            cpu,
            io,
            known,
            dry_run,
        } => {
            let (original, current) = match known {
                Some(k) => k,
                None => match units::get(&id) {
                    Ok(w) => (w, w),
                    Err(error) => {
                        return UnitDone::Failed {
                            id,
                            error: format!("can't read weights: {error}"),
                        };
                    }
                },
            };
            let (want, kept) = plan(original, cpu, io);
            if want != current {
                if dry_run {
                    tracing::info!("{id}: would set {want} (was {original})");
                } else if let Err(e) = units::set(&id, &want, &current) {
                    return UnitDone::Failed {
                        id,
                        error: format!("can't set {want}: {e}"),
                    };
                } else {
                    tracing::info!("{id}: set {want} for {profile} (was {original})");
                }
            }
            UnitDone::Claimed {
                id,
                owner,
                profile,
                cgroup,
                original,
                applied: want,
                kept,
            }
        }
        UnitJob::Restore {
            id,
            original,
            applied,
            dry_run,
        } => {
            if !dry_run && applied != original {
                // Only undo our own change; leave it if someone set something else since.
                match units::get(&id) {
                    Ok(now) if now == applied => match units::set(&id, &original, &now) {
                        Ok(()) => tracing::info!("{id}: restored {original}"),
                        Err(e) => tracing::warn!("{id}: can't restore {original}: {e}"),
                    },
                    Ok(now) => tracing::info!("{id}: left as {now} (changed by something else)"),
                    Err(_) => {} // unit is gone
                }
            }
            UnitDone::Restored { id }
        }
    }
}

/// Runs jobs on a thread of its own. Results arrive on `done`; the thread
/// ends once every sender of the returned channel is dropped.
pub fn spawn_worker(
    done: tokio::sync::mpsc::UnboundedSender<UnitDone>,
) -> std::io::Result<(
    std::sync::mpsc::Sender<UnitJob>,
    std::thread::JoinHandle<()>,
)> {
    let (tx, rx) = std::sync::mpsc::channel::<UnitJob>();
    let handle = std::thread::Builder::new()
        .name("scdr-units".into())
        .spawn(move || {
            for job in rx {
                if done.send(run(job)).is_err() {
                    return;
                }
            }
        })?;
    Ok((tx, handle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_fills_only_defaults() {
        let unset = Weights::default();
        assert_eq!(
            plan(unset, Some(300), None).0,
            Weights {
                cpu: Some(300),
                io: None
            }
        );
        // baloo ships CPUWeight=1: kept, not overridden.
        let own = Weights {
            cpu: Some(1),
            io: None,
        };
        let (want, kept) = plan(own, Some(20), Some(20));
        assert_eq!(
            want,
            Weights {
                cpu: Some(1),
                io: Some(20)
            }
        );
        assert_eq!(
            kept,
            Weights {
                cpu: Some(1),
                io: None
            }
        );
    }
}
