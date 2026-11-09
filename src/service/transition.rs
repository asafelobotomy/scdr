//! Moving a process between states, applying or restoring its profile.

use super::inherit::Inherited;
use super::{Managed, RuleRef, Service, State, Trigger};
use crate::apply::{self, Outcome, Snapshot};
use crate::config::NamedProfile;
use crate::procfs::ProcInfo;

impl Service {
    pub(super) fn transition(
        &mut self,
        pid: u32,
        info: &ProcInfo,
        trigger: Trigger,
        matched: Option<(RuleRef, NamedProfile)>,
        excepted: Option<String>,
        inherit: Option<Inherited>,
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
                    log_apply(info, &profile.name, &m.last, dry_run, target != m.target, trigger);
                }
                m.rule = rule;
                m.profile = profile.name;
                m.target = target;
                State::Managed(m)
            }
            (State::Managed(m), None) => {
                apply::apply(pid, &m.original, &[m.target], dry_run);
                let why = excepted.as_deref().unwrap_or("no rule matches");
                // A role ending is logged once for its unit, not per process.
                if trigger == Trigger::Role {
                    tracing::debug!("{} [{pid}]: restored ({why})", info.comm);
                } else {
                    tracing::info!("{} [{pid}]: restored ({why})", info.comm);
                }
                idle_state()
            }
            (_, Some((rule, profile))) => match Snapshot::original(pid).map(|o| undo(inherit, o)) {
                Ok(original) if original.task.cpu.is_protected() && profile.profile.sched.is_none() => {
                    State::Excepted("main thread is realtime or sched_ext".into())
                }
                Ok(original) => {
                    let target = original.with_profile(&profile.profile);
                    let last = apply::apply(pid, &target, &[original], dry_run);
                    log_apply(info, &profile.name, &last, dry_run, true, trigger);
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
            (_, None) => {
                if let Some(inh) = inherit {
                    drop_inherited(pid, info, &inh, dry_run);
                }
                idle_state()
            }
        };
    }
}

fn undo(inherit: Option<Inherited>, own: Snapshot) -> Snapshot {
    match inherit {
        Some(inh) => inh.undo(own),
        None => own,
    }
}

/// A process no rule or role covers carries a boosted ancestor's values:
/// put back what it would have had without scdr.
fn drop_inherited(pid: u32, info: &ProcInfo, inh: &Inherited, dry_run: bool) {
    let Ok(own) = Snapshot::original(pid) else {
        return;
    };
    let want = inh.undo(own);
    if want == own && own.task.cpu.slice_ns == inh.target.task.cpu.slice_ns {
        return;
    }
    let out = apply::apply(pid, &want, &[inh.target], dry_run);
    if out.changed > 0 || !out.errors.is_empty() {
        tracing::debug!("{} [{pid}]: dropped inherited boost ({out})", info.comm);
    }
}

fn log_apply(
    info: &ProcInfo,
    profile: &str,
    out: &Outcome,
    dry_run: bool,
    changed_target: bool,
    trigger: Trigger,
) {
    if !changed_target && out.changed == 0 && out.errors.is_empty() {
        return;
    }
    let verb = if dry_run { "would apply" } else { "applied" };
    if out.errors.is_empty() {
        // Children usually inherit their parent's values, so most new processes
        // need no change; role switches are logged once for their unit.
        if out.changed == 0 || trigger == Trigger::Role {
            tracing::debug!("{} [{}]: {verb} {profile} ({out})", info.comm, info.pid);
        } else {
            tracing::info!("{} [{}]: {verb} {profile} ({out})", info.comm, info.pid);
        }
    } else {
        tracing::warn!(
            "{} [{}]: {verb} {profile} ({out}): {}",
            info.comm,
            info.pid,
            out.errors.join("; ")
        );
    }
}
