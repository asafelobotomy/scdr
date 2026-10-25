//! Runtime control from the panel widget and `scdrctl`: pause/resume and
//! switching roles on or off. Both persist in $STATE_DIRECTORY (systemd's
//! StateDirectory=scdr, i.e. /var/lib/scdr) so they survive a restart.

use super::{Service, State};
use crate::apply;
use crate::config::Role;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Runtime {
    paused: bool,
    roles_off: BTreeSet<String>,
}

/// One managed process, for `Processes()`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessReport {
    pub pid: u32,
    pub name: String,
    pub profile: String,
    /// The role that applied, or `None` when a rule did.
    pub role: Option<String>,
    /// The rule's file and conditions, or the role's description.
    pub why: String,
    /// The desktop app it belongs to (from its app.slice unit), if any.
    pub app: Option<String>,
}

/// The compact summary behind the `State` D-Bus property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetState {
    pub paused: bool,
    pub managed: usize,
    pub power_profile: Option<String>,
    pub cpu_pressure: Option<f64>,
    pub pressure_active: bool,
    pub scheduler: Vec<String>,
    /// role -> (profile, enabled); roles without a profile are omitted.
    pub roles: Vec<(String, String, bool)>,
    pub agents: usize,
    pub boosted: Vec<ProcessReport>,
}

impl Service {
    /// Puts back every process, unit and system knob scdr changed.
    pub(super) fn restore_all(&mut self) {
        let mut reverted = 0;
        for (pid, entry) in self.procs.iter_mut() {
            if let State::Managed(m) = &entry.state {
                if !self.dry_run {
                    apply::apply(*pid, &m.original, &[m.target], false);
                }
                reverted += 1;
                entry.state = State::Unmatched;
            }
        }
        tracing::info!("restored {reverted} processes");
        self.restore_units();
        self.restore_system();
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Restores everything scdr changed and stops changing anything.
    pub fn pause(&mut self) {
        if self.paused {
            return;
        }
        self.restore_all();
        self.paused = true;
        tracing::info!("paused");
        self.save_runtime();
    }

    pub fn resume(&mut self) {
        if !self.paused {
            return;
        }
        self.paused = false;
        tracing::info!("resumed");
        self.save_runtime();
        self.apply_system();
        self.rescan();
    }

    pub fn set_role_enabled(&mut self, role: Role, enabled: bool) {
        let changed = if enabled { self.roles_off.remove(&role) } else { self.roles_off.insert(role) };
        if changed {
            tracing::info!("{role} role {}", if enabled { "on" } else { "off" });
            self.save_runtime();
            self.rescan();
        }
    }

    pub(super) fn role_enabled(&self, role: Role) -> bool {
        !self.roles_off.contains(&role)
    }

    pub fn processes(&self) -> Vec<ProcessReport> {
        let mut out: Vec<ProcessReport> = self
            .procs
            .iter()
            .filter_map(|(pid, e)| match &e.state {
                State::Managed(m) => {
                    let role = (m.rule.source == "role")
                        .then(|| m.rule.conditions.split(':').next().unwrap_or("").to_owned());
                    let why = if role.is_some() {
                        m.rule.conditions.clone()
                    } else {
                        format!("{} #{}: {}", m.rule.source, m.rule.index, m.rule.conditions)
                    };
                    Some(ProcessReport {
                        pid: *pid,
                        name: crate::procfs::comm(*pid).unwrap_or_default(),
                        profile: m.profile.clone(),
                        role,
                        why,
                        app: app_of(*pid),
                    })
                }
                _ => None,
            })
            .collect();
        out.sort_by_key(|p| p.pid);
        out
    }

    pub fn widget_state(&self) -> WidgetState {
        let processes = self.processes();
        let roles = self.config.roles.for_power(self.power_profile.as_deref());
        WidgetState {
            paused: self.paused,
            managed: processes.len(),
            power_profile: self.power_profile.clone(),
            // Whole percent: State is re-sent to every listener when it changes.
            cpu_pressure: self.cpu_pressure.map(f64::round),
            pressure_active: self.pressure_active,
            scheduler: self.policy_report.clone(),
            roles: Role::ALL
                .into_iter()
                .filter_map(|r| Some((r.to_string(), roles.get(r)?.to_owned(), self.role_enabled(r))))
                .collect(),
            agents: self.sessions.len(),
            boosted: processes.into_iter().filter(|p| p.role.is_some()).collect(),
        }
    }

    /// Loads persisted pause/role state; call before `startup`.
    pub fn load_runtime(&mut self) {
        let Some(path) = runtime_path() else { return };
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        match serde_json::from_str::<Runtime>(&text) {
            Ok(rt) => {
                self.paused = rt.paused;
                self.roles_off = rt.roles_off.iter().filter_map(|r| r.parse().ok()).collect();
                if self.paused {
                    tracing::info!("starting paused (scdrctl resume to continue)");
                }
            }
            Err(e) => tracing::warn!("{}: {e}", path.display()),
        }
    }

    fn save_runtime(&self) {
        let Some(path) = runtime_path() else { return };
        let rt = Runtime {
            paused: self.paused,
            roles_off: self.roles_off.iter().map(|r| r.to_string()).collect(),
        };
        if let Err(e) = std::fs::write(&path, serde_json::to_string_pretty(&rt).unwrap_or_default()) {
            tracing::warn!("can't save {}: {e}", path.display());
        }
    }
}

/// `app-com.anthropic.Claude-3405.scope` -> `com.anthropic.Claude`;
/// `app-org.telegram.desktop@autostart.service` -> `org.telegram.desktop`.
fn app_of(pid: u32) -> Option<String> {
    let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    let path = cgroup.lines().find_map(|l| l.strip_prefix("0::"))?;
    if !path.contains("/app.slice/") {
        return None;
    }
    let unit = crate::units::UnitId::from_cgroup(path)?.unit;
    app_name(&unit)
}

fn app_name(unit: &str) -> Option<String> {
    let base = unit.strip_prefix("app-")?;
    let base = base.strip_suffix(".scope").or_else(|| base.strip_suffix(".service"))?;
    let base = base.split('@').next()?;
    let base = base.strip_prefix("flatpak-").unwrap_or(base);
    // Transient scopes end in "-<random or pid>".
    let name = match base.rsplit_once('-') {
        Some((head, tail)) if tail.chars().all(|c| c.is_ascii_hexdigit()) => head,
        _ => base,
    };
    Some(name.replace("\\x2d", "-"))
}

fn runtime_path() -> Option<PathBuf> {
    let dir = std::env::var_os("STATE_DIRECTORY")?;
    // systemd may pass several colon-separated directories; use the first.
    let first = dir.to_string_lossy().split(':').next()?.to_owned();
    Some(PathBuf::from(first).join("runtime.json"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn app_names_from_units() {
        let n = |u| super::app_name(u);
        assert_eq!(n("app-com.anthropic.Claude-3405.scope").as_deref(), Some("com.anthropic.Claude"));
        assert_eq!(n("app-org.telegram.desktop@autostart.service").as_deref(), Some("org.telegram.desktop"));
        assert_eq!(n("app-flatpak-org.mozilla.firefox-12345.scope").as_deref(), Some("org.mozilla.firefox"));
        assert_eq!(n("app-pamac\\x2dtray\\x2dplasma@autostart.service").as_deref(), Some("pamac-tray-plasma"));
        assert_eq!(n("plasma-kwin_wayland.service"), None);
    }
}
