//! `explain` and `status` reports, shared by the daemon (over D-Bus, as JSON)
//! and by `scdrctl` (offline, without a daemon).

use crate::apply::Snapshot;
use crate::config::{Config, Profile};
use crate::procfs::ProcInfo;
use crate::service::RuleRef;
use crate::system::{self, SchedExt};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::{self, Write as _};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Explain {
    pub pid: u32,
    pub name: Option<String>,
    pub exe: Option<String>,
    pub cmdline: Option<String>,
    pub cgroup: Option<String>,
    pub uid: Option<u32>,
    pub state: String,
    pub reason: Option<String>,
    pub rule: Option<RuleRef>,
    pub profile: Option<String>,
    pub profile_chain: Vec<String>,
    pub settings: Option<String>,
    pub current: Option<String>,
    pub original: Option<String>,
    pub target: Option<String>,
    pub threads: Option<String>,
    pub errors: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub version: String,
    pub dry_run: bool,
    pub watcher: String,
    pub uptime_secs: u64,
    pub config_files: Vec<String>,
    pub profiles: usize,
    pub rules: usize,
    pub tracked: usize,
    pub managed: usize,
    pub excepted: usize,
    pub unmatched: usize,
    pub with_errors: usize,
    pub by_profile: BTreeMap<String, usize>,
    pub autogroup: Option<bool>,
    pub sched_ext: Option<SchedExt>,
    pub io_schedulers: Vec<(String, String)>,
}

/// What the config says about `pid`, computed without daemon state.
pub fn offline(config: &Config, pid: u32) -> Explain {
    let mut ex = Explain {
        pid,
        state: "no such process".into(),
        ..Default::default()
    };
    let Ok(info) = ProcInfo::read(pid) else {
        return ex;
    };
    ex.name = Some(info.comm.clone());
    ex.exe = info.exe.clone();
    ex.cmdline = Some(info.cmdline_string());
    ex.cgroup = Some(info.cgroup.clone());
    ex.uid = Some(info.uid);
    let current = Snapshot::read(pid).ok();
    ex.current = current.map(|s| s.to_string());

    if info.kthread {
        ex.state = "kernel thread".into();
        return ex;
    }
    if let Some(n) = config.exceptions.exception(&info) {
        ex.state = "excepted".into();
        ex.reason = Some(format!("{n:?} is in [exceptions]"));
        return ex;
    }
    if let Some(l) = config.exceptions.launcher(&info) {
        ex.state = "excepted".into();
        ex.reason = Some(format!("{l:?} is a launcher"));
        return ex;
    }

    let Some(rule) = config.rules.find(&info) else {
        ex.state = "unmatched".into();
        return ex;
    };
    let profile = &config.profiles[&rule.profile];
    ex.state = "matches".into();
    if current.is_some_and(|c| c.task.cpu.is_protected()) && profile.profile.sched.is_none() {
        ex.state = "excepted".into();
        ex.reason = Some("main thread is realtime or sched_ext".into());
    }
    ex.rule = Some(RuleRef {
        source: rule.source.display().to_string(),
        index: rule.index,
        conditions: rule.conditions.clone(),
    });
    ex.profile = Some(profile.name.clone());
    ex.profile_chain = profile.chain.clone();
    ex.settings = Some(format_profile(&profile.profile));
    if ex.reason.is_none() {
        ex.target = current.map(|c| c.with_profile(&profile.profile).to_string());
    }
    ex.notes = notes(&profile.profile);
    ex
}

/// Caveats about whether a profile's settings can take effect on this system.
pub fn notes(p: &Profile) -> Vec<String> {
    let mut notes = Vec::new();
    if p.io.is_some() {
        let ignored = system::ioprio_ignored_on();
        if !ignored.is_empty() {
            notes.push(format!(
                "io priority only takes effect under bfq; ignored on {}",
                ignored.join(", ")
            ));
        }
    }
    if let Some(scx) = system::sched_ext().filter(SchedExt::is_active) {
        let name = scx.ops.as_deref().unwrap_or("a sched_ext scheduler");
        if p.slice_us.is_some() || p.sched.is_some() || p.nice.is_some() {
            notes.push(format!(
                "{name} is active: slice_us is ignored and nice/policy handling depends on it"
            ));
        }
    }
    if p.nice.is_some() && system::autogroup() == Some(true) {
        notes.push("autogroup is on: nice only orders tasks within the same session".into());
    }
    notes
}

pub fn format_profile(p: &Profile) -> String {
    let mut s = String::new();
    let mut put = |k: &str, v: String| {
        let _ = write!(s, "{}{k}={v}", if s.is_empty() { "" } else { " " });
    };
    if let Some(v) = p.sched {
        put("sched", v.to_string());
    }
    if let Some(v) = p.nice {
        put("nice", v.to_string());
    }
    if let Some(v) = p.slice_us {
        put("slice", format!("{v}us"));
    }
    if let Some(v) = p.io {
        put("io", v.to_string());
    }
    if let Some(v) = p.oom_score_adj {
        put("oom_score_adj", v.to_string());
    }
    if s.is_empty() {
        s.push_str("(no settings)");
    }
    s
}

impl fmt::Display for Explain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let row = |f: &mut fmt::Formatter<'_>, k: &str, v: &str| writeln!(f, "  {k:<9}{v}");
        write!(f, "pid {}", self.pid)?;
        if let Some(n) = &self.name {
            write!(f, "  {n}")?;
        }
        if let Some(e) = &self.exe {
            write!(f, "  ({e})")?;
        }
        writeln!(f)?;
        if let Some(c) = self.cmdline.as_deref().filter(|c| !c.is_empty()) {
            row(f, "cmdline", c)?;
        }
        if let Some(c) = self.cgroup.as_deref().filter(|c| !c.is_empty()) {
            row(f, "cgroup", c)?;
        }
        match &self.reason {
            Some(r) => row(f, "state", &format!("{} — {r}", self.state))?,
            None => row(f, "state", &self.state)?,
        }
        if let Some(r) = &self.rule {
            row(f, "rule", &format!("{} #{}: {}", r.source, r.index, r.conditions))?;
        }
        if let Some(p) = &self.profile {
            let chain = if self.profile_chain.len() > 1 {
                format!(" (inherits {})", self.profile_chain[..self.profile_chain.len() - 1].join(" → "))
            } else {
                String::new()
            };
            row(f, "profile", &format!("{p}{chain}: {}", self.settings.as_deref().unwrap_or("")))?;
        }
        for (k, v) in [
            ("original", &self.original),
            ("target", &self.target),
            ("current", &self.current),
            ("threads", &self.threads),
        ] {
            if let Some(v) = v {
                row(f, k, v)?;
            }
        }
        for e in &self.errors {
            row(f, "error", e)?;
        }
        for n in &self.notes {
            row(f, "note", n)?;
        }
        Ok(())
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mode = if self.dry_run { " (dry run)" } else { "" };
        writeln!(f, "scdrd {}{mode}, up {}s, events via {}", self.version, self.uptime_secs, self.watcher)?;
        writeln!(f, "config    {} profiles, {} rules from:", self.profiles, self.rules)?;
        for file in &self.config_files {
            writeln!(f, "            {file}")?;
        }
        writeln!(
            f,
            "processes {} tracked: {} managed, {} excepted, {} unmatched",
            self.tracked, self.managed, self.excepted, self.unmatched
        )?;
        if self.with_errors > 0 {
            writeln!(f, "          {} managed processes had errors (see explain)", self.with_errors)?;
        }
        for (profile, n) in &self.by_profile {
            writeln!(f, "            {n:>5}  {profile}")?;
        }
        let autogroup = match self.autogroup {
            Some(true) => "on",
            Some(false) => "off",
            None => "unknown",
        };
        writeln!(f, "autogroup {autogroup}")?;
        match &self.sched_ext {
            Some(s) if s.is_active() => {
                writeln!(f, "sched_ext {} ({})", s.state, s.ops.as_deref().unwrap_or("?"))?
            }
            Some(s) => writeln!(f, "sched_ext {} (EEVDF in use)", s.state)?,
            None => writeln!(f, "sched_ext not supported by this kernel")?,
        }
        let io: Vec<String> = self.io_schedulers.iter().map(|(d, s)| format!("{d}={s}")).collect();
        writeln!(f, "io sched  {}", io.join(" "))
    }
}
