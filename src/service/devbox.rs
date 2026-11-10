//! Delegating the focus roles to scx_devbox.
//!
//! Without help, scdr boosts the focused app by renicing and re-slicing
//! every thread of every process in it, and asks systemd to change its
//! unit's CPUWeight; under CPU pressure it does the same to every other app
//! (the background role). Each focus change costs hundreds of syscalls and
//! a couple of systemctl runs, and lands after the click.
//!
//! scx_devbox can do both in the scheduler instead. When it runs and its
//! loader announces the hints API (/run/scx_devbox/api), scdr writes the
//! focused app's cgroup, each user's app.slice and agent tools slices (as
//! neutral: never held back) to `devbox-hints` in its
//! runtime directory; the loader turns them into cgroup ids in the
//! scheduler's memory. The foreground and background roles then step aside:
//! a focus change is one small file write, and unfocused apps are weighted
//! down only while the CPUs are actually contended, measured by the
//! scheduler at each enqueue. Rules, and the audio and game roles, are
//! unchanged. If devbox goes away, scdr takes the roles back.

use super::Service;
use super::roles::{app_slice, cgroup_pids};
use crate::system;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const API: &str = "/run/scx_devbox/api";
const STATS: &str = "/run/scx_devbox/stats";

/// One second of scx_devbox's scheduling, from its stats file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DevboxSample {
    pub seq: u64,
    pub time: i64,
    /// Enqueues per second: interactive, batch, of focused tasks, and of
    /// unfocused apps' tasks damped under contention.
    pub interactive: u64,
    pub batch: u64,
    pub focus: u64,
    pub damped: u64,
    /// Batch tasks preempted for interactive wakeups; starvation rescues.
    pub preempt: u64,
    pub rescue: u64,
    /// Mean CPU frequency target, percent.
    pub perf_pct: u32,
    /// How long focused tasks waited from queued to running, ms.
    pub wait_max_ms: f64,
    pub wait_avg_ms: f64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct StatsFile {
    samples: Vec<DevboxSample>,
}

/// Samples in @text newer than @last_seq (all of them if the loader
/// restarted and its sequence went back), and the newest seq seen.
fn new_samples(text: &str, last_seq: u64) -> (Vec<DevboxSample>, u64) {
    let Ok(file) = serde_json::from_str::<StatsFile>(text) else {
        return (Vec::new(), last_seq);
    };
    let newest = file.samples.iter().map(|s| s.seq).max().unwrap_or(0);
    let from = if newest < last_seq { 0 } else { last_seq };
    let fresh: Vec<DevboxSample> = file.samples.into_iter().filter(|s| s.seq > from).collect();
    (fresh, newest.max(if newest < last_seq { 0 } else { last_seq }))
}

/// How long devbox may be gone before scdr takes the roles back. Restarting
/// it (a package upgrade) takes a second or two; taking over for that window
/// re-weights every app, and leaves the machine on EEVDF with each app's
/// tools competing with its UI. The hints file stays, so devbox picks the
/// focus straight back up when it returns.
const GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// Whether to stay delegated, given whether devbox takes hints right now.
fn keep_delegating(
    delegated: bool,
    available: bool,
    gone_since: &mut Option<std::time::Instant>,
    now: std::time::Instant,
) -> bool {
    if available {
        *gone_since = None;
        return true;
    }
    if !delegated {
        return false;
    }
    let since = *gone_since.get_or_insert(now);
    now.duration_since(since) < GRACE
}
const SUPPORTED_API: &str = "1";
pub const HINTS_FILE: &str = "devbox-hints";

/// Whether scx_devbox is the running sched_ext scheduler and takes hints.
fn devbox_takes_hints() -> bool {
    let running = system::sched_ext()
        .filter(system::SchedExt::is_active)
        .and_then(|s| s.ops)
        .is_some_and(|ops| ops == "devbox");
    running && fs::read_to_string(API).is_ok_and(|v| v.trim() == SUPPORTED_API)
}

impl Service {
    /// From the policy tick: switch delegation on or off as devbox comes
    /// and goes, and keep the hints current.
    pub fn sync_devbox(&mut self) {
        let eligible = !self.dry_run && !self.paused && self.hints_path.is_some();
        let on = eligible
            && keep_delegating(
                self.devbox_hints,
                devbox_takes_hints(),
                &mut self.devbox_gone_since,
                std::time::Instant::now(),
            );
        if on != self.devbox_hints {
            self.devbox_hints = on;
            tracing::info!(
                "scx_devbox focus hints {}: foreground and background roles {}",
                if on { "on" } else { "off" },
                if on { "handled in the scheduler" } else { "applied by scdr again" }
            );
            // Processes boosted or demoted by those roles change hands.
            let mut pids = self.role_pids();
            for uid in self.session_uids() {
                pids.extend(cgroup_pids(app_slice(uid).trim_end_matches('/')));
            }
            self.reevaluate(pids);
        }
        self.write_hints();
        self.read_devbox_stats();
    }

    /// The samples devbox wrote since the last tick, for the widget.
    fn read_devbox_stats(&mut self) {
        let running = system::sched_ext()
            .filter(system::SchedExt::is_active)
            .and_then(|s| s.ops)
            .is_some_and(|ops| ops == "devbox");
        let text = if running { fs::read_to_string(STATS).unwrap_or_default() } else { String::new() };
        let (fresh, seq) = new_samples(&text, self.devbox_seq);
        self.devbox_samples = fresh;
        self.devbox_seq = seq;
    }

    /// Whether the foreground and background roles are left to devbox.
    pub fn devbox_hints(&self) -> bool {
        self.devbox_hints
    }

    /// The hints for the current sessions; removes the file when off.
    pub(super) fn write_hints(&mut self) {
        let Some(path) = self.hints_path.clone() else {
            return;
        };
        if !self.devbox_hints {
            if self.hints_written.take().is_some() {
                let _ = fs::remove_file(&path);
            }
            return;
        }
        let mut lines: Vec<String> = self
            .sessions
            .values()
            .filter_map(|s| s.foreground.as_ref().map(|(cg, _)| format!("focus {cg}")))
            .collect();
        lines.sort();
        lines.dedup();
        for uid in self.session_uids() {
            lines.push(format!("apps {}", app_slice(uid).trim_end_matches('/')));
        }
        // Agent tools: work the user waits on, so neither boosted nor held back.
        lines.extend(self.tools_slice_cgroups().into_iter().map(|cg| format!("neutral {cg}")));
        let text = lines.join("\n") + "\n";
        if self.hints_written.as_deref() == Some(text.as_str()) {
            return;
        }
        match write_atomic(&path, &text) {
            Ok(()) => self.hints_written = Some(text),
            Err(e) => tracing::warn!("can't write {}: {e}", path.display()),
        }
    }

    fn session_uids(&self) -> Vec<u32> {
        let mut uids: Vec<u32> = self.sessions.values().map(|s| s.uid).collect();
        uids.sort_unstable();
        uids.dedup();
        uids
    }
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    #[test]
    fn stats_samples_are_taken_once() {
        let file = |seqs: &[u64]| {
            let s: Vec<String> = seqs.iter().map(|q| format!(r#"{{"seq":{q},"wait_max_ms":1.5}}"#)).collect();
            format!(r#"{{"samples":[{}]}}"#, s.join(","))
        };
        let (got, seq) = super::new_samples(&file(&[1, 2, 3]), 0);
        assert_eq!((got.len(), seq), (3, 3));
        assert_eq!(got[0].wait_max_ms, 1.5);
        // Next tick: only what's new.
        let (got, seq) = super::new_samples(&file(&[2, 3, 4, 5]), seq);
        assert_eq!(got.iter().map(|s| s.seq).collect::<Vec<_>>(), vec![4, 5]);
        // The loader restarted: its sequence starts again.
        let (got, seq) = super::new_samples(&file(&[1, 2]), seq);
        assert_eq!((got.len(), seq), (2, 2));
        // Unreadable or missing: nothing, and the position is kept.
        assert_eq!(super::new_samples("", 7), (Vec::new(), 7));
    }

    #[test]
    fn a_devbox_restart_doesnt_hand_the_roles_back() {
        use super::{GRACE, keep_delegating};
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let mut gone = None;
        assert!(keep_delegating(false, true, &mut gone, t0));
        // devbox restarting: still delegated within the grace period...
        assert!(keep_delegating(true, false, &mut gone, t0 + Duration::from_secs(1)));
        assert!(keep_delegating(true, false, &mut gone, t0 + Duration::from_secs(5)));
        // ...and back before it ran out: the clock resets.
        assert!(keep_delegating(true, true, &mut gone, t0 + Duration::from_secs(6)));
        assert_eq!(gone, None);
        // Gone for good: handed back once the grace period is over.
        assert!(keep_delegating(true, false, &mut gone, t0 + Duration::from_secs(7)));
        assert!(!keep_delegating(true, false, &mut gone, t0 + Duration::from_secs(7) + GRACE));
        // Never delegated: nothing to keep.
        assert!(!keep_delegating(false, false, &mut None, t0));
    }
    use crate::service::Service;

    #[test]
    fn hints_list_focus_and_app_slices() {
        let dir = std::env::temp_dir().join(format!("scdr-hints-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(super::HINTS_FILE);
        let mut svc = Service::new(Config::from_sources(Vec::new()).unwrap(), false);
        svc.hints_path = Some(path.clone());
        svc.devbox_hints = true;
        svc.session(1000, ":1.5").foreground = Some(("/user.slice/x/app.slice/app-a-1.scope".into(), 42));
        svc.write_hints();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "focus /user.slice/x/app.slice/app-a-1.scope\n\
             apps /user.slice/user-1000.slice/user@1000.service/app.slice\n"
        );
        // Off again: the file goes, so a devbox reading it falls back too.
        svc.devbox_hints = false;
        svc.write_hints();
        assert!(!path.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
