//! `scdrctl bench`: the latency comparison, with a check of whether it can run and progress lines.

use anyhow::bail;
use serde_json::json;
use std::time::Duration;

pub(crate) struct Args {
    pub duration: u64,
    pub interval_us: u64,
    pub load: Option<usize>,
    pub repeat: usize,
    pub json: bool,
    pub ndjson: bool,
    pub check: bool,
}

/// Why a benchmark can't run now, or None. scdrd's own work would change the numbers.
async fn blocked() -> Option<String> {
    let conn = zbus::Connection::system().await.ok()?;
    let bus = zbus::fdo::DBusProxy::new(&conn).await.ok()?;
    let name = zbus::names::BusName::try_from("org.scdr.Daemon1").ok()?;
    bus.name_has_owner(name).await.ok()?.then(|| {
        "scdrd is running and would change the numbers. Pause scdr first (scdrctl pause), or stop scdrd.".to_owned()
    })
}

pub(crate) async fn run(a: Args) -> anyhow::Result<()> {
    let cpus = std::thread::available_parallelism().map_or(4, |n| n.get());
    let scenarios = scdr::bench::default_scenarios();
    let load_threads = a.load.unwrap_or(cpus * 2);
    let repeat = a.repeat.max(1);
    if a.check {
        let reason = blocked().await;
        let seconds = a.duration * scenarios.len() as u64 * repeat as u64;
        let doc = json!({
            "ok": true, "can_run": reason.is_none(), "reason": reason.clone().unwrap_or_default(),
            "scenarios": scenarios.len(), "rounds": repeat, "seconds": seconds, "cpus": cpus, "load_threads": load_threads,
        });
        if a.json || a.ndjson {
            println!("{doc}");
        } else {
            match reason {
                None => println!("can run: {} scenarios, about {seconds}s", scenarios.len()),
                Some(r) => println!("can't run: {r}"),
            }
        }
        return Ok(());
    }
    if let Some(reason) = blocked().await.filter(|_| a.ndjson) {
        bail!("{reason}");
    }
    let opt = scdr::bench::Options {
        duration: Duration::from_secs(a.duration),
        interval: Duration::from_micros(a.interval_us),
        load_threads,
    };
    let quiet = a.json || a.ndjson;
    if !quiet {
        eprintln!("{load_threads} busy threads on {cpus} CPUs, {}s per scenario, {repeat} round(s)", a.duration);
    }
    let total = scenarios.len() * repeat;
    let mut step = 0;
    let mut runs: Vec<Vec<scdr::bench::Result>> = vec![Vec::new(); scenarios.len()];
    for round in 1..=repeat {
        for (i, s) in scenarios.iter().enumerate() {
            if a.ndjson {
                println!("{}", json!({ "event": "scenario", "name": s.name, "round": round, "rounds": repeat, "percent": step * 100 / total }));
            } else if !quiet {
                eprintln!("  round {round}/{repeat}: {}", s.name);
            }
            runs[i].push(scdr::bench::run(s, &opt));
            step += 1;
        }
    }
    let results: Vec<_> = runs.iter().map(|r| scdr::bench::median(r)).collect();
    if a.ndjson {
        println!("{}", json!({ "event": "done", "percent": 100, "results": results }));
    } else if a.json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        scdr::bench::print_table(&results);
    }
    Ok(())
}
