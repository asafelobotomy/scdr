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

mod manager;
use manager::manager_command;

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

    /// Runs `systemctl ARGS` against the manager that owns this unit.
    fn systemctl(&self, args: &[&str]) -> Result<String, String> {
        manager_command(self.user.as_deref(), "systemctl", args)
    }
}

/// Moves running processes of `user` into a new transient scope of their
/// manager (as `systemd-run --scope` does for itself), in `slice`. What they
/// start later stays in it.
pub fn start_scope(user: &str, unit: &str, slice: &str, pids: &[u32], description: &str) -> Result<(), String> {
    let count = pids.len().to_string();
    let pids: Vec<String> = pids.iter().map(u32::to_string).collect();
    let mut args = vec![
        "call",
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
        "StartTransientUnit",
        "ssa(sv)a(sa(sv))",
        unit,
        "fail",
        "4",
        "PIDs",
        "au",
        &count,
    ];
    args.extend(pids.iter().map(String::as_str));
    args.extend(["Description", "s", description, "Slice", "s", slice]);
    args.extend(["CollectMode", "s", "inactive-or-failed", "0"]);
    manager_command(Some(user), "busctl", &args).map(|_| ())
}

/// The name of the unit a process belongs to (`app-firefox-12.scope`),
/// without resolving which manager owns it.
pub fn unit_name(cgroup: &str) -> Option<&str> {
    let unit = cgroup
        .split('/')
        .filter(|p| !p.is_empty())
        .rfind(|p| p.ends_with(".service") || p.ends_with(".scope"))?;
    (unit != "init.scope" && !unit.starts_with("user@")).then_some(unit)
}

/// The cgroup of the unit a process belongs to: its path cut after the
/// unit's own component (a process may sit in a delegated sub-cgroup).
pub fn unit_cgroup(cgroup: &str) -> Option<String> {
    let parts: Vec<&str> = cgroup.split('/').filter(|p| !p.is_empty()).collect();
    let at = parts
        .iter()
        .rposition(|p| p.ends_with(".service") || p.ends_with(".scope"))?;
    Some(format!("/{}", parts[..=at].join("/")))
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

/// A uid's user name, cached: unit ids are resolved on every exec of a
/// managed process, and getpwuid re-reads /etc/passwd each time.
fn user_name(uid: u32) -> Option<String> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<u32, Option<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().unwrap().get(&uid) {
        return hit.clone();
    }
    let name = lookup_user_name(uid);
    // A user without a name now may get one (sssd, systemd-homed): only cache hits.
    if name.is_some() {
        cache.lock().unwrap().insert(uid, name.clone());
    }
    name
}

fn lookup_user_name(uid: u32) -> Option<String> {
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
    parse_show(&id.systemctl(&["show", "-p", "CPUWeight", "-p", "IOWeight", "--", &id.unit])?)
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
    set_properties(id, &props)
}

/// `systemctl set-property --runtime` on `id`.
pub fn set_properties(id: &UnitId, props: &[String]) -> Result<(), String> {
    if props.is_empty() {
        return Ok(());
    }
    let mut args = vec!["set-property", "--runtime", "--", id.unit.as_str()];
    args.extend(props.iter().map(String::as_str));
    id.systemctl(&args).map(|_| ())
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
        assert_eq!(unit_name(&app), Some("app-firefox-12.scope"));
        assert_eq!(unit_name("/system.slice/docker-ab.scope/inner"), Some("docker-ab.scope"));
        assert_eq!(unit_name("/user.slice/user-1000.slice/user@1000.service/init.scope"), None);
        assert_eq!(
            unit_cgroup("/system.slice/docker-ab.scope/inner").as_deref(),
            Some("/system.slice/docker-ab.scope")
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
                let mut cmd = std::process::Command::new("systemctl");
                cmd.args(["--user", "show", "-p", "CPUWeight", "-p", "IOWeight", "--"]).arg(&id.unit);
                if let Ok(out) = manager::run(cmd) {
                    parse_show(&out).unwrap();
                }
            }
        }
    }
}
