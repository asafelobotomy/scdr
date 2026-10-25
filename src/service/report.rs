//! What the daemon reports: `scdrctl explain` and `scdrctl status`.

use super::{Service, State};
use crate::explain::{self, Explain, Status};
use crate::system;
use std::collections::BTreeMap;

impl Service {
    pub fn explain(&self, pid: u32) -> Explain {
        let mut ex = explain::offline(&self.config, pid);
        let Some(entry) = self.procs.get(&pid) else {
            if ex.name.is_some() {
                ex.state = "not tracked yet".into();
            }
            return ex;
        };
        match &entry.state {
            State::Pending => ex.state = "pending".into(),
            State::KernelThread => ex.state = "kernel thread".into(),
            State::Excepted(why) => {
                ex.state = "excepted".into();
                ex.reason = Some(why.clone());
                ex.rule = None;
                ex.profile = None;
                ex.target = None;
            }
            State::Unmatched => ex.state = "unmatched".into(),
            State::Managed(m) => {
                ex.state = if self.dry_run { "managed (dry run)" } else { "managed" }.into();
                // What the daemon applied, which may be a role the offline view can't see.
                ex.rule = Some(m.rule.clone());
                if let Some(p) = self.config.profiles.get(&m.profile) {
                    ex.profile = Some(p.name.clone());
                    ex.profile_chain = p.chain.clone();
                    ex.settings = Some(explain::format_profile(&p.profile));
                    ex.notes = explain::notes(&p.profile);
                }
                ex.original = Some(m.original.to_string());
                ex.target = Some(m.target.to_string());
                ex.threads = Some(m.last.to_string());
                ex.errors = m.last.errors.clone();
                if m.last.foreign > 0 {
                    ex.notes.push(format!(
                        "{} threads have values set by another tool (rtkit, GameMode, renice…) and were left alone",
                        m.last.foreign
                    ));
                }
                if m.last.protected > 0 {
                    ex.notes.push(format!(
                        "{} realtime or sched_ext threads were left alone",
                        m.last.protected
                    ));
                }
            }
        }
        if let Some(cg) = ex.cgroup.clone() {
            if let Some((id, st)) = self.unit_for(&cg) {
                let holder = if st.owner == pid {
                    "held by this process".to_owned()
                } else {
                    format!("held by pid {}", st.owner)
                };
                ex.unit = Some(format!(
                    "{id}: {} (was {}), {holder} for {}",
                    st.applied, st.original, st.profile
                ));
                let kept: Vec<String> = [("CPUWeight", st.kept.cpu), ("IOWeight", st.kept.io)]
                    .into_iter()
                    .filter_map(|(k, v)| Some(format!("{k}={}", v?)))
                    .collect();
                if !kept.is_empty() {
                    ex.notes.push(format!(
                        "the unit sets its own {}; scdr left it alone",
                        kept.join(" ")
                    ));
                }
            }
        }
        ex
    }

    pub fn status(&self) -> Status {
        let mut by_profile: BTreeMap<String, usize> = BTreeMap::new();
        let (mut managed, mut excepted, mut unmatched, mut errors) = (0, 0, 0, 0);
        for e in self.procs.values() {
            match &e.state {
                State::Managed(m) => {
                    managed += 1;
                    *by_profile.entry(m.profile.clone()).or_default() += 1;
                    if !m.last.errors.is_empty() {
                        errors += 1;
                    }
                }
                State::Excepted(_) => excepted += 1,
                State::Unmatched => unmatched += 1,
                State::Pending | State::KernelThread => {}
            }
        }
        Status {
            version: env!("CARGO_PKG_VERSION").into(),
            dry_run: self.dry_run,
            watcher: self.watcher.into(),
            uptime_secs: self.started.elapsed().as_secs(),
            config_files: self.config.files.iter().map(|p| p.display().to_string()).collect(),
            profiles: self.config.profiles.len(),
            rules: self.config.rules.len(),
            tracked: self.procs.len(),
            managed,
            excepted,
            unmatched,
            with_errors: errors,
            by_profile,
            autogroup: system::autogroup(),
            sched_ext: system::sched_ext(),
            units: self.unit_count(),
            uclamp_active: self.uclamp_active,
            governors: system::cpu_governors(),
            disks: system::disks(),
            power_profile: self.power_profile().map(str::to_owned),
            roles: self.config.roles.for_power(self.power_profile()),
            sessions: self.sessions_report(),
            cpu_pressure: self.cpu_pressure().0,
            pressure_active: self.cpu_pressure().1,
            policy: self.policy_report.clone(),
        }
    }
}
