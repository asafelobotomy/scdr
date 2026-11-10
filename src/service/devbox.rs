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
//! focused app's cgroup and each user's app.slice to `devbox-hints` in its
//! runtime directory; the loader turns them into cgroup ids in the
//! scheduler's memory. The foreground and background roles then step aside:
//! a focus change is one small file write, and unfocused apps are weighted
//! down only while the CPUs are actually contended, measured by the
//! scheduler at each enqueue. Rules, and the audio and game roles, are
//! unchanged. If devbox goes away, scdr takes the roles back.

use super::Service;
use super::roles::{app_slice, cgroup_pids};
use crate::system;
use std::fs;
use std::path::Path;

const API: &str = "/run/scx_devbox/api";

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
