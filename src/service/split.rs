//! Splitting agent tools out of their app's scope (see `config::split`).
//!
//! For a process in a split app's scope whose binary isn't the app's own,
//! scdr finds its tool leader: the highest ancestor in the same scope that
//! is also not the app (a Claude Code session's wrapper, an MCP server).
//! The leader and its descendants then move into
//! `app-<app>-tools-<leader pid>.scope` through the unit worker; what they
//! start afterwards is born there. A process left behind (forked during the
//! move) gets a scope of its own when it is next evaluated.

use super::unit_jobs::UnitJob;
use super::{Service, Trigger};
use crate::procfs::{self, ProcInfo};
use crate::units::{self, UnitId};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// How long to leave a tool alone after systemd refused to move it.
const FAILED_BACKOFF: Duration = Duration::from_secs(60);
const MAX_DEPTH: usize = 32;

#[derive(Debug, Default)]
pub(super) struct SplitState {
    /// App unit -> (its cgroup, the app's own binary).
    app_exe: HashMap<String, (String, Option<String>)>,
    /// Tool leaders (pid, start time) queued or moved, and their scopes.
    leaders: HashMap<(u32, u64), String>,
    /// Every pid a queued or finished move took along.
    members: HashSet<u32>,
    failed: HashMap<(u32, u64), Instant>,
}

impl Service {
    /// After a scan, or the re-check 1.5 s after an exec: queue the move if
    /// `pid` is an agent tool still inside a split app's scope. Not at exec
    /// itself: apps run short commands all the time (Claude Desktop runs
    /// git), and those are gone by the re-check; children a tool starts in
    /// its first moments move with it.
    pub(super) fn maybe_split(&mut self, pid: u32, info: &ProcInfo, trigger: Trigger) {
        if !self.config.split.enabled || !matches!(trigger, Trigger::Scan | Trigger::Recheck) {
            return;
        }
        let Some(unit) = units::unit_name(&info.cgroup) else {
            return;
        };
        if !unit.starts_with("app-") || unit.contains("-tools-") {
            return;
        }
        let Some(app) = super::control::app_of_cgroup(&info.cgroup) else {
            return;
        };
        if !self.config.split.app(&app) || self.config.split.keeps(info) {
            return;
        }
        let (Some(user), Some(scope_cg)) = (
            UnitId::from_cgroup(&info.cgroup).and_then(|u| u.user),
            units::unit_cgroup(&info.cgroup),
        ) else {
            return;
        };
        let unit = unit.to_owned();
        let Some(app_exe) = self.app_exe(&unit, &scope_cg) else {
            return;
        };
        if info.exe.as_deref() == Some(app_exe.as_str()) {
            return; // the app itself
        }

        // Part of a move already on its way.
        if self.split.members.contains(&pid) {
            return;
        }
        let mut leader = tool_leader(info, &app_exe, &self.config.split);
        // Already moved: this one was forked while that happened.
        if self.split.leaders.contains_key(&leader) && leader.0 != pid {
            leader = (pid, info.start_time);
        }
        if self.split.leaders.contains_key(&leader) {
            return;
        }
        if self.split.failed.get(&leader).is_some_and(|t| t.elapsed() < FAILED_BACKOFF) {
            return;
        }
        let pids = tree_in_scope(&scope_cg, leader.0);
        if pids.is_empty() {
            return;
        }
        let tool_unit = format!("app-{app}-tools-{}.scope", leader.0);
        let what = format!("{app} tool {} ({})", procfs::comm(leader.0).unwrap_or_default(), leader.0);
        self.split.leaders.insert(leader, tool_unit.clone());
        self.split.members.extend(pids.iter().copied());
        self.queue_unit_job(UnitJob::Split {
            id: UnitId { unit: tool_unit, user: Some(user) },
            pids,
            what,
            dry_run: self.dry_run,
        });
    }

    /// A move finished: re-evaluate the processes in their new scope.
    pub(super) fn split_done(&mut self, id: &UnitId, pids: Vec<u32>, result: Result<(), String>) {
        match result {
            Ok(()) => {
                tracing::info!("moved {} agent tool processes into {}", pids.len(), id.unit);
                self.reevaluate(pids);
            }
            // It exited before the move: nothing to retry.
            Err(e) if e.contains("No such process") => {
                tracing::debug!("tools for {} exited before the move", id.unit);
                for p in &pids {
                    self.split.members.remove(p);
                }
            }
            Err(e) => {
                tracing::warn!("can't move tools into {}: {e}; retrying in {}s", id.unit, FAILED_BACKOFF.as_secs());
                let gone: Vec<(u32, u64)> =
                    self.split.leaders.iter().filter(|(_, u)| **u == id.unit).map(|(k, _)| *k).collect();
                for k in gone {
                    self.split.leaders.remove(&k);
                    self.split.failed.insert(k, Instant::now());
                }
                for p in &pids {
                    self.split.members.remove(p);
                }
            }
        }
    }

    /// After a rescan: forget apps and leaders that are gone.
    pub(super) fn sweep_split(&mut self) {
        self.split
            .app_exe
            .retain(|_, (cg, _)| std::path::Path::new(&format!("/sys/fs/cgroup{cg}")).exists());
        let procs = &self.procs;
        let alive = |k: &(u32, u64)| procs.get(&k.0).is_some_and(|e| e.start_time == k.1);
        self.split.leaders.retain(|k, _| alive(k));
        self.split.members.retain(|p| procs.contains_key(p));
        self.split.failed.retain(|k, t| alive(k) && t.elapsed() < FAILED_BACKOFF);
    }

    /// The binary of the app a scope belongs to: its main process's, which
    /// a scope name ending in a pid names (`app-foo-1234.scope`), else the
    /// scope's oldest process.
    fn app_exe(&mut self, unit: &str, scope_cg: &str) -> Option<String> {
        if let Some((_, exe)) = self.split.app_exe.get(unit) {
            return exe.clone();
        }
        let named = unit
            .trim_end_matches(".scope")
            .rsplit('-')
            .next()
            .and_then(|t| t.parse::<u32>().ok())
            .filter(|&p| ProcInfo::read(p).is_ok_and(|i| i.cgroup == scope_cg));
        let main = named.or_else(|| scope_pids(scope_cg).into_iter().min());
        let exe = main.and_then(|p| ProcInfo::read(p).ok()).and_then(|i| i.exe);
        self.split.app_exe.insert(unit.to_owned(), (scope_cg.to_owned(), exe.clone()));
        exe
    }
}

/// The highest ancestor of `info` (itself included) in the same scope that
/// is also not the app: the top of its tool tree.
fn tool_leader(info: &ProcInfo, app_exe: &str, split: &crate::config::Split) -> (u32, u64) {
    let mut leader = (info.pid, info.start_time);
    let mut ppid = info.ppid;
    for _ in 0..MAX_DEPTH {
        let Ok(parent) = ProcInfo::read(ppid) else {
            break;
        };
        let is_tool = parent.cgroup == info.cgroup
            && parent.exe.as_deref().is_some_and(|e| e != app_exe)
            && !split.keeps(&parent);
        if !is_tool {
            break;
        }
        leader = (parent.pid, parent.start_time);
        ppid = parent.ppid;
    }
    leader
}

fn scope_pids(scope_cg: &str) -> Vec<u32> {
    std::fs::read_to_string(format!("/sys/fs/cgroup{scope_cg}/cgroup.procs"))
        .map(|s| s.lines().filter_map(|l| l.parse().ok()).collect())
        .unwrap_or_default()
}

/// `leader` and its descendants among the scope's processes.
fn tree_in_scope(scope_cg: &str, leader: u32) -> Vec<u32> {
    let pids = scope_pids(scope_cg);
    if !pids.contains(&leader) {
        return Vec::new();
    }
    let parent: HashMap<u32, u32> = pids
        .iter()
        .filter_map(|&p| procfs::Stat::read(p).ok().map(|s| (p, s.ppid)))
        .collect();
    descendants(leader, &parent)
}

/// `root` and every pid whose parent chain (within `parent`) reaches it.
fn descendants(root: u32, parent: &HashMap<u32, u32>) -> Vec<u32> {
    let mut tree: HashSet<u32> = HashSet::from([root]);
    let mut grew = true;
    while grew {
        grew = false;
        for (&p, pp) in parent {
            if !tree.contains(&p) && tree.contains(pp) {
                tree.insert(p);
                grew = true;
            }
        }
    }
    let mut out: Vec<u32> = tree.into_iter().collect();
    out.sort_unstable();
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_tool_tree_is_its_leader_and_descendants() {
        // 10 app main; 11 renderer; 20 tool leader; 21, 22 its children; 23 grandchild.
        let parent = [(10, 1), (11, 10), (20, 10), (21, 20), (22, 20), (23, 21)].into_iter().collect();
        assert_eq!(super::descendants(20, &parent), vec![20, 21, 22, 23]);
        assert_eq!(super::descendants(11, &parent), vec![11]);
    }
}
