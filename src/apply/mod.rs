//! Applying profiles to every thread of a process, without clobbering
//! values someone else set.
//!
//! scdr remembers each process's original state and what it last applied.
//! A thread is only changed while its current value is one of those
//! ("owned"); anything else was set by another tool (rtkit, GameMode, the
//! user with `renice`) and is left alone.

use crate::config::Profile;
use crate::procfs;
use crate::sched::{self, SchedAttr};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;

/// The CPU scheduling values scdr manages on one task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuState {
    pub policy: u32,
    pub priority: u32,
    pub nice: i32,
    /// EEVDF slice in ns. In targets and originals 0 means "kernel default";
    /// read from a live task it is always the effective slice.
    pub slice_ns: u64,
    /// uclamp requests (0..1024); (0, 1024) is the unclamped default.
    pub util_min: u32,
    pub util_max: u32,
}

const UCLAMP_DEFAULT: (u32, u32) = (0, 1024);

impl CpuState {
    fn from_attr(a: &SchedAttr) -> Self {
        CpuState {
            policy: a.policy,
            priority: a.priority,
            nice: a.nice,
            slice_ns: a.runtime,
            util_min: a.util_min,
            util_max: a.util_max,
        }
    }

    fn uclamp(&self) -> (u32, u32) {
        (self.util_min, self.util_max)
    }

    pub fn is_realtime(&self) -> bool {
        matches!(
            self.policy,
            sched::SCHED_FIFO | sched::SCHED_RR | sched::SCHED_DEADLINE
        )
    }

    /// Policy, priority and nice: what ownership is decided on.
    fn class(&self) -> (u32, u32, i32) {
        (self.policy, self.priority, self.nice)
    }

    /// Policies scdr never moves a task out of on its own.
    pub fn is_protected(&self) -> bool {
        self.is_realtime() || self.policy == sched::SCHED_EXT
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskState {
    pub cpu: CpuState,
    pub ioprio: u32,
}

impl TaskState {
    pub fn read(tid: u32) -> io::Result<Self> {
        Ok(TaskState {
            cpu: CpuState::from_attr(&sched::get_attr(tid)?),
            ioprio: sched::get_ioprio(tid)?,
        })
    }
}

/// What a process looked like before scdr touched it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub task: TaskState,
    pub oom: Option<i32>,
}

impl Snapshot {
    pub fn read(pid: u32) -> io::Result<Self> {
        Ok(Snapshot {
            task: TaskState::read(pid)?,
            oom: sched::get_oom_score_adj(pid).ok(),
        })
    }

    /// Reads a process's state to restore later. Its slice is recorded as
    /// "kernel default", since a live read can't tell default from custom.
    pub fn original(pid: u32) -> io::Result<Self> {
        let mut s = Self::read(pid)?;
        s.task.cpu.slice_ns = 0;
        Ok(s)
    }

    /// This snapshot with a profile laid over it.
    pub fn with_profile(&self, p: &Profile) -> Snapshot {
        let mut cpu = self.task.cpu;
        cpu.slice_ns = 0;
        if let Some(policy) = p.sched {
            cpu.policy = policy.kernel_policy();
            cpu.priority = policy.rt_priority();
        }
        if !cpu.is_realtime() {
            cpu.priority = 0;
            if let Some(n) = p.nice {
                cpu.nice = n;
            }
            if let Some(s) = p.slice_us {
                cpu.slice_ns = u64::from(s) * 1000;
            }
        }
        if p.has_uclamp() {
            cpu.util_min = p.uclamp_min.unwrap_or(UCLAMP_DEFAULT.0);
            cpu.util_max = p.uclamp_max.unwrap_or(UCLAMP_DEFAULT.1);
        }
        Snapshot {
            task: TaskState {
                cpu,
                ioprio: p.io.map(|io| io.to_kernel()).unwrap_or(self.task.ioprio),
            },
            oom: p.oom_score_adj.or(self.oom),
        }
    }
}

impl fmt::Display for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = &self.task.cpu;
        match sched::Policy::from_kernel(c.policy, c.priority) {
            Some(p) => write!(f, "policy={p}")?,
            None if c.policy == sched::SCHED_DEADLINE => f.write_str("policy=deadline")?,
            None if c.policy == sched::SCHED_EXT => f.write_str("policy=ext")?,
            None => write!(f, "policy={}", c.policy)?,
        }
        if !c.is_realtime() {
            write!(f, " nice={}", c.nice)?;
            match c.slice_ns {
                0 => f.write_str(" slice=default")?,
                ns => write!(f, " slice={}us", ns / 1000)?,
            }
        }
        if c.uclamp() != UCLAMP_DEFAULT {
            write!(f, " uclamp={}..{}", c.util_min, c.util_max)?;
        }
        match sched::IoPrio::from_kernel(self.task.ioprio) {
            Some(io) => write!(f, " io={io}")?,
            None => f.write_str(" io=none")?,
        }
        if let Some(o) = self.oom {
            write!(f, " oom_score_adj={o}")?;
        }
        Ok(())
    }
}

/// Per-thread result of one apply pass.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub threads: usize,
    /// Threads that were changed (or would be, in dry-run mode).
    pub changed: usize,
    /// Threads already at the target.
    pub unchanged: usize,
    /// Threads with a value set by something else, left alone.
    pub foreign: usize,
    /// Realtime or sched_ext threads, left alone.
    pub protected: usize,
    pub errors: Vec<String>,
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} threads: {} changed, {} already set, {} set by something else, {} realtime/ext",
            self.threads, self.changed, self.unchanged, self.foreign, self.protected
        )?;
        if !self.errors.is_empty() {
            write!(f, ", {} errors", self.errors.len())?;
        }
        Ok(())
    }
}

/// Moves every thread of `pid` to `target`, touching only values that are
/// still in `owned`. With `dry_run`, only counts what would change.
pub fn apply(pid: u32, target: &Snapshot, owned: &[Snapshot], dry_run: bool) -> Outcome {
    let mut out = apply_threads(&procfs::tids(pid), target, owned, dry_run);

    // oom_score_adj is per process.
    if let (Some(want), Ok(cur)) = (target.oom, sched::get_oom_score_adj(pid)) {
        if cur != want && owned.iter().any(|o| o.oom == Some(cur)) && !dry_run {
            if let Err(e) = sched::set_oom_score_adj(pid, want) {
                push_err(&mut out, pid, "oom_score_adj", e);
            }
        }
    }

    out
}

/// `sched_getattr` always reports a fair task's slice, so a default slice
/// can't be told apart from a custom one by value. scdr only cares about
/// slices it sets: the target's (when non-zero), or clearing one it set before.
fn slice_ok(cur: &CpuState, target: &CpuState, owned: &[Snapshot]) -> bool {
    if target.is_realtime() {
        return true;
    }
    if target.slice_ns != 0 {
        return cur.slice_ns == target.slice_ns;
    }
    !owned
        .iter()
        .any(|o| o.task.cpu.slice_ns != 0 && o.task.cpu.slice_ns == cur.slice_ns)
}

/// uclamp, like slices, is only written when scdr sets it or clears its own.
fn uclamp_ok(cur: &CpuState, target: &CpuState, owned: &[Snapshot]) -> bool {
    if target.uclamp() != UCLAMP_DEFAULT {
        return cur.uclamp() == target.uclamp();
    }
    !owned
        .iter()
        .any(|o| o.task.cpu.uclamp() != UCLAMP_DEFAULT && o.task.cpu.uclamp() == cur.uclamp())
}

fn apply_threads(tids: &[u32], target: &Snapshot, owned: &[Snapshot], dry_run: bool) -> Outcome {
    let mut out = Outcome::default();

    for &tid in tids {
        let Ok(attr) = sched::get_attr(tid) else {
            continue; // thread exited
        };
        let Ok(ioprio) = sched::get_ioprio(tid) else {
            continue;
        };
        out.threads += 1;
        let cur = CpuState::from_attr(&attr);

        let t = target.task.cpu;
        let at_class = cur.class() == t.class();
        if cur.is_protected() && !at_class {
            out.protected += 1;
            continue;
        }

        // A thread someone else tuned (an app parking a worker at batch/19,
        // rtkit, renice) is left alone entirely, not just the tuned field.
        let cpu_owned = at_class || owned.iter().any(|o| o.task.cpu.class() == cur.class());
        let io_owned = ioprio == target.task.ioprio || owned.iter().any(|o| o.task.ioprio == ioprio);
        if !cpu_owned || !io_owned {
            out.foreign += 1;
            continue;
        }
        let uclamp_needed = !uclamp_ok(&cur, &t, owned);
        let cpu_needed = !at_class || !slice_ok(&cur, &t, owned) || uclamp_needed;
        let io_needed = ioprio != target.task.ioprio;
        if !cpu_needed && !io_needed {
            out.unchanged += 1;
            continue;
        }
        out.changed += 1;
        if dry_run {
            continue;
        }

        if cpu_needed {
            let mut new = SchedAttr {
                policy: t.policy,
                priority: t.priority,
                nice: t.nice,
                runtime: t.slice_ns,
                flags: if t.is_realtime() {
                    sched::SCHED_FLAG_RESET_ON_FORK
                } else {
                    0
                },
                ..attr
            };
            if uclamp_needed {
                new.flags |= sched::SCHED_FLAG_UTIL_CLAMP;
                // u32::MAX asks the kernel to reset a clamp to its default.
                (new.util_min, new.util_max) = if t.uclamp() == UCLAMP_DEFAULT {
                    (u32::MAX, u32::MAX)
                } else {
                    t.uclamp()
                };
            }
            if let Err(e) = sched::set_attr(tid, &new) {
                push_err(&mut out, tid, "sched_setattr", e);
            }
        }
        if io_needed {
            if let Err(e) = sched::set_ioprio(tid, target.task.ioprio) {
                push_err(&mut out, tid, "ioprio_set", e);
            }
        }
    }

    out
}

fn push_err(out: &mut Outcome, tid: u32, what: &str, e: io::Error) {
    // A thread exiting mid-pass is not an error.
    if e.raw_os_error() != Some(libc::ESRCH) && out.errors.len() < 8 {
        out.errors.push(format!("{what} on {tid}: {e}"));
    }
}

#[cfg(test)]
mod tests;
