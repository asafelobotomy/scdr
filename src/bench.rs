//! `scdrctl bench`: wake-up latency of an interactive-style thread while
//! busy threads saturate every CPU, under scdr's levers.
//!
//! A probe thread sleeps for `interval` and records how late it actually
//! runs (timer slack set to 1 ns, so this is scheduling delay). Load threads
//! spin and count iterations, so the cost of each setting shows up as lost
//! throughput. Everything runs unprivileged: a thread may raise its own
//! nice and set its own EEVDF slice.

use crate::sched;
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
pub struct Scenario {
    pub name: String,
    /// Probe thread settings (`None` = leave as is).
    pub probe_nice: Option<i32>,
    pub probe_slice_us: Option<u32>,
    /// Load thread nice.
    pub load_nice: Option<i32>,
}

impl Scenario {
    pub fn new(name: &str, probe_nice: Option<i32>, probe_slice_us: Option<u32>, load_nice: Option<i32>) -> Self {
        Scenario { name: name.into(), probe_nice, probe_slice_us, load_nice }
    }
}

/// The comparison `scdrctl bench` runs by default, mirroring scdr's levers.
pub fn default_scenarios() -> Vec<Scenario> {
    let mut v = vec![
        Scenario::new("baseline", None, None, None),
        Scenario::new("probe slice 2000us (foreground)", None, Some(2000), None),
        Scenario::new("probe slice 1000us (audio-client)", None, Some(1000), None),
        Scenario::new("probe slice 500us (audio)", None, Some(500), None),
        Scenario::new("load nice 5 (background)", None, None, Some(5)),
        Scenario::new("slice 1000us + load nice 5", None, Some(1000), Some(5)),
    ];
    // Only root may lower nice below 0.
    if unsafe { libc::geteuid() } == 0 {
        v.push(Scenario::new("probe nice -5 (game)", Some(-5), None, None));
        v.push(Scenario::new("probe nice -11 + slice 500us (audio)", Some(-11), Some(500), None));
    }
    v
}

#[derive(Debug, Clone, Serialize)]
pub struct Result {
    pub scenario: Scenario,
    pub samples: usize,
    pub p50_us: f64,
    pub p90_us: f64,
    pub p99_us: f64,
    pub p999_us: f64,
    pub max_us: f64,
    /// Load throughput in millions of iterations per second.
    pub load_mops: f64,
    pub errors: Vec<String>,
}

pub struct Options {
    pub duration: Duration,
    pub interval: Duration,
    pub load_threads: usize,
}

pub fn run(s: &Scenario, opt: &Options) -> Result {
    let stop = Arc::new(AtomicBool::new(false));
    let ops = Arc::new(AtomicU64::new(0));
    let mut errors = Vec::new();

    let loads: Vec<_> = (0..opt.load_threads)
        .map(|_| {
            let (stop, ops, nice) = (stop.clone(), ops.clone(), s.load_nice);
            std::thread::spawn(move || {
                let err = nice.and_then(|n| set_self(Some(n), None).err());
                let mut local = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    for _ in 0..1000 {
                        local = std::hint::black_box(local.wrapping_add(1));
                    }
                    ops.fetch_add(1000, Ordering::Relaxed);
                }
                err
            })
        })
        .collect();
    std::thread::sleep(Duration::from_millis(300)); // let the load settle

    let (probe_nice, probe_slice, interval, duration) = (s.probe_nice, s.probe_slice_us, opt.interval, opt.duration);
    let probe = std::thread::spawn(move || {
        let err = set_self(probe_nice, probe_slice).err();
        // SAFETY: prctl with integer arguments; 1 ns slack measures the scheduler, not timers.
        unsafe { libc::prctl(libc::PR_SET_TIMERSLACK, 1, 0, 0, 0) };
        let mut lat = Vec::with_capacity((duration.as_micros() / interval.as_micros().max(1)) as usize);
        let end = Instant::now() + duration;
        while Instant::now() < end {
            let t0 = Instant::now();
            std::thread::sleep(interval);
            let late = t0.elapsed().saturating_sub(interval);
            lat.push(late.as_nanos() as f64 / 1000.0);
        }
        (lat, err)
    });
    let started = Instant::now();
    let ops_before = ops.load(Ordering::Relaxed);
    let (mut lat, probe_err) = probe.join().unwrap_or_default();
    let elapsed = started.elapsed().as_secs_f64();
    let load_mops = (ops.load(Ordering::Relaxed) - ops_before) as f64 / elapsed / 1e6;
    stop.store(true, Ordering::Relaxed);
    for h in loads {
        if let Ok(Some(e)) = h.join() {
            errors.push(format!("load thread: {e}"));
        }
    }
    errors.extend(probe_err.map(|e| format!("probe: {e}")));
    errors.dedup();

    lat.sort_by(f64::total_cmp);
    let pct = |p: f64| lat.get(((lat.len() as f64 - 1.0) * p).round() as usize).copied().unwrap_or(0.0);
    Result {
        scenario: s.clone(),
        samples: lat.len(),
        p50_us: pct(0.50),
        p90_us: pct(0.90),
        p99_us: pct(0.99),
        p999_us: pct(0.999),
        max_us: lat.last().copied().unwrap_or(0.0),
        load_mops,
        errors,
    }
}

/// Per-metric median over repeated runs of one scenario.
pub fn median(runs: &[Result]) -> Result {
    let med = |f: fn(&Result) -> f64| {
        let mut v: Vec<f64> = runs.iter().map(f).collect();
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let mut errors: Vec<String> = runs.iter().flat_map(|r| r.errors.clone()).collect();
    errors.dedup();
    Result {
        scenario: runs[0].scenario.clone(),
        samples: runs.iter().map(|r| r.samples).sum(),
        p50_us: med(|r| r.p50_us),
        p90_us: med(|r| r.p90_us),
        p99_us: med(|r| r.p99_us),
        p999_us: med(|r| r.p999_us),
        max_us: med(|r| r.max_us),
        load_mops: med(|r| r.load_mops),
        errors,
    }
}

/// Sets nice and/or EEVDF slice on the calling thread.
fn set_self(nice: Option<i32>, slice_us: Option<u32>) -> std::io::Result<()> {
    let tid = unsafe { libc::gettid() } as u32;
    let mut attr = sched::get_attr(tid)?;
    if let Some(n) = nice {
        attr.nice = n;
    }
    if let Some(s) = slice_us {
        attr.runtime = u64::from(s) * 1000;
    }
    attr.flags = 0;
    sched::set_attr(tid, &attr)
}

pub fn print_table(results: &[Result]) {
    println!(
        "{:<38} {:>8} {:>8} {:>8} {:>9} {:>9} {:>10}",
        "scenario", "p50 µs", "p90 µs", "p99 µs", "p99.9 µs", "max µs", "load Mop/s"
    );
    for r in results {
        println!(
            "{:<38} {:>8.0} {:>8.0} {:>8.0} {:>9.0} {:>9.0} {:>10.0}",
            r.scenario.name, r.p50_us, r.p90_us, r.p99_us, r.p999_us, r.max_us, r.load_mops
        );
        for e in &r.errors {
            println!("    ! {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_run_produces_samples() {
        // One step above our current nice: an unprivileged thread may only
        // raise it, and scdr itself may already have reniced this test.
        let tid = unsafe { libc::gettid() } as u32;
        let load_nice = (sched::get_attr(tid).unwrap().nice + 1).min(19);
        let r = run(
            &Scenario::new("t", None, Some(1000), Some(load_nice)),
            &Options { duration: Duration::from_millis(200), interval: Duration::from_millis(1), load_threads: 2 },
        );
        assert!(r.samples > 50, "{r:?}");
        assert!(r.p50_us <= r.p99_us && r.p99_us <= r.max_us);
        assert!(r.load_mops > 0.0);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
    }
}
