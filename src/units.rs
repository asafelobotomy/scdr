//! systemd unit weights (`CPUWeight=`, `IOWeight=`), set through systemd.
//!
//! The cpu and io controllers are usually off inside `app.slice`,
//! `session.slice` and `system.slice`, so there is no `cpu.weight` file to
//! write. Asking systemd for the property enables the controller where
//! needed and keeps systemd the owner of the cgroup tree. User units belong
//! to that user's `systemd --user`, reached as root with `--machine=USER@`.
//! Properties are set `--runtime`, so nothing survives a reboot.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SYSTEMCTL_TIMEOUT: Duration = Duration::from_secs(5);

/// A unit and the manager that owns it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UnitId {
    pub unit: String,
    /// `None` for the system manager, else the user whose manager owns it.
    pub user: Option<String>,
}

impl UnitId {
    /// The unit a process belongs to, from its cgroup path.
    pub fn from_cgroup(cgroup: &str) -> Option<Self> {
        let parts: Vec<&str> = cgroup.split('/').filter(|p| !p.is_empty()).collect();
        let at = parts
            .iter()
            .rposition(|p| p.ends_with(".service") || p.ends_with(".scope"))?;
        let unit = parts[at];
        // A manager's own process, not a unit we should weigh.
        if unit == "init.scope" || unit.starts_with("user@") {
            return None;
        }
        let user = match parts[..at].iter().find_map(|p| user_manager_uid(p)) {
            Some(uid) => Some(user_name(uid)?),
            None => None,
        };
        Some(UnitId {
            unit: unit.to_owned(),
            user,
        })
    }

    fn systemctl(&self) -> Command {
        let mut cmd = Command::new("systemctl");
        if let Some(user) = &self.user {
            cmd.arg("--user").arg(format!("--machine={user}@"));
        }
        cmd.arg("--no-ask-password");
        cmd
    }
}

impl fmt::Display for UnitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.user {
            Some(u) => write!(f, "{} (user {u})", self.unit),
            None => write!(f, "{} (system)", self.unit),
        }
    }
}

/// `user@1000.service` -> 1000.
fn user_manager_uid(part: &str) -> Option<u32> {
    part.strip_prefix("user@")?.strip_suffix(".service")?.parse().ok()
}

fn user_name(uid: u32) -> Option<String> {
    let mut buf = vec![0 as libc::c_char; 1024];
    // SAFETY: zeroed passwd is a valid out-parameter.
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: all pointers are valid for the given sizes; result is checked.
    let rc = unsafe { libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result) };
    if rc != 0 || result.is_null() {
        return None;
    }
    // SAFETY: getpwuid_r filled pw_name with a NUL-terminated string in buf.
    let name = unsafe { std::ffi::CStr::from_ptr(pwd.pw_name) };
    Some(name.to_string_lossy().into_owned())
}

/// A unit's weights; `None` means "not set" (systemd's default, 100).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Weights {
    pub cpu: Option<u64>,
    pub io: Option<u64>,
}

impl fmt::Display for Weights {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let show = |w: Option<u64>| w.map_or("default".to_owned(), |w| w.to_string());
        write!(f, "CPUWeight={} IOWeight={}", show(self.cpu), show(self.io))
    }
}

pub fn get(id: &UnitId) -> Result<Weights, String> {
    let mut cmd = id.systemctl();
    cmd.args(["show", "-p", "CPUWeight", "-p", "IOWeight", "--"]).arg(&id.unit);
    parse_show(&run(cmd)?)
}

fn parse_show(out: &str) -> Result<Weights, String> {
    let mut w = Weights::default();
    for line in out.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let parsed = match value {
            "" | "[not set]" | "18446744073709551615" => None,
            v => Some(v.parse::<u64>().map_err(|_| format!("unsupported {key}={v}"))?),
        };
        match key {
            "CPUWeight" => w.cpu = parsed,
            "IOWeight" => w.io = parsed,
            _ => {}
        }
    }
    Ok(w)
}

/// Sets the weights that differ from `current`. `None` resets to default.
pub fn set(id: &UnitId, want: &Weights, current: &Weights) -> Result<(), String> {
    let mut props = Vec::new();
    let fmt = |w: Option<u64>| w.map(|w| w.to_string()).unwrap_or_default();
    if want.cpu != current.cpu {
        props.push(format!("CPUWeight={}", fmt(want.cpu)));
    }
    if want.io != current.io {
        props.push(format!("IOWeight={}", fmt(want.io)));
    }
    if props.is_empty() {
        return Ok(());
    }
    let mut cmd = id.systemctl();
    cmd.args(["set-property", "--runtime", "--"]).arg(&id.unit).args(&props);
    run(cmd).map(|_| ())
}

fn run(mut cmd: Command) -> Result<String, String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("running systemctl: {e}"))?;
    let deadline = Instant::now() + SYSTEMCTL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("systemctl timed out".into());
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_from_cgroup() {
        let me = user_name(unsafe { libc::getuid() }).unwrap();
        let uid = unsafe { libc::getuid() };
        let app = format!("/user.slice/user-{uid}.slice/user@{uid}.service/app.slice/app-firefox-12.scope");
        assert_eq!(
            UnitId::from_cgroup(&app),
            Some(UnitId { unit: "app-firefox-12.scope".into(), user: Some(me) })
        );
        assert_eq!(
            UnitId::from_cgroup("/system.slice/NetworkManager.service"),
            Some(UnitId { unit: "NetworkManager.service".into(), user: None })
        );
        assert_eq!(
            UnitId::from_cgroup("/user.slice/user-1000.slice/session-2.scope").unwrap().user,
            None
        );
        // Delegated subtrees belong to the unit above them.
        assert_eq!(
            UnitId::from_cgroup("/system.slice/docker-ab.scope/inner").unwrap().unit,
            "docker-ab.scope"
        );
        for skip in ["/init.scope", "/user.slice/user-1000.slice/user@1000.service/init.scope", "/system.slice", ""] {
            assert_eq!(UnitId::from_cgroup(skip), None, "{skip}");
        }
    }

    #[test]
    fn parse_systemctl_show() {
        assert_eq!(
            parse_show("CPUWeight=[not set]\nIOWeight=200\n").unwrap(),
            Weights { cpu: None, io: Some(200) }
        );
        assert_eq!(parse_show("CPUWeight=\n").unwrap(), Weights::default());
        assert!(parse_show("CPUWeight=idle\n").is_err());
    }

    #[test]
    fn reads_own_scope_weights() {
        // Works unprivileged: our own unit belongs to our own manager.
        let cg = std::fs::read_to_string("/proc/self/cgroup").unwrap();
        let path = cg.lines().find_map(|l| l.strip_prefix("0::")).unwrap();
        if let Some(id) = UnitId::from_cgroup(path) {
            if id.user.is_some() {
                let mut cmd = Command::new("systemctl");
                cmd.args(["--user", "show", "-p", "CPUWeight", "-p", "IOWeight", "--"]).arg(&id.unit);
                if let Ok(out) = run(cmd) {
                    parse_show(&out).unwrap();
                }
            }
        }
    }
}
