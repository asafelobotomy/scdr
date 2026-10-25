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

/// The distinct cpufreq governors in use, e.g. `["powersave"]`.
pub fn cpu_governors() -> Vec<String> {
    let Ok(entries) = fs::read_dir("/sys/devices/system/cpu/cpufreq") else {
        return Vec::new();
    };
    let mut govs: Vec<String> = entries
        .flatten()
        .filter_map(|e| fs::read_to_string(e.path().join("scaling_governor")).ok())
        .map(|g| g.trim().to_owned())
        .collect();
    govs.sort();
    govs.dedup();
    govs
}

/// uclamp only steers CPU frequency under the schedutil governor.
pub fn uclamp_effective() -> bool {
    cpu_governors().iter().any(|g| g == "schedutil")
}

const IOCOST_QOS: &str = "/sys/fs/cgroup/io.cost.qos";

/// A whole block device and which I/O priority levers it honours.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Disk {
    pub name: String,
    /// `MAJ:MIN`
    pub dev: String,
    pub scheduler: String,
    pub iocost: bool,
}

impl Disk {
    /// Per-task `io` priorities only work under bfq.
    pub fn honours_ioprio(&self) -> bool {
        self.scheduler == "bfq"
    }

    /// `IOWeight=` works under bfq or with iocost enabled.
    pub fn honours_io_weight(&self) -> bool {
        self.scheduler == "bfq" || self.iocost
    }

    pub fn lever(&self) -> &'static str {
        match (self.honours_ioprio(), self.iocost) {
            (true, _) => "io priorities and IOWeight",
            (false, true) => "IOWeight (iocost)",
            (false, false) => "nothing: io priorities and IOWeight are ignored",
        }
    }
}

/// Real block devices (no loop, ram, zram, optical).
pub fn disks() -> Vec<Disk> {
    let Ok(entries) = fs::read_dir("/sys/block") else {
        return Vec::new();
    };
    let iocost = iocost_state();
    let mut out: Vec<Disk> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if ["loop", "ram", "zram", "sr"].iter().any(|p| name.starts_with(p)) {
                return None;
            }
            let sched = fs::read_to_string(e.path().join("queue/scheduler")).ok()?;
            let dev = fs::read_to_string(e.path().join("dev")).ok()?.trim().to_owned();
            Some(Disk {
                iocost: iocost.iter().any(|(d, on)| *d == dev && *on),
                scheduler: active_io_scheduler(&sched)?,
                name,
                dev,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn active_io_scheduler(s: &str) -> Option<String> {
    let start = s.find('[')?;
    let end = s[start..].find(']')? + start;
    Some(s[start + 1..end].to_owned())
}

/// `(MAJ:MIN, enabled)` from io.cost.qos; empty when iocost was never set up.
fn iocost_state() -> Vec<(String, bool)> {
    fs::read_to_string(IOCOST_QOS)
        .map(|s| parse_iocost_qos(&s))
        .unwrap_or_default()
}

fn parse_iocost_qos(s: &str) -> Vec<(String, bool)> {
    s.lines()
        .filter_map(|l| {
            let mut parts = l.split_ascii_whitespace();
            let dev = parts.next()?.to_owned();
            let on = parts.any(|p| p == "enable=1");
            Some((dev, on))
        })
        .collect()
}

pub fn set_iocost(dev: &str, enable: bool) -> io::Result<()> {
    fs::write(IOCOST_QOS, format!("{dev} enable={}", enable as u8))
}

/// Disks that ignore per-task I/O priorities, as "name (scheduler)".
pub fn ioprio_ignored_on() -> Vec<String> {
    disks()
        .into_iter()
        .filter(|d| !d.honours_ioprio())
        .map(|d| format!("{} ({})", d.name, d.scheduler))
        .collect()
}

/// Disks that ignore `IOWeight=`.
pub fn io_weight_ignored_on() -> Vec<String> {
    disks()
        .into_iter()
        .filter(|d| !d.honours_io_weight())
        .map(|d| format!("{} ({}, no iocost)", d.name, d.scheduler))
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

    #[test]
    fn parses_iocost_qos() {
        let qos = "259:0 enable=1 ctrl=auto rpct=0.00 rlat=250000\n8:0 enable=0 ctrl=auto\n";
        assert_eq!(
            super::parse_iocost_qos(qos),
            vec![("259:0".to_owned(), true), ("8:0".to_owned(), false)]
        );
        assert!(super::parse_iocost_qos("").is_empty());
    }
}
