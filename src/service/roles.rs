//! Live context from each user's session agent: the focused app, processes
//! playing audio, and GameMode games. Each session is keyed by the agent's
//! uid; scdrd only accepts pids that belong to that uid.

use super::{RuleRef, Service, Trigger};
use crate::config::{NamedProfile, Role};
use crate::procfs::ProcInfo;
use crate::units::UnitId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone, Default)]
pub(super) struct Session {
    /// The agent's unique bus name, to notice when it goes away.
    pub sender: String,
    /// cgroup path of the focused app's unit, and the window's pid.
    pub foreground: Option<(String, u32)>,
    pub audio: BTreeSet<u32>,
    pub games: BTreeSet<u32>,
}

/// One session's live state, for `status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionReport {
    pub uid: u32,
    pub foreground: Option<String>,
    pub audio: Vec<u32>,
    pub games: Vec<u32>,
}

pub(super) type Sessions = HashMap<u32, Session>;

impl Service {
    /// The focused window of `uid`'s session is `pid`. Only app units
    /// (`app.slice`) are boosted, not the desktop's own services.
    pub fn set_foreground(&mut self, uid: u32, sender: &str, pid: u32) -> Result<(), String> {
        let info = owned_process(uid, pid)?;
        let in_app_slice = info.cgroup.contains(&format!("/user@{uid}.service/app.slice/"));
        let new = (in_app_slice && UnitId::from_cgroup(&info.cgroup).is_some())
            .then(|| (info.cgroup.clone(), pid));

        let session = self.session(uid, sender);
        let old = std::mem::replace(&mut session.foreground, new.clone());
        if old.as_ref().map(|o| &o.0) == new.as_ref().map(|n| &n.0) {
            return Ok(()); // same app, another window
        }
        let mut affected = Vec::new();
        for (cgroup, _) in old.iter().chain(new.iter()) {
            affected.extend(cgroup_pids(cgroup));
        }
        self.reevaluate(affected);
        Ok(())
    }

    pub fn set_audio(&mut self, uid: u32, sender: &str, pids: Vec<u32>) -> Result<(), String> {
        let pids = owned_pids(uid, pids);
        let session = self.session(uid, sender);
        let old = std::mem::replace(&mut session.audio, pids.clone());
        self.reevaluate(old.symmetric_difference(&pids).copied().collect());
        Ok(())
    }

    pub fn set_games(&mut self, uid: u32, sender: &str, pids: Vec<u32>) -> Result<(), String> {
        let pids = owned_pids(uid, pids);
        let session = self.session(uid, sender);
        let old = std::mem::replace(&mut session.games, pids.clone());
        self.reevaluate(old.symmetric_difference(&pids).copied().collect());
        Ok(())
    }

    /// The bus names of connected session agents.
    pub fn agent_senders(&self) -> Vec<String> {
        self.sessions.values().map(|s| s.sender.clone()).collect()
    }

    /// An agent disconnected: forget its session and re-evaluate what it boosted.
    pub fn drop_agent(&mut self, sender: &str) {
        let gone: Vec<u32> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.sender == sender)
            .map(|(uid, _)| *uid)
            .collect();
        for uid in gone {
            let s = self.sessions.remove(&uid).unwrap();
            tracing::info!("session agent for uid {uid} went away");
            let mut affected: Vec<u32> = s.audio.into_iter().chain(s.games).collect();
            if let Some((cgroup, _)) = s.foreground {
                affected.extend(cgroup_pids(&cgroup));
            }
            self.reevaluate(affected);
        }
    }

    /// power-profiles-daemon switched profiles; roles may differ.
    pub fn set_power_profile(&mut self, profile: Option<String>) {
        if self.power_profile == profile {
            return;
        }
        let before = self.config.roles.for_power(self.power_profile.as_deref());
        tracing::info!("power profile: {}", profile.as_deref().unwrap_or("unknown"));
        self.power_profile = profile;
        if self.config.roles.for_power(self.power_profile.as_deref()) != before {
            let affected = self.role_pids();
            self.reevaluate(affected);
        }
    }

    /// Whether any session has a GameMode game running.
    pub fn gaming(&self) -> bool {
        self.sessions.values().any(|s| !s.games.is_empty())
    }

    pub fn power_profile(&self) -> Option<&str> {
        self.power_profile.as_deref()
    }

    /// The role profile for a process no rule matched, if any.
    pub(super) fn role_match(&self, info: &ProcInfo) -> Option<(RuleRef, NamedProfile)> {
        let session = self.sessions.get(&info.uid)?;
        let roles = self.config.roles.for_power(self.power_profile.as_deref());
        let focused = session.foreground.as_ref().is_some_and(|(cg, _)| {
            info.cgroup == *cg || info.cgroup.starts_with(&format!("{cg}/"))
        });
        let active = |role: Role| -> bool {
            match role {
                Role::Game => session.games.contains(&info.pid),
                Role::Audio => session.audio.contains(&info.pid),
                Role::Foreground => focused,
                // Never the focused app, even with the foreground role off.
                Role::Background => {
                    self.pressure_active
                        && !focused
                        && info.cgroup.starts_with(&app_slice(info.uid))
                }
            }
        };
        let role = Role::ALL.into_iter().find(|r| roles.get(*r).is_some() && active(*r))?;
        let profile = self.config.profiles.get(roles.get(role)?)?.clone();
        let rule = RuleRef {
            source: "role".into(),
            index: 0,
            conditions: format!("{role}: {}", role.describe()),
        };
        Some((rule, profile))
    }

    pub fn sessions_report(&self) -> Vec<SessionReport> {
        let mut out: Vec<SessionReport> = self
            .sessions
            .iter()
            .map(|(uid, s)| SessionReport {
                uid: *uid,
                foreground: s.foreground.as_ref().map(|(cg, pid)| {
                    let unit = UnitId::from_cgroup(cg).map(|u| u.unit).unwrap_or_else(|| cg.clone());
                    format!("{unit} (window pid {pid})")
                }),
                audio: s.audio.iter().copied().collect(),
                games: s.games.iter().copied().collect(),
            })
            .collect();
        out.sort_by_key(|s| s.uid);
        out
    }

    fn session(&mut self, uid: u32, sender: &str) -> &mut Session {
        let s = self.sessions.entry(uid).or_default();
        s.sender = sender.to_owned();
        s
    }

    fn role_pids(&self) -> Vec<u32> {
        let mut pids = Vec::new();
        for s in self.sessions.values() {
            pids.extend(s.audio.iter().chain(&s.games));
            if let Some((cg, _)) = &s.foreground {
                pids.extend(cgroup_pids(cg));
            }
        }
        pids
    }

    pub(super) fn reevaluate(&mut self, mut pids: Vec<u32>) {
        pids.sort_unstable();
        pids.dedup();
        for pid in pids {
            self.evaluate(pid, Trigger::Role);
        }
    }
}

/// A user's `app.slice`, where desktop apps run.
pub(super) fn app_slice(uid: u32) -> String {
    format!("/user.slice/user-{uid}.slice/user@{uid}.service/app.slice/")
}

fn owned_process(uid: u32, pid: u32) -> Result<ProcInfo, String> {
    let info = ProcInfo::read(pid).map_err(|_| format!("no process {pid}"))?;
    if uid != 0 && info.uid != uid {
        return Err(format!("process {pid} does not belong to uid {uid}"));
    }
    Ok(info)
}

/// Drops pids the caller doesn't own (or that don't exist).
fn owned_pids(uid: u32, pids: Vec<u32>) -> BTreeSet<u32> {
    pids.into_iter()
        .filter(|&pid| owned_process(uid, pid).is_ok())
        .collect()
}

/// Every process in a cgroup and its children.
pub(super) fn cgroup_pids(cgroup: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let root = format!("/sys/fs/cgroup{cgroup}");
    let mut stack = vec![std::path::PathBuf::from(root)];
    while let Some(dir) = stack.pop() {
        if let Ok(s) = std::fs::read_to_string(dir.join("cgroup.procs")) {
            out.extend(s.lines().filter_map(|l| l.parse::<u32>().ok()));
        }
        if let Ok(entries) = std::fs::read_dir(&dir) {
            stack.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
        }
    }
    out
}
