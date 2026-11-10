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

use crate::procfs::ProcInfo;
use crate::rules::Matcher;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSplit {
    pub apps: Option<Vec<String>>,
    pub keep: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct Split {
    apps: Matcher,
    keep: Matcher,
    /// Whether any app is listed (cheap check before matching).
    pub enabled: bool,
}

impl Default for Split {
    fn default() -> Self {
        Split::compile(&[], &[]).expect("empty split compiles")
    }
}

impl Split {
    pub(crate) fn compile(apps: &[String], keep: &[String]) -> Result<Self, String> {
        Ok(Split {
            apps: Matcher::compile(apps)?,
            keep: Matcher::compile(keep)?,
            enabled: !apps.is_empty(),
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

/// Merges a file's `[split]` over what lower files set: lists replace.
pub(crate) fn merge(apps: &mut Vec<String>, keep: &mut Vec<String>, raw: RawSplit) {
    if let Some(a) = raw.apps {
        *apps = a;
    }
    if let Some(k) = raw.keep {
        *keep = k;
    }
}
