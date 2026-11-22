//! scdrd: the SCDR daemon.

mod peers;

use anyhow::Context;
use clap::Parser;
use scdr::config::{Config, Paths};
use scdr::dbus::{BUS_NAME, Daemon, OBJECT_PATH, Published, publish_state};
use scdr::policy::Policy;
use scdr::service::{Service, cpu_some_avg10};
use scdr::watcher;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::signal::unix::{SignalKind, signal};
use tokio::time::{Instant, sleep_until};

#[derive(Parser)]
#[command(version, about = "SCDR process scheduling daemon")]
struct Args {
    /// Log what would change without changing anything.
    #[arg(long)]
    dry_run: bool,
    /// Scan once, print a summary and exit (no D-Bus, no event watching).
    #[arg(long)]
    once: bool,
    /// Don't claim the D-Bus name (for testing alongside a running daemon).
    #[arg(long)]
    no_dbus: bool,
    /// Admin config directory.
    #[arg(long, default_value = scdr::config::ETC_DIR)]
    etc_dir: PathBuf,
    /// Shipped config directory.
    #[arg(long, default_value = scdr::config::SHARE_DIR)]
    share_dir: PathBuf,
    /// State directory: runtime.json and the widget's settings layer.
    #[arg(long, env = "STATE_DIRECTORY", default_value = scdr::config::STATE_DIR)]
    state_dir: PathBuf,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // uQ's log format (`<4>scdrd: [SCDR-…] …` in the journal) and a crash record on a panic.
    scdr::uq_diag::init("scdrd", "info");

    let args = Args::parse();
    // Exit 3 (scdrd.service: RestartPreventExitStatus=3) rather than a
    // restart loop while another priority daemon runs.
    let rivals = scdr::rivals::running(std::path::Path::new("/proc"));
    if !rivals.is_empty() && !args.dry_run {
        tracing::error!(
            "[SCDR-022] {} is running: it and scdr would undo each other's priorities. Import its rules \
             (scdrctl import ananicy /etc/ananicy.d --output /etc/scdr/rules.d/50-ananicy.toml), \
             then stop and disable it (systemctl disable --now <its unit>) and start scdrd again",
            rivals.join(" and ")
        );
        std::process::exit(3);
    }
    for d in scdr::rivals::overlapping(std::path::Path::new("/proc")) {
        tracing::warn!(
            "[SCDR-023] {d} is running: it also boosts games and switches sched_ext schedulers. Leave one of them in \
             charge of games and schedulers (scdr: turn off the game role and [sched_ext])"
        );
    }
    let paths = Paths {
        etc: args.etc_dir,
        share: args.share_dir,
        state: args.state_dir,
    };
    let config = load_config(&paths)?;
    if config.files.is_empty() {
        tracing::warn!("[SCDR-024] no configuration files found; nothing will be managed");
    }
    tracing::info!(
        "{} profiles, {} rules from {} files{}",
        config.profiles.len(),
        config.rules.len(),
        config.files.len(),
        if args.dry_run { " (dry run)" } else { "" }
    );

    let mut service = Service::new(config, args.dry_run);
    service.load_runtime();
    // systemd's RuntimeDirectory=scdr; kept across restarts so a crash can be undone.
    service.ledger_path = std::env::var_os("RUNTIME_DIRECTORY")
        .map(|d| std::path::PathBuf::from(d).join("ledger.json"))
        .filter(|_| !args.once);
    // Focus hints for scx_devbox go next to it (see service::devbox).
    service.hints_path = service
        .ledger_path
        .as_ref()
        .map(|p| p.with_file_name(scdr::service::DEVBOX_HINTS_FILE));

    if args.once {
        service.startup();
        service.run_unit_jobs_now();
        print!("{}", service.status());
        return Ok(());
    }

    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    match watcher::spawn(tx) {
        Ok(()) => service.watcher = "netlink",
        Err(e) => tracing::warn!(
            "[SCDR-011] process events unavailable ({e}); falling back to rescans every {}s",
            service.config.system.rescan_secs.max(5)
        ),
    }
    service.startup();
    let mut polling = service.watcher == "polling";

    // systemctl runs on a worker thread so it never blocks this loop.
    let (done_tx, mut unit_done) = tokio::sync::mpsc::unbounded_channel();
    let (job_tx, unit_worker) = scdr::service::spawn_unit_worker(done_tx)?;

    let service = Arc::new(Mutex::new(service));
    let bus = if args.no_dbus {
        None
    } else {
        let iface = Daemon {
            service: service.clone(),
            paths: paths.clone(),
            trial: Default::default(),
        };
        let conn = zbus::connection::Builder::system()
            // No call to a peer may wait forever (scx_loader, systemd, power-profiles-daemon).
            .method_timeout(peers::BUS_CALL_TIMEOUT)
            .name(BUS_NAME)
            .serve_at(OBJECT_PATH, iface)
            .build()
            .await
            .context("claiming org.scdr.Daemon1 on the system bus (is scdrd already running?)")?;
        Some(conn)
    };

    // A session agent leaving the bus (crash, logout) drops its roles at once;
    // the periodic check in the rescan arm stays as a backstop.
    // Every bus call at startup is bounded: a peer that never answers (power-profiles-daemon
    // starting at the same moment, say) must not stop the loop from ever running.
    //
    // Signal streams are drained by tasks of their own, never by the loop below (see peers.rs).
    let mut gone_agents = match &bus {
        Some(conn) => peers::gone_names(conn).await,
        None => tokio::sync::mpsc::unbounded_channel().1,
    };

    // Follow power-profiles-daemon when it is running; roles can depend on it.
    let (ppd_tx, mut ppd_changes) = tokio::sync::mpsc::unbounded_channel::<Option<String>>();
    let mut ppd_following = match &bus {
        Some(conn) => peers::follow_power_profiles(conn, &service, ppd_tx.clone()).await,
        None => false,
    };
    let mut ppd_retry = 0u32;

    let rescan_interval = |secs: u64, polling: bool| match (secs, polling) {
        (0, false) => None,
        (s, true) => Some(Duration::from_secs(s.clamp(1, 5))),
        (s, false) => Some(Duration::from_secs(s)),
    };
    let mut rescan_every = rescan_interval(service.lock().unwrap().config.system.rescan_secs, polling);
    let mut next_rescan = rescan_every.map(|d| Instant::now() + d);
    let far = || Instant::now() + Duration::from_secs(3600);

    // Exec re-checks all use the same delay, so a FIFO queue stays ordered.
    let mut rechecks: VecDeque<(Instant, u32, u64)> = VecDeque::new();

    // Pressure sampling and scheduler policy run on a short tick, which also
    // publishes the State property when it changes.
    let mut policy = Policy::default();
    let mut last_state = Published::default();
    let mut tick = tokio::time::interval(Duration::from_secs(2));

    let mut hup = signal(SignalKind::hangup())?;
    let mut term = signal(SignalKind::terminate())?;
    let mut int = signal(SignalKind::interrupt())?;
    tracing::info!("running");

    loop {
        for job in service.lock().unwrap().take_unit_jobs() {
            let _ = job_tx.send(job);
        }
        let recheck_at = rechecks.front().map(|r| r.0).unwrap_or_else(far);
        tokio::select! {
            Some(ev) = events.recv() => {
                let mut svc = service.lock().unwrap();
                let delay = Duration::from_millis(svc.config.system.exec_recheck_ms);
                if let Some((pid, start)) = svc.handle(ev) {
                    rechecks.push_back((Instant::now() + delay, pid, start));
                }
                // Drain whatever else is queued while we hold the lock.
                while let Ok(ev) = events.try_recv() {
                    if let Some((pid, start)) = svc.handle(ev) {
                        rechecks.push_back((Instant::now() + delay, pid, start));
                    }
                }
                // The event socket died for good: poll /proc instead.
                if !polling && svc.watcher == "polling" {
                    polling = true;
                    rescan_every = rescan_interval(svc.config.system.rescan_secs, true);
                    next_rescan = rescan_every.map(|d| Instant::now() + d);
                }
            }
            Some(done) = unit_done.recv() => {
                service.lock().unwrap().unit_done(done);
            }
            _ = sleep_until(recheck_at), if !rechecks.is_empty() => {
                let now = Instant::now();
                let mut svc = service.lock().unwrap();
                while rechecks.front().is_some_and(|r| r.0 <= now) {
                    let (_, pid, start) = rechecks.pop_front().unwrap();
                    svc.recheck(pid, start);
                }
            }
            _ = sleep_until(next_rescan.unwrap_or_else(far)), if next_rescan.is_some() => {
                if let Some(conn) = &bus {
                    peers::drop_dead_agents(conn, &service).await;
                }
                service.lock().unwrap().rescan();
                next_rescan = rescan_every.map(|d| Instant::now() + d);
            }
            Some(name) = gone_agents.recv() => {
                service.lock().unwrap().drop_agent(&name);
            }
            Some(change) = ppd_changes.recv() => match change {
                Some(profile) => {
                    service.lock().unwrap().set_power_profile(Some(profile));
                    if stopped(policy_step(&mut policy, bus.as_ref(), &service), &mut term, &mut int).await {
                        break;
                    }
                }
                // power-profiles-daemon went away: look for it again on the tick.
                None => ppd_following = false,
            },
            _ = tick.tick() => {
                // power-profiles-daemon may come up after us: look again every 30 s.
                ppd_retry += 1;
                if !ppd_following && ppd_retry % 15 == 0 {
                    if let Some(conn) = &bus {
                        ppd_following = peers::follow_power_profiles(conn, &service, ppd_tx.clone()).await;
                    }
                }
                service.lock().unwrap().save_ledger();
                // A scheduler switch can take many seconds: stop requests still end it.
                if stopped(policy_step(&mut policy, bus.as_ref(), &service), &mut term, &mut int).await {
                    break;
                }
                if let Some(conn) = &bus {
                    publish_state(conn, &service, &mut last_state).await;
                }
            }
            _ = hup.recv() => {
                match Config::load(&paths) {
                    Ok(config) => {
                        tracing::info!("reloading: {} profiles, {} rules", config.profiles.len(), config.rules.len());
                        service.lock().unwrap().reload(config);
                    }
                    Err(e) => tracing::error!("[SCDR-025] reload failed, keeping current config: {e}"),
                }
            }
            _ = term.recv() => break,
            _ = int.recv() => break,
        }
    }

    tracing::info!("shutting down");
    // Let the worker finish what it has, so in-flight claims get restored too.
    drop(job_tx);
    let _ = unit_worker.join();
    while let Ok(done) = unit_done.try_recv() {
        service.lock().unwrap().unit_done(done);
    }
    policy.restore(bus.as_ref()).await;
    service.lock().unwrap().shutdown();
    Ok(())
}

/// Loads the configuration. The widget's settings layer is only checked when
/// saved, so an upgrade that drops a profile it names would stop scdrd from
/// starting, and the widget can't fix it while scdrd is down: move a layer
/// that breaks the config aside and start without it.
fn load_config(paths: &Paths) -> anyhow::Result<Config> {
    let err = match Config::load(paths) {
        Ok(c) => return Ok(c),
        Err(e) => e,
    };
    let layer = paths.state.join(scdr::config::layer::LAYER_FILE);
    let bad = layer.with_extension("toml.bad");
    if !layer.is_file() || std::fs::rename(&layer, &bad).is_err() {
        return Err(err).context("loading configuration");
    }
    match Config::load(paths) {
        Ok(c) => {
            tracing::error!("[SCDR-026] {err}; moved the widget's settings to {} and started without them", bad.display());
            Ok(c)
        }
        Err(_) => {
            // The layer wasn't the problem: put it back.
            let _ = std::fs::rename(&bad, &layer);
            Err(err).context("loading configuration")
        }
    }
}

/// Samples CPU pressure, then brings the scheduler and EEVDF knobs in line.
async fn policy_step(policy: &mut Policy, bus: Option<&zbus::Connection>, service: &Arc<Mutex<Service>>) {
    if service.lock().unwrap().is_paused() {
        policy.restore(bus).await; // a no-op once restored
        service.lock().unwrap().policy_report = vec!["paused: scheduler and EEVDF knobs left as they were".into()];
        return;
    }
    policy.forget_override_if_asked();
    let (sched_ext, eevdf, power, gaming) = {
        let mut svc = service.lock().unwrap();
        if let Some(avg10) = cpu_some_avg10() {
            svc.set_cpu_pressure(avg10);
        }
        let power = svc.power_profile().map(str::to_owned);
        (svc.config.sched_ext.clone(), svc.config.eevdf.clone(), power, svc.gaming())
    };
    let mut report = policy.sync(bus, &sched_ext, &eevdf, power.as_deref(), gaming).await;
    let mut svc = service.lock().unwrap();
    svc.sync_devbox();
    if svc.devbox_hints() {
        report.push("devbox: focus hints on; foreground and background roles run in the scheduler".into());
    }
    svc.policy_report = report;
}

/// Runs `step` unless SIGTERM or SIGINT comes first; true when one did (the step is dropped).
async fn stopped(
    step: impl std::future::Future<Output = ()>,
    term: &mut tokio::signal::unix::Signal,
    int: &mut tokio::signal::unix::Signal,
) -> bool {
    tokio::select! {
        _ = step => false,
        _ = term.recv() => true,
        _ = int.recv() => true,
    }
}
