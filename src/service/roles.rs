//! Live context from each user's session agent: the focused app, processes
//! playing audio, and GameMode games. Each session is keyed by the agent's
//! bus name, so two agents of one user (two seats, a second login) don't
//! overwrite each other; roles for a process combine every session of its
//! uid. scdrd only accepts pids that belong to the agent's uid.

use super::{RuleRef, Service, State, Trigger};
use crate::config::{NamedProfile, Role};
use crate::procfs::ProcInfo;
use crate::units::UnitId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Default)]
pub(super) struct Session {
    /// The uid the agent runs as.
    pub uid: u32,
    /// cgroup path of the focused app's unit, and the window's pid.
    pub foreground: Option<(String, u32)>,
    /// pid -> start time, so a reused pid doesn't inherit the role.
    pub audio: BTreeMap<u32, u64>,
    pub games: BTreeMap<u32, u64>,
}

/// One session's live state, for `status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionReport {
    pub uid: u32,
    pub foreground: Option<String>,
    pub audio: Vec<u32>,
    pub games: Vec<u32>,
}

/// Sessions by their agent's unique bus name.
pub(super) type Sessions = HashMap<String, Session>;

impl Service {
    /// The focused window of `uid`'s session is `pid`. Only app units
    /// (`app.slice`) are boosted, not the desktop's own services. `pid` 0:
    /// no app is focused (the screen is locked).
    pub fn set_foreground(&mut self, uid: u32, sender: &str, pid: u32) -> Result<(), String> {
        let new = if pid == 0 {
            None
        } else {
            let info = owned_process(uid, pid)?;
            let in_app_slice = info.cgroup.contains(&format!("/user@{uid}.service/app.slice/"));
            (in_app_slice && UnitId::from_cgroup(&info.cgroup).is_some())
                .then(|| (info.cgroup.clone(), pid))
        };

        // Without a focused app the background role can't be placed; if
        // pressure came first (e.g. right after a restart), re-check every app.
        let first = !self.sessions.values().any(|s| s.uid == uid && s.foreground.is_some());
        let session = self.session(uid, sender);
        let old = std::mem::replace(&mut session.foreground, new.clone());
        // With devbox handling focus, telling it is all there is to do.
        if self.devbox_hints {
            self.write_hints();
            return Ok(());
        }
        let mut affected = Vec::new();
        // (Only once a focused app is known: focus on a desktop window
        // places nothing, so re-checking every app then is wasted work.)
        if first && new.is_some() && self.pressure_active {
            affected.extend(cgroup_pids(app_slice(uid).trim_end_matches('/')));
        } else if old.as_ref().map(|o| &o.0) == new.as_ref().map(|n| &n.0) {
            return Ok(()); // same app, another window
        }
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
        self.reevaluate(changed(&old, &pids));
        Ok(())
    }

    pub fn set_games(&mut self, uid: u32, sender: &str, pids: Vec<u32>) -> Result<(), String> {
        let pids = owned_pids(uid, pids);
        let session = self.session(uid, sender);
        let old = std::mem::replace(&mut session.games, pids.clone());
        self.reevaluate(changed(&old, &pids));
        Ok(())
    }

    /// The bus names of connected session agents.
    pub fn agent_senders(&self) -> Vec<String> {
        self.sessions.keys().cloned().collect()
    }

    /// An agent disconnected: forget its session and re-evaluate what it boosted.
    pub fn drop_agent(&mut self, sender: &str) {
        let Some(s) = self.sessions.remove(sender) else {
            return;
        };
        tracing::info!("session agent {sender} (uid {}) went away", s.uid);
        self.write_hints();
        let mut affected: Vec<u32> = s.audio.into_keys().chain(s.games.into_keys()).collect();
        if let Some((cgroup, _)) = s.foreground {
            affected.extend(cgroup_pids(&cgroup));
        }
        self.reevaluate(affected);
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
        let sessions: Vec<&Session> = self.sessions.values().filter(|s| s.uid == info.uid).collect();
        if sessions.is_empty() {
            return None;
        }
        let roles = self.config.roles.for_power(self.power_profile.as_deref());
        let focused = sessions.iter().any(|s| {
            s.foreground.as_ref().is_some_and(|(cg, _)| {
                info.cgroup == *cg || info.cgroup.starts_with(&format!("{cg}/"))
            })
        });
        let active = |role: Role| -> bool {
            match role {
                Role::Game => sessions.iter().any(|s| s.games.get(&info.pid) == Some(&info.start_time)),
                Role::Audio => sessions.iter().any(|s| s.audio.get(&info.pid) == Some(&info.start_time)),
                // Left to scx_devbox while it takes hints (see `devbox`).
                Role::Foreground => focused && !self.devbox_hints,
                // Never the focused app, even with the foreground role off.
                Role::Background => {
                    !self.devbox_hints
                        && self.pressure_active
                        && !focused
                        && info.cgroup.starts_with(&app_slice(info.uid))
                }
            }
        };
        let role = Role::ALL
            .into_iter()
            .find(|r| self.role_enabled(*r) && roles.get(*r).is_some() && active(*r))?;
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
            .values()
            .map(|s| SessionReport {
                uid: s.uid,
                foreground: s.foreground.as_ref().map(|(cg, pid)| {
                    let unit = UnitId::from_cgroup(cg).map(|u| u.unit).unwrap_or_else(|| cg.clone());
                    format!("{unit} (window pid {pid})")
                }),
                audio: s.audio.keys().copied().collect(),
                games: s.games.keys().copied().collect(),
            })
            .collect();
        out.sort_by_key(|s| s.uid);
        out
    }

    pub(super) fn session(&mut self, uid: u32, sender: &str) -> &mut Session {
        self.sessions
            .entry(sender.to_owned())
            .or_insert_with(|| Session { uid, ..Default::default() })
    }

    pub(super) fn role_pids(&self) -> Vec<u32> {
        let mut pids = Vec::new();
        for s in self.sessions.values() {
            pids.extend(s.audio.keys().chain(s.games.keys()));
            if let Some((cg, _)) = &s.foreground {
                pids.extend(cgroup_pids(cg));
            }
        }
        pids
    }

    pub(super) fn reevaluate(&mut self, mut pids: Vec<u32>) {
        pids.sort_unstable();
        pids.dedup();
        // Per-process lines are debug-level for role changes; this summarises them.
        let mut moved: BTreeMap<String, usize> = BTreeMap::new();
        for pid in pids {
            let before = self.profile_of(pid);
            self.evaluate(pid, Trigger::Role);
            let after = self.profile_of(pid);
            if before != after {
                *moved.entry(after.unwrap_or_else(|| "restored".into())).or_default() += 1;
            }
        }
        if !moved.is_empty() {
            let summary: Vec<String> = moved.iter().map(|(p, n)| format!("{n} → {p}")).collect();
            tracing::info!("roles: {}", summary.join(", "));
        }
    }

    fn profile_of(&self, pid: u32) -> Option<String> {
        match &self.procs.get(&pid)?.state {
            State::Managed(m) => Some(m.profile.clone()),
            _ => None,
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

/// The caller's own pids (others, and ones that don't exist, are dropped),
/// with each process's start time.
fn owned_pids(uid: u32, pids: Vec<u32>) -> BTreeMap<u32, u64> {
    pids.into_iter()
        .filter_map(|pid| owned_process(uid, pid).ok().map(|i| (pid, i.start_time)))
        .collect()
}

/// Pids whose role starts or ends between two reports.
fn changed(old: &BTreeMap<u32, u64>, new: &BTreeMap<u32, u64>) -> Vec<u32> {
    let mut pids: Vec<u32> = old
        .iter()
        .filter(|(p, t)| new.get(p) != Some(t))
        .chain(new.iter().filter(|(p, t)| old.get(p) != Some(t)))
        .map(|(p, _)| *p)
        .collect();
    pids.dedup();
    pids
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

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::service::Service;

    #[test]
    fn agents_of_one_user_keep_their_own_sessions() {
        let mut svc = Service::new(Config::from_sources(Vec::new()).unwrap(), true);
        svc.session(1000, ":1.10").games.insert(4242, 7);
        svc.session(1000, ":1.20").audio.insert(4343, 8);
        assert_eq!(svc.sessions.len(), 2);
        // The second agent leaving doesn't take the first one's state with it.
        svc.drop_agent(":1.20");
        assert_eq!(svc.agent_senders(), vec![":1.10".to_owned()]);
        assert!(svc.sessions[":1.10"].games.contains_key(&4242));
        svc.drop_agent(":1.99"); // unknown names are ignored
        assert_eq!(svc.sessions.len(), 1);
    }

    #[test]
    fn a_reused_pid_counts_as_a_change() {
        let old = [(10, 1), (11, 1)].into_iter().collect();
        let new = [(10, 1), (11, 2), (12, 1)].into_iter().collect();
        let mut got = super::changed(&old, &new);
        got.sort_unstable();
        got.dedup();
        assert_eq!(got, vec![11, 12]);
    }
}
