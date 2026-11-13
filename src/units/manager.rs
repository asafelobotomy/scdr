//! Running `systemctl` / `busctl` against a user's manager or the system one.
//!
//! A user's manager only accepts its own user on its private socket, and
//! refuses root. `--machine=USER@` gets there through `systemd-run` and a
//! PAM login, which opens a full logind session per call (hundreds an hour
//! under CPU pressure, each announced to every logind client). So scdr runs
//! the tool as that user instead; that needs CAP_SETUID and CAP_SETGID.
//!
//! - The kernel refusing the uid switch applies to every user: from then on
//!   scdr uses `--machine` for all of them.
//! - A user with no manager running (no `/run/user/UID/systemd/private`, e.g.
//!   the login screen's greeter user) has no units to manage: the call fails
//!   quietly, without `--machine`, which would start a whole session for it.
//! - Any other failure to connect sends that one user to `--machine`; the
//!   others keep running as themselves.

use std::collections::BTreeSet;
use std::fmt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const SYSTEMCTL_TIMEOUT: Duration = Duration::from_secs(5);

/// Cleared the first time the kernel refuses the uid switch.
static AS_USER: AtomicBool = AtomicBool::new(true);
/// Users whose manager couldn't be reached as themselves.
static MACHINE_USERS: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// What to do after running as the user failed to reach its manager.
#[derive(Debug, PartialEq, Eq)]
enum Fallback {
    /// The uid switch was refused: `--machine` for everyone from now on.
    Everyone,
    /// No manager runs for this user: give up on this call.
    NoManager,
    /// `--machine` for this user from now on.
    ThisUser,
}

fn fallback(e: &RunError, manager_running: bool) -> Fallback {
    match e {
        RunError::Spawn(_) => Fallback::Everyone,
        RunError::Failed(_) if !manager_running => Fallback::NoManager,
        RunError::Failed(_) => Fallback::ThisUser,
    }
}

/// Runs `PROGRAM ARGS` (systemctl or busctl) against `user`'s manager, or
/// the system manager for `None`.
pub(super) fn manager_command(user: Option<&str>, program: &str, args: &[&str]) -> Result<String, String> {
    let extra: &[&str] = if program == "systemctl" { &["--no-ask-password"] } else { &[] };
    if let Some(user) = user {
        let machine = MACHINE_USERS.lock().unwrap().contains(user);
        if AS_USER.load(Ordering::Relaxed) && !machine {
            if let Some((uid, gid)) = user_ids(user) {
                let mut cmd = Command::new(program);
                cmd.uid(uid)
                    .gid(gid)
                    .env("XDG_RUNTIME_DIR", format!("/run/user/{uid}"))
                    .env_remove("DBUS_SESSION_BUS_ADDRESS")
                    .arg("--user")
                    .args(extra)
                    .args(args);
                match run(cmd) {
                    Err(e) if e.unreachable() => {
                        let running = Path::new(&format!("/run/user/{uid}/systemd/private")).exists();
                        match fallback(&e, running) {
                            Fallback::NoManager => return Err(format!("no user manager runs for {user}")),
                            Fallback::Everyone => {
                                AS_USER.store(false, Ordering::Relaxed);
                                tracing::warn!(
                                    "can't run {program} as {user} ({e}); using --machine for every user, which opens a login session per call"
                                );
                            }
                            Fallback::ThisUser => {
                                MACHINE_USERS.lock().unwrap().insert(user.to_owned());
                                tracing::warn!(
                                    "can't reach {user}'s manager as {user} ({e}); using --machine for {user}, which opens a login session per call"
                                );
                            }
                        }
                    }
                    other => return other.map_err(|e| e.to_string()),
                }
            }
        }
    }
    let mut cmd = Command::new(program);
    if let Some(user) = user {
        cmd.arg("--user").arg(format!("--machine={user}@"));
    }
    cmd.args(extra).args(args);
    run(cmd).map_err(|e| e.to_string())
}

/// A user's uid and primary gid.
fn user_ids(name: &str) -> Option<(u32, u32)> {
    let cname = std::ffi::CString::new(name).ok()?;
    let mut buf = vec![0 as libc::c_char; 1024];
    // SAFETY: zeroed passwd is a valid out-parameter.
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: all pointers are valid for the given sizes; result is checked.
    let rc = unsafe { libc::getpwnam_r(cname.as_ptr(), &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result) };
    (rc == 0 && !result.is_null()).then_some((pwd.pw_uid, pwd.pw_gid))
}

#[derive(Debug)]
pub(super) enum RunError {
    /// systemctl couldn't be started (or the uid switch was refused).
    Spawn(std::io::Error),
    Failed(String),
}

impl RunError {
    fn unreachable(&self) -> bool {
        match self {
            RunError::Spawn(e) => e.kind() == std::io::ErrorKind::PermissionDenied,
            RunError::Failed(msg) => msg.contains("Failed to connect"),
        }
    }
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunError::Spawn(e) => write!(f, "running systemctl: {e}"),
            RunError::Failed(e) => f.write_str(e),
        }
    }
}

pub(super) fn run(mut cmd: Command) -> Result<String, RunError> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(RunError::Spawn)?;
    let deadline = Instant::now() + SYSTEMCTL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunError::Failed("systemctl timed out".into()));
            }
            Err(e) => return Err(RunError::Failed(e.to_string())),
        }
    }
    let out = child.wait_with_output().map_err(|e| RunError::Failed(e.to_string()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(RunError::Failed(String::from_utf8_lossy(&out.stderr).trim().to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::{Fallback, RunError, fallback};

    #[test]
    fn only_a_refused_uid_switch_affects_every_user() {
        let refused = RunError::Spawn(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert_eq!(fallback(&refused, true), Fallback::Everyone);
        let connect = RunError::Failed("Failed to connect to user scope bus via local transport".into());
        // The greeter: no manager, nothing to manage, no --machine session.
        assert_eq!(fallback(&connect, false), Fallback::NoManager);
        assert_eq!(fallback(&connect, true), Fallback::ThisUser);
    }
}
