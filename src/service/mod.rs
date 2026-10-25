//! Daemon state: which processes exist, what applies to them, and why.

mod control;
mod knobs;
mod lineage;
mod pressure;
mod report;
mod roles;

pub use control::{ProcessReport, WidgetState};
pub use pressure::cpu_some_avg10;
pub use roles::SessionReport;
mod units;

use lineage::hand_tuned;

use crate::apply::{self, Outcome, Snapshot};
use crate::config::{Config, NamedProfile};
use crate::procfs::{self, ProcInfo};
use crate::rules::Rule;
use crate::system;
use crate::watcher::Event;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

/// Why a process is being evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Startup or periodic /proc scan; re-applies to catch new threads.
    Scan,
    /// Just exec'd: also check whether a launcher hand-tuned it.
    Exec,
    /// Delayed re-check after exec, once the cgroup has settled.
    Recheck,
    /// comm changed (prctl PR_SET_NAME).
    Comm,
    /// A session role (foreground, audio, game) or power profile changed.
    Role,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleRef {
    pub source: String,
    pub index: usize,
    pub conditions: String,
}

impl RuleRef {
    fn from_rule(r: &Rule) -> Self {
        RuleRef {
            source: r.source.display().to_string(),
            index: r.index,
            conditions: r.conditions.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Managed {
    pub rule: RuleRef,
    pub profile: String,
    pub original: Snapshot,
    pub target: Snapshot,
    pub last: Outcome,
}

#[derive(Debug, Clone)]
pub enum State {
    /// Seen via fork, not yet evaluated.
    Pending,
    KernelThread,
    Excepted(String),
    Unmatched,
    Managed(Box<Managed>),
}

#[derive(Debug, Clone)]
pub struct Entry {
    /// 0 until the process has been read from /proc.
    pub start_time: u64,
    pub ppid: u32,
    /// Set when a launcher (or a hand-tuned exec) started this process tree;
    /// inherited by children and kept across exec.
    pub sticky: Option<String>,
    pub state: State,
}

impl Entry {
    fn new(ppid: u32, sticky: Option<String>) -> Self {
        Entry {
            start_time: 0,
            ppid,
            sticky,
            state: State::Pending,
        }
    }
}

pub struct Service {
    pub config: Config,
    pub dry_run: bool,
    pub watcher: &'static str,
    procs: HashMap<u32, Entry>,
    units: BTreeMap<crate::units::UnitId, units::UnitState>,
    autogroup_original: Option<bool>,
    /// Disks (MAJ:MIN) on which scdr enabled iocost.
    iocost_enabled: Vec<String>,
    /// Whether uclamp settings can take effect (schedutil in use).
    uclamp_active: bool,
    sessions: roles::Sessions,
    power_profile: Option<String>,
    cpu_pressure: Option<f64>,
    pressure_active: bool,
    paused: bool,
    roles_off: std::collections::BTreeSet<crate::config::Role>,
    /// Scheduler policy status lines, from the main loop.
    pub policy_report: Vec<String>,
    started: Instant,
    self_pid: u32,
}

impl Service {
    pub fn new(config: Config, dry_run: bool) -> Self {
        Service {
            config,
            dry_run,
            watcher: "polling",
            procs: HashMap::new(),
            units: BTreeMap::new(),
            autogroup_original: None,
            iocost_enabled: Vec::new(),
            uclamp_active: false,
            sessions: Default::default(),
            power_profile: None,
            cpu_pressure: None,
            pressure_active: false,
            paused: false,
            roles_off: Default::default(),
            policy_report: Vec::new(),
            started: Instant::now(),
            self_pid: std::process::id(),
        }
    }

    pub fn startup(&mut self) {
        self.apply_system();
        self.rescan();
    }

    /// Handles one watcher event. Returns a process to re-check later.
    pub fn handle(&mut self, ev: Event) -> Option<(u32, u64)> {
        match ev {
            Event::Fork {
                parent_tgid,
                child_pid,
                child_tgid,
            } => {
                // New threads inherit their creator's settings; only track processes.
                if child_pid == child_tgid {
                    let sticky = self
                        .procs
                        .get(&parent_tgid)
                        .and_then(|p| p.sticky.clone());
                    self.procs.insert(child_pid, Entry::new(parent_tgid, sticky));
                }
                None
            }
            Event::Exec { pid, tgid } if pid == tgid => {
                self.evaluate(pid, Trigger::Exec);
                let start = self.procs.get(&pid)?.start_time;
                (self.config.system.exec_recheck_ms > 0).then_some((pid, start))
            }
            Event::Comm { pid, tgid } if pid == tgid => {
                self.evaluate(pid, Trigger::Comm);
                None
            }
            Event::Exit { pid, tgid } if pid == tgid => {
                self.procs.remove(&pid);
                None
            }
            Event::Overrun => {
                self.rescan();
                None
            }
            _ => None,
        }
    }

    pub fn recheck(&mut self, pid: u32, start_time: u64) {
        if self.procs.get(&pid).is_some_and(|e| e.start_time == start_time) {
            self.evaluate(pid, Trigger::Recheck);
        }
    }

    pub fn rescan(&mut self) {
        self.uclamp_active = system::uclamp_effective();
        let mut pids = procfs::pids();
        // Parents usually have lower pids, so their launcher state is known first.
        pids.sort_unstable();
        let alive: std::collections::HashSet<u32> = pids.iter().copied().collect();
        self.procs.retain(|pid, _| alive.contains(pid));
        for pid in pids {
            self.evaluate(pid, Trigger::Scan);
        }
        self.sweep_units();
    }

    pub fn reload(&mut self, config: Config) {
        self.config = config;
        self.apply_system();
        self.rescan();
    }

    /// Restores every process scdr changed, and the autogroup setting.
    pub fn shutdown(&mut self) {
        self.restore_all();
    }

    fn evaluate(&mut self, pid: u32, trigger: Trigger) {
        if self.paused {
            return;
        }
        let Ok(info) = ProcInfo::read(pid) else {
            self.procs.remove(&pid);
            return;
        };

        let known = self.procs.get(&pid);
        let reused = known.is_some_and(|e| e.start_time != 0 && e.start_time != info.start_time);
        if known.is_none() || reused {
            let sticky = self.inherited_sticky(info.ppid);
            self.procs.insert(pid, Entry::new(info.ppid, sticky));
        }

        if info.kthread {
            let e = self.procs.get_mut(&pid).unwrap();
            e.start_time = info.start_time;
            e.state = State::KernelThread;
            return;
        }

        let first_look = self.procs[&pid].start_time == 0;
        {
            let e = self.procs.get_mut(&pid).unwrap();
            e.start_time = info.start_time;
            e.ppid = info.ppid;
            if e.sticky.is_none() {
                if let Some(l) = self.config.exceptions.launcher(&info) {
                    e.sticky = Some(format!("started by launcher {l:?}"));
                }
            }
        }
        if trigger == Trigger::Exec && first_look && self.procs[&pid].sticky.is_none() {
            if let Some(why) = hand_tuned(pid, info.ppid) {
                self.procs.get_mut(&pid).unwrap().sticky = Some(why);
            }
        }

        let excepted = if pid == 1 || pid == self.self_pid {
            Some("system process".to_owned())
        } else if let Some(s) = &self.procs[&pid].sticky {
            Some(s.clone())
        } else {
            self.config
                .exceptions
                .exception(&info)
                .map(|n| format!("{n:?} is in [exceptions]"))
        };

        // An explicit rule wins; roles only cover processes no rule matches.
        let matched = match excepted {
            Some(_) => None,
            None => self
                .config
                .rules
                .find(&info)
                .map(|r| (RuleRef::from_rule(r), self.config.profiles[&r.profile].clone()))
                .or_else(|| self.role_match(&info)),
        };
        let matched = matched.map(|(rule, mut p)| {
            if !self.uclamp_active {
                p.profile.uclamp_min = None;
                p.profile.uclamp_max = None;
            }
            (rule, p)
        });

        let unit_profile = matched.as_ref().map(|(_, p)| p.clone());
        self.transition(pid, &info, trigger, matched, excepted);
        let managed = matches!(self.procs[&pid].state, State::Managed(_));
        self.sync_unit(pid, &info, unit_profile.as_ref().filter(|_| managed));
    }

    fn transition(
        &mut self,
        pid: u32,
        info: &ProcInfo,
        trigger: Trigger,
        matched: Option<(RuleRef, NamedProfile)>,
        excepted: Option<String>,
    ) {
        let dry_run = self.dry_run;
        let entry = self.procs.get_mut(&pid).unwrap();
        let previous = std::mem::replace(&mut entry.state, State::Pending);
        if let (Some(why), Trigger::Exec) = (&excepted, trigger) {
            tracing::debug!("{} [{pid}]: left alone: {why}", info.comm);
        }

        let idle_state = || match &excepted {
            Some(why) => State::Excepted(why.clone()),
            None => State::Unmatched,
        };

        entry.state = match (previous, matched) {
            (State::Managed(mut m), Some((rule, profile))) => {
                let target = m.original.with_profile(&profile.profile);
                if target != m.target || trigger == Trigger::Scan {
                    m.last = apply::apply(pid, &target, &[m.original, m.target], dry_run);
                    log_apply(info, &profile.name, &m.last, dry_run, target != m.target);
                }
                m.rule = rule;
                m.profile = profile.name;
                m.target = target;
                State::Managed(m)
            }
            (State::Managed(m), None) => {
                apply::apply(pid, &m.original, &[m.target], dry_run);
                tracing::info!(
                    "{} [{pid}]: restored ({})",
                    info.comm,
                    excepted.as_deref().unwrap_or("no rule matches")
                );
                idle_state()
            }
            (_, Some((rule, profile))) => match Snapshot::original(pid) {
                Ok(original) if original.task.cpu.is_protected() && profile.profile.sched.is_none() => {
                    State::Excepted("main thread is realtime or sched_ext".into())
                }
                Ok(original) => {
                    let target = original.with_profile(&profile.profile);
                    let last = apply::apply(pid, &target, &[original], dry_run);
                    log_apply(info, &profile.name, &last, dry_run, true);
                    State::Managed(Box::new(Managed {
                        rule,
                        profile: profile.name,
                        original,
                        target,
                        last,
                    }))
                }
                Err(_) => State::Unmatched, // exited
            },
            (_, None) => idle_state(),
        };
    }
}

fn log_apply(info: &ProcInfo, profile: &str, out: &Outcome, dry_run: bool, changed_target: bool) {
    if !changed_target && out.changed == 0 && out.errors.is_empty() {
        return;
    }
    let verb = if dry_run { "would apply" } else { "applied" };
    if out.errors.is_empty() {
        tracing::info!("{} [{}]: {verb} {profile} ({out})", info.comm, info.pid);
    } else {
        tracing::warn!(
            "{} [{}]: {verb} {profile} ({out}): {}",
            info.comm,
            info.pid,
            out.errors.join("; ")
        );
    }
}
