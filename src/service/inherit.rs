//! Values a process inherited from a boosted ancestor are scdr's doing, not
//! the process's own. fork() copies policy, nice, the custom slice, uclamp,
//! io priority and oom_score_adj, so a child of a process scdr boosted
//! starts out boosted. Read as-is, that boost would become the child's
//! "original" and be put back on restore; a child no rule or role covers
//! would keep it for good.

use super::{Managed, Service, State};
use crate::apply::Snapshot;

/// How far up the process tree to look for a managed ancestor.
const MAX_DEPTH: usize = 64;

/// The boost an ancestor carries: what scdr set, and what it replaced.
#[derive(Debug, Clone, Copy)]
pub(super) struct Inherited {
    pub target: Snapshot,
    pub original: Snapshot,
}

impl Service {
    /// The nearest managed ancestor of a process whose parent is `ppid`,
    /// looking through processes no rule or role covers (they may carry
    /// the same inherited values) but not past exceptions or launchers.
    pub(super) fn inherited_from(&self, ppid: u32) -> Option<Inherited> {
        let mut pid = ppid;
        for _ in 0..MAX_DEPTH {
            if pid <= 1 {
                return None;
            }
            let e = self.procs.get(&pid)?;
            match &e.state {
                State::Managed(m) => return Some(Inherited::of(m)),
                State::Unmatched | State::Pending => pid = e.ppid,
                State::Excepted(_) | State::KernelThread => return None,
            }
        }
        None
    }
}

impl Inherited {
    fn of(m: &Managed) -> Self {
        Inherited {
            target: m.target,
            original: m.original,
        }
    }

    /// `own` with each value it shares with the ancestor's target replaced
    /// by the ancestor's original. Values the profile left alone are equal
    /// in both, so they pass through unchanged.
    pub fn undo(&self, own: Snapshot) -> Snapshot {
        let (t, o) = (&self.target, &self.original);
        let mut s = own;
        let c = &mut s.task.cpu;
        if (c.policy, c.priority, c.nice)
            == (t.task.cpu.policy, t.task.cpu.priority, t.task.cpu.nice)
        {
            c.policy = o.task.cpu.policy;
            c.priority = o.task.cpu.priority;
            c.nice = o.task.cpu.nice;
        }
        if (c.util_min, c.util_max) == (t.task.cpu.util_min, t.task.cpu.util_max) {
            c.util_min = o.task.cpu.util_min;
            c.util_max = o.task.cpu.util_max;
        }
        // Originals always record the kernel-default slice (see
        // Snapshot::original), so there is nothing to undo there.
        if s.task.ioprio == t.task.ioprio {
            s.task.ioprio = o.task.ioprio;
        }
        if s.oom == t.oom {
            s.oom = o.oom;
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::Inherited;
    use crate::apply::{CpuState, Snapshot, TaskState};

    fn snap(nice: i32, slice_ns: u64, ioprio: u32, oom: i32) -> Snapshot {
        Snapshot {
            task: TaskState {
                cpu: CpuState {
                    policy: 0,
                    priority: 0,
                    nice,
                    slice_ns,
                    util_min: 0,
                    util_max: 1024,
                },
                ioprio,
            },
            oom: Some(oom),
        }
    }

    #[test]
    fn inherited_boost_is_not_an_original() {
        // Parent: nice 0 / oom 200 boosted to nice -2 (foreground).
        let inh = Inherited {
            target: snap(-2, 2_000_000, 0, 200),
            original: snap(0, 0, 0, 200),
        };
        // A child that inherited the boost started out at nice 0.
        assert_eq!(inh.undo(snap(-2, 0, 0, 200)), snap(0, 0, 0, 200));
        // A child someone reniced on purpose keeps its own value.
        assert_eq!(inh.undo(snap(5, 0, 0, 200)), snap(5, 0, 0, 200));
    }

    #[test]
    fn only_shared_fields_are_undone() {
        // compositor: nice -5, io be:0, oom -900 over nice 0, io none, oom 0.
        let inh = Inherited {
            target: snap(-5, 0, 0x4000, -900),
            original: snap(0, 0, 0, 0),
        };
        // Child kept the inherited oom but set its own nice.
        assert_eq!(inh.undo(snap(3, 0, 0x4000, -900)), snap(3, 0, 0, 0));
    }
}
