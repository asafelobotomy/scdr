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
    /// Memory and io pressure (PSI "some" avg10), for the widget's graphs.
    pub memory_pressure: Option<f64>,
    pub io_pressure: Option<f64>,
    /// scx_devbox samples (one a second) new since the last tick; the
    /// widget keeps the history. Empty unless devbox runs.
    pub devbox: Vec<super::devbox::DevboxSample>,
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
        self.process_reports(false)
    }

    /// Reports for managed processes; with `roles_only`, just those a role
    /// boosted. Each report reads /proc, so the widget's 2 s State refresh
    /// asks only for the few it shows, not every managed process.
    fn process_reports(&self, roles_only: bool) -> Vec<ProcessReport> {
        let mut out: Vec<ProcessReport> = self
            .procs
            .iter()
            .filter_map(|(pid, e)| match &e.state {
                State::Managed(m) if !roles_only || m.rule.source == "role" => {
                    let role = (m.rule.source == "role")
                        .then(|| m.rule.conditions.split(':').next().unwrap_or("").to_owned());
                    let why = if role.is_some() {
                        m.rule.conditions.clone()
                    } else {
                        format!("{} #{}: {}", m.rule.source, m.rule.index, m.rule.conditions)
                    };
                    Some(ProcessReport {
                        pid: *pid,
                        name: m.name.clone(),
                        profile: m.profile.clone(),
                        role,
                        why,
                        app: m.app.clone(),
                    })
                }
                _ => None,
            })
            .collect();
        out.sort_by_key(|p| p.pid);
        out
    }

    pub fn widget_state(&self) -> WidgetState {
        let managed = self.procs.values().filter(|e| matches!(e.state, State::Managed(_))).count();
        let roles = self.config.roles.for_power(self.power_profile.as_deref());
        WidgetState {
            paused: self.paused,
            managed,
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
            boosted: self.process_reports(true),
            memory_pressure: crate::system::psi_some_avg10("memory").map(f64::round),
            io_pressure: crate::system::psi_some_avg10("io").map(f64::round),
            devbox: self.devbox_samples.clone(),
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
        // Write then rename, so a crash mid-write can't leave it empty.
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(&rt).unwrap_or_default();
        if let Err(e) = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &path)) {
            tracing::warn!("can't save {}: {e}", path.display());
        }
    }
}

/// `app-com.anthropic.Claude-3405.scope` -> `com.anthropic.Claude`;
/// `app-org.telegram.desktop@autostart.service` -> `org.telegram.desktop`.
/// The desktop app a cgroup belongs to, if it is in an `app.slice` unit.
pub(super) fn app_of_cgroup(cgroup: &str) -> Option<String> {
    if !cgroup.contains("/app.slice/") {
        return None;
    }
    app_name(crate::units::unit_name(cgroup)?)
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
