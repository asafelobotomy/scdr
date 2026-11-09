//! Launcher detection: which processes were started by something that
//! hand-tunes priorities, so scdr leaves them and their descendants alone.

use super::Service;
use super::inherit::Inherited;
use crate::apply;
use crate::procfs;

impl Service {
    /// The launcher state a new process inherits from its ancestors.
    pub(super) fn inherited_sticky(&self, mut ppid: u32) -> Option<String> {
        for _ in 0..64 {
            if ppid <= 1 {
                return None;
            }
            if let Some(parent) = self.procs.get(&ppid) {
                if parent.start_time != 0 {
                    return parent.sticky.clone();
                }
            }
            let comm = procfs::comm(ppid)?;
            if self.config.exceptions.is_launcher_name(&comm) {
                return Some(format!("started by launcher {comm:?}"));
            }
            ppid = procfs::Stat::read(ppid).ok()?.ppid;
        }
        None
    }
}

/// A freshly exec'd process whose priorities differ from its parent's was
/// started through something like `nice -n 10 foo` or `chrt`, which exec'd
/// away before we saw it. Treat it as hand-tuned.
///
/// Unless the difference is scdr's own: a child forked while its parent
/// still carried a managed ancestor's boost keeps that boost, even after
/// scdr put the parent back (`sh -c 'systemd-run … app'` from a boosted
/// compositor). Values equal to that boost are inherited, not hand-tuned.
pub(super) fn hand_tuned(pid: u32, ppid: u32, inherited: Option<&Inherited>) -> Option<String> {
    let me = apply::TaskState::read(pid).ok()?;
    let parent = apply::TaskState::read(ppid).ok()?;
    differs(&me, &parent, inherited)
        .then(|| "priorities differ from its parent's (started via nice/chrt/ionice?)".to_owned())
}

fn differs(
    me: &apply::TaskState,
    parent: &apply::TaskState,
    inherited: Option<&Inherited>,
) -> bool {
    let key = |t: &apply::TaskState| (t.cpu.policy, t.cpu.nice, t.ioprio);
    // The child holds exactly what scdr gave its ancestor, or what scdr put
    // back: that is scdr's doing, whatever the parent shows now (it may
    // still be boosted, already put back, or gone, leaving systemd).
    let ours =
        |inh: &Inherited| key(me) == key(&inh.target.task) || key(me) == key(&inh.original.task);
    if inherited.is_some_and(ours) {
        return false;
    }
    key(me) != key(parent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply::{CpuState, Snapshot, TaskState};

    fn task(nice: i32) -> TaskState {
        TaskState {
            cpu: CpuState {
                policy: 0,
                priority: 0,
                nice,
                slice_ns: 0,
                util_min: 0,
                util_max: 1024,
            },
            ioprio: 0,
        }
    }

    #[test]
    fn an_inherited_boost_is_not_hand_tuning() {
        // Hyprland boosted to -5; sh already put back to 0; systemd-run forked before that.
        let inh = Inherited {
            target: Snapshot {
                task: task(-5),
                oom: None,
            },
            original: Snapshot {
                task: task(0),
                oom: None,
            },
        };
        assert!(!differs(&task(-5), &task(0), Some(&inh)));
        // And the other way round: child already put back, parent not yet.
        assert!(!differs(&task(0), &task(-5), Some(&inh)));
        // Reparented to systemd --user (nice 0) while still boosted.
        assert!(!differs(&task(-5), &task(0), Some(&inh)));
        // `nice -n 10` is still hand-tuning.
        assert!(differs(&task(10), &task(0), Some(&inh)));
        assert!(differs(&task(10), &task(0), None));
        assert!(!differs(&task(0), &task(0), None));
    }
}
