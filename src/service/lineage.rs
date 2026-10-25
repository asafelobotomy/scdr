//! Launcher detection: which processes were started by something that
//! hand-tunes priorities, so scdr leaves them and their descendants alone.

use super::Service;
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
pub(super) fn hand_tuned(pid: u32, ppid: u32) -> Option<String> {
    let me = apply::TaskState::read(pid).ok()?;
    let parent = apply::TaskState::read(ppid).ok()?;
    let cpu_differs = me.cpu.policy != parent.cpu.policy || me.cpu.nice != parent.cpu.nice;
    (cpu_differs || me.ioprio != parent.ioprio)
        .then(|| "priorities differ from its parent's (started via nice/chrt/ionice?)".to_owned())
}
