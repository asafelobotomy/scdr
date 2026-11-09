//! Daemon state: which processes exist, what applies to them, and why.

mod control;
mod inherit;
mod knobs;
mod ledger;
mod lineage;
mod pressure;
mod report;
mod roles;
mod transition;
mod unit_jobs;

pub use control::{ProcessReport, WidgetState};
pub use pressure::cpu_some_avg10;
pub use roles::SessionReport;
pub use unit_jobs::{UnitDone, UnitJob, spawn_worker as spawn_unit_worker};
mod units;

use lineage::hand_tuned;

use crate::apply::{self, Outcome, Snapshot};
use crate::config::Config;
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
    /// comm and desktop app as of the last evaluation, for reports, so
    /// listing managed processes needs no /proc reads.
    pub name: String,
    pub app: Option<String>,
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
    /// The boost a managed ancestor carried when this process was forked.
    /// Recorded then, because by its first evaluation the parent may have
    /// exited and the process been reparented (double-forking launchers
    /// such as Hyprland's exec), which hides where its values came from.
    inherited: Option<inherit::Inherited>,
}

impl Entry {
    fn new(ppid: u32, sticky: Option<String>, inherited: Option<inherit::Inherited>) -> Self {
        Entry {
            start_time: 0,
            ppid,
            sticky,
            state: State::Pending,
            inherited,
        }
    }
}

pub struct Service {
    pub config: Config,
    pub dry_run: bool,
    pub watcher: &'static str,
    procs: HashMap<u32, Entry>,
    units: BTreeMap<crate::units::UnitId, units::UnitState>,
    unit_queue: units::UnitQueue,
    autogroup_original: Option<bool>,
    /// Disks (MAJ:MIN) on which scdr enabled iocost.
    iocost_enabled: Vec<String>,
    /// Whether uclamp settings can take effect (schedutil in use).
    uclamp_active: bool,
    sessions: roles::Sessions,
    power_profile: Option<String>,
    cpu_pressure: Option<f64>,
    pressure_active: bool,
    /// When the background role last switched, for `[pressure] hold_secs`.
    pressure_switched: Option<Instant>,
    paused: bool,
    roles_off: std::collections::BTreeSet<crate::config::Role>,
    /// Scheduler policy status lines, from the main loop.
    pub policy_report: Vec<String>,
    started: Instant,
    self_pid: u32,
    /// Where the crash-recovery ledger lives (see `ledger`); `None` disables it.
    pub ledger_path: Option<std::path::PathBuf>,
    ledger_written: Option<String>,
}

impl Service {
    pub fn new(config: Config, dry_run: bool) -> Self {
        Service {
            config,
            dry_run,
            watcher: "polling",
            procs: HashMap::new(),
            units: BTreeMap::new(),
            unit_queue: Default::default(),
            autogroup_original: None,
            iocost_enabled: Vec::new(),
            uclamp_active: false,
            sessions: Default::default(),
            power_profile: None,
            cpu_pressure: None,
            pressure_active: false,
            pressure_switched: None,
            paused: false,
            roles_off: Default::default(),
            policy_report: Vec::new(),
            started: Instant::now(),
            self_pid: std::process::id(),
            ledger_path: None,
            ledger_written: None,
        }
    }

    pub fn startup(&mut self) {
        self.recover_ledger();
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
                    let inherited = self.inherited_from(parent_tgid);
                    self.procs.insert(child_pid, Entry::new(parent_tgid, sticky, inherited));
                } else {
                    self.new_thread(child_tgid, child_pid);
                }
                None
            }
            // A non-leader thread that execs takes over the process (de_thread):
            // it is the same process with a new program.
            Event::Exec { tgid, .. } => {
                self.evaluate(tgid, Trigger::Exec);
                let start = self.procs.get(&tgid)?.start_time;
                (self.config.system.exec_recheck_ms > 0).then_some((tgid, start))
            }
            Event::Comm { pid, tgid } if pid == tgid => {
                self.evaluate(pid, Trigger::Comm);
                None
            }
            // The leader thread exiting ends the process only once it has no
            // other threads; until then it is a zombie leader with live ones.
            Event::Exit { pid, tgid } if pid == tgid => {
                if procfs::tids(pid).len() <= 1 {
                    self.procs.remove(&pid);
                }
                None
            }
            Event::Overrun => {
                self.rescan();
                None
            }
            Event::Lost => {
                self.watcher = "polling";
                self.rescan();
                None
            }
            _ => None,
        }
    }

    /// A managed process started a thread. Threads inherit their creator's
    /// settings, except a realtime policy: scdr sets RESET_ON_FORK with it
    /// (so children don't run away realtime), which also resets new
    /// threads. Give those the target now rather than at the next scan.
    fn new_thread(&mut self, tgid: u32, tid: u32) {
        if self.dry_run {
            return;
        }
        if let Some(State::Managed(m)) = self.procs.get(&tgid).map(|e| &e.state) {
            if m.target.task.cpu.is_realtime() {
                apply::apply_tid(tid, &m.target, &[m.original, m.target]);
            }
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
        self.run_unit_jobs_now();
        self.save_ledger();
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
            let inherited = self.inherited_from(info.ppid);
            self.procs.insert(pid, Entry::new(info.ppid, sticky, inherited));
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
            let inherited = self.procs[&pid].inherited.or_else(|| self.inherited_from(info.ppid));
            if let Some(why) = hand_tuned(pid, info.ppid, inherited.as_ref()) {
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

        // The first time a process is up for a profile (not excepted, not
        // already managed), values it shares with the boost it was forked
        // with are inherited, not its own. That may be after an exec, not
        // at first sight: `systemd-run` is excepted (as systemd-*) until it
        // execs the app it launches. The record is used once.
        let inherit = match self.procs[&pid].state {
            State::Managed(_) => None,
            _ if excepted.is_some() => None,
            State::Pending => {
                let rec = self.procs.get_mut(&pid).unwrap().inherited.take();
                rec.or_else(|| self.inherited_from(info.ppid))
            }
            _ => self.procs.get_mut(&pid).unwrap().inherited.take(),
        };
        let unit_profile = matched.as_ref().map(|(_, p)| p.clone());
        self.transition(pid, &info, trigger, matched, excepted, inherit);
        let managed = matches!(self.procs[&pid].state, State::Managed(_));
        self.sync_unit(pid, &info, unit_profile.as_ref().filter(|_| managed));
    }
}
