//! `[split]`: apps whose helper processes are agent tools, not the app.
//!
//! An agent app (Claude Desktop) runs its tools inside its own app scope:
//! Claude Code sessions, their shells, test runs, compilers, VMs and MCP
//! servers. Sharing the scope makes them part of "the focused app" for
//! every per-unit control (scx_devbox's focus, CPU/IO weights, memory
//! limits). For the apps listed here, scdr moves each tool's process tree
//! into a scope of its own (`app-<app>-tools-<pid>.scope`), so the app's
//! own processes (its UI) and its tools can be treated apart.
//!
//! A tool is a process running a different binary from the app itself;
//! `keep` lists helpers that stay with the app anyway (crash reporters,
//! sandbox helpers). A file that sets a list replaces it, so a later file
//! can turn splitting off with `apps = []`.
//!
//! Each app's tool scopes share one slice, `app-<app>-tools.slice` (shown
//! by systemd as `app-<app>\x2dtools.slice`), which takes the limits set
//! here for all of them together: `memory_high` (systemd's `MemoryHigh=`,
//! bytes with K/M/G/T or a % of RAM), `io_weight` and `cpu_weight`.

use crate::procfs::ProcInfo;
use crate::rules::Matcher;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSplit {
    pub apps: Option<Vec<String>>,
    pub keep: Option<Vec<String>>,
    pub memory_high: Option<String>,
    pub io_weight: Option<u64>,
    pub cpu_weight: Option<u64>,
}

/// Limits for an app's tools slice, as systemd properties.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SliceLimits {
    pub memory_high: Option<String>,
    pub io_weight: Option<u64>,
    pub cpu_weight: Option<u64>,
}

impl SliceLimits {
    pub fn is_empty(&self) -> bool {
        *self == SliceLimits::default()
    }

    /// `set-property` arguments for these limits; unset ones go back to
    /// systemd's defaults, so the empty set undoes any earlier one.
    pub fn properties(&self) -> Vec<String> {
        let w = |v: Option<u64>| v.map(|v| v.to_string()).unwrap_or_default();
        vec![
            format!("MemoryHigh={}", self.memory_high.as_deref().unwrap_or("infinity")),
            format!("IOWeight={}", w(self.io_weight)),
            format!("CPUWeight={}", w(self.cpu_weight)),
        ]
    }

    fn validate(&self) -> Result<(), String> {
        for (key, v) in [("io_weight", self.io_weight), ("cpu_weight", self.cpu_weight)] {
            if v.is_some_and(|v| !(1..=10000).contains(&v)) {
                return Err(format!("{key} must be 1..10000"));
            }
        }
        match self.memory_high.as_deref() {
            None | Some("infinity") => Ok(()),
            Some(m) if memory_size(m) => Ok(()),
            Some(m) => Err(format!("memory_high {m:?}: use bytes with K/M/G/T, a percentage or \"infinity\"")),
        }
    }
}

fn memory_size(s: &str) -> bool {
    if let Some(p) = s.strip_suffix('%') {
        return p.parse::<f64>().is_ok_and(|p| p > 0.0 && p <= 100.0);
    }
    let digits = s.trim_end_matches(['K', 'M', 'G', 'T']);
    s.len() - digits.len() <= 1 && digits.parse::<u64>().is_ok_and(|n| n > 0)
}

#[derive(Debug, Clone)]
pub struct Split {
    apps: Matcher,
    keep: Matcher,
    /// Whether any app is listed (cheap check before matching).
    pub enabled: bool,
    pub limits: SliceLimits,
}

impl Default for Split {
    fn default() -> Self {
        Split::compile(&[], &[], SliceLimits::default()).expect("empty split compiles")
    }
}

impl Split {
    pub(crate) fn compile(apps: &[String], keep: &[String], limits: SliceLimits) -> Result<Self, String> {
        limits.validate()?;
        Ok(Split {
            apps: Matcher::compile(apps)?,
            keep: Matcher::compile(keep)?,
            enabled: !apps.is_empty(),
            limits,
        })
    }

    /// Whether the app (as named from its app.slice unit) has its tools split off.
    pub fn app(&self, name: &str) -> bool {
        self.enabled && self.apps.is_match(name)
    }

    /// Helpers that stay in the app's scope even though their binary differs.
    pub fn keeps(&self, p: &ProcInfo) -> bool {
        self.keep.any(&p.names())
    }
}

/// What the config files set, merged.
#[derive(Debug, Default)]
pub(crate) struct Merged {
    pub apps: Vec<String>,
    pub keep: Vec<String>,
    pub limits: SliceLimits,
}

/// Merges a file's `[split]` over what lower files set: lists and values replace.
pub(crate) fn merge(m: &mut Merged, raw: RawSplit) {
    if let Some(a) = raw.apps {
        m.apps = a;
    }
    if let Some(k) = raw.keep {
        m.keep = k;
    }
    m.limits.memory_high = raw.memory_high.or(m.limits.memory_high.take());
    m.limits.io_weight = raw.io_weight.or(m.limits.io_weight);
    m.limits.cpu_weight = raw.cpu_weight.or(m.limits.cpu_weight);
}
