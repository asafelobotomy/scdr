//! What `check`, `list` and `pause --dry-run` say, for people and as JSON.

use crate::{PathArgs, daemon, not_running};
use scdr::explain::Status;
use scdr::plan::{Change, Plan};
use scdr::procfs::{self, ProcInfo};
use serde_json::json;

pub(crate) fn check(paths: &PathArgs, json: bool) -> anyhow::Result<()> {
    let c = paths.load()?;
    let files: Vec<String> = c.files.iter().map(|f| f.display().to_string()).collect();
    let warnings: Vec<String> = c.rules.warnings().iter().map(|w| w.to_string()).collect();
    if json {
        println!("{}", json!({ "ok": true, "profiles": c.profiles.len(), "rules": c.rules.len(), "files": files, "warnings": warnings }));
        return Ok(());
    }
    println!("ok: {} profiles, {} rules", c.profiles.len(), c.rules.len());
    files.iter().for_each(|f| println!("  {f}"));
    warnings.iter().for_each(|w| println!("warning: {w}"));
    Ok(())
}

struct Row {
    pid: u32,
    name: String,
    profile: String,
    rule: String,
    excepted: bool,
}

fn rows(config: &scdr::config::Config, all: bool) -> Vec<Row> {
    let mut pids = procfs::pids();
    pids.sort_unstable();
    let mut rows = Vec::new();
    for pid in pids {
        let Ok(info) = ProcInfo::read(pid) else { continue };
        if info.kthread {
            continue;
        }
        let row = |profile: &str, rule: String, excepted| Row { pid, name: info.comm.clone(), profile: profile.to_owned(), rule, excepted };
        if config.exceptions.exception(&info).is_some() || config.exceptions.launcher(&info).is_some() {
            rows.push(row("(excepted)", String::new(), true));
        } else if let Some(r) = config.rules.find(&info) {
            let file = r.source.file_name().unwrap_or_default().to_string_lossy();
            rows.push(row(&r.profile, format!("{file} #{}", r.index), false));
        } else if all {
            rows.push(row("-", String::new(), false));
        }
    }
    rows
}

pub(crate) fn list(paths: &PathArgs, all: bool, json: bool) -> anyhow::Result<()> {
    let rows = rows(&paths.load()?, all);
    if json {
        let list: Vec<_> = rows
            .iter()
            .map(|r| json!({ "pid": r.pid, "name": r.name, "profile": r.profile, "rule": r.rule, "excepted": r.excepted }))
            .collect();
        println!("{}", json!({ "ok": true, "processes": list }));
        return Ok(());
    }
    println!("{:>7}  {:<18} {:<16} RULE", "PID", "PROFILE", "NAME");
    for r in rows {
        println!("{:>7}  {:<18} {:<16} {}", r.pid, r.profile, r.name, r.rule);
    }
    Ok(())
}

/// What `pause` would restore, from the daemon's own numbers.
pub(crate) async fn pause_plan() -> anyhow::Result<Plan> {
    let d = daemon().await?;
    let state: serde_json::Value = serde_json::from_str(&d.state().await.map_err(not_running)?)?;
    if state["paused"].as_bool() == Some(true) {
        return Ok(Plan::refused("scdr is already paused", "Nothing is changed while scdr is paused."));
    }
    let st: Status = serde_json::from_str(&d.status().await.map_err(not_running)?)?;
    let mut changes = vec![Change::new(
        "Put back",
        format!("Priorities of {} app{}", st.managed, if st.managed == 1 { "" } else { "s" }),
        "each goes back to what it had before scdr",
        Some(true),
    )];
    if st.units > 0 {
        changes.push(Change::new("Put back", format!("Weights of {} unit{}", st.units, if st.units == 1 { "" } else { "s" }), "CPU and disk weights go back", Some(true)));
    }
    if st.sched_ext.is_some() {
        changes.push(Change::new("Put back", "CPU scheduler", "the scheduler scdr switched to is replaced by the one before", Some(true)));
    }
    changes.push(Change::new("Until you resume", "scdr changes nothing", "Resume starts it again at once", Some(true)));
    Ok(Plan::new("Pause scheduling", changes))
}
