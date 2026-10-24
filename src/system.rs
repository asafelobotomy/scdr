//! System-wide scheduler facts and knobs.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;

const AUTOGROUP: &str = "/proc/sys/kernel/sched_autogroup_enabled";

pub fn autogroup() -> Option<bool> {
    fs::read_to_string(AUTOGROUP)
        .ok()
        .map(|s| s.trim() != "0")
}

pub fn set_autogroup(enabled: bool) -> io::Result<()> {
    fs::write(AUTOGROUP, if enabled { "1" } else { "0" })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchedExt {
    /// `enabled`, `disabled`, `enabling`, `disabling`.
    pub state: String,
    /// Name of the loaded BPF scheduler, when there is one.
    pub ops: Option<String>,
}

impl SchedExt {
    pub fn is_active(&self) -> bool {
        self.state == "enabled"
    }
}

/// `None` when the kernel has no sched_ext support.
pub fn sched_ext() -> Option<SchedExt> {
    let state = fs::read_to_string("/sys/kernel/sched_ext/state").ok()?;
    let ops = fs::read_to_string("/sys/kernel/sched_ext/root/ops")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    Some(SchedExt {
        state: state.trim().to_owned(),
        ops,
    })
}

/// `(device, active I/O scheduler)` for real block devices.
pub fn io_schedulers() -> Vec<(String, String)> {
    let Ok(entries) = fs::read_dir("/sys/block") else {
        return Vec::new();
    };
    let mut out: Vec<_> = entries
        .flatten()
        .filter_map(|e| {
            let dev = e.file_name().to_string_lossy().into_owned();
            if ["loop", "ram", "zram", "sr"].iter().any(|p| dev.starts_with(p)) {
                return None;
            }
            let s = fs::read_to_string(e.path().join("queue/scheduler")).ok()?;
            Some((dev, active_io_scheduler(&s)?))
        })
        .collect();
    out.sort();
    out
}

fn active_io_scheduler(s: &str) -> Option<String> {
    let start = s.find('[')?;
    let end = s[start..].find(']')? + start;
    Some(s[start + 1..end].to_owned())
}

/// Devices whose scheduler ignores per-task I/O priorities (everything but bfq).
pub fn ioprio_ignored_on() -> Vec<String> {
    io_schedulers()
        .into_iter()
        .filter(|(_, s)| s != "bfq")
        .map(|(d, s)| format!("{d} ({s})"))
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_active_scheduler() {
        assert_eq!(super::active_io_scheduler("[none] mq-deadline kyber bfq\n").as_deref(), Some("none"));
        assert_eq!(super::active_io_scheduler("none mq-deadline kyber [bfq]\n").as_deref(), Some("bfq"));
        assert_eq!(super::active_io_scheduler("none\n"), None);
    }
}
