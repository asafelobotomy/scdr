//! scdrd: the SCDR daemon.

use anyhow::Context;
use clap::Parser;
use scdr::config::{Config, Paths};
use scdr::dbus::{BUS_NAME, Daemon, OBJECT_PATH, PowerProfilesProxy};
use futures_util::{Stream, StreamExt};
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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        // No colour codes in the journal.
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .without_time() // journald adds timestamps
        .with_target(false)
        .init();

    let args = Args::parse();
    // Exit 3 (scdrd.service: RestartPreventExitStatus=3) rather than a
    // restart loop while another priority daemon runs.
    let rivals = scdr::rivals::running(std::path::Path::new("/proc"));
    if !rivals.is_empty() && !args.dry_run {
        tracing::error!(
            "{} is running: it and scdr would undo each other's priorities. Import its rules \
             (scdrctl import ananicy /etc/ananicy.d --output /etc/scdr/rules.d/50-ananicy.toml), \
             then stop and disable it (systemctl disable --now <its unit>) and start scdrd again",
            rivals.join(" and ")
        );
        std::process::exit(3);
    }
    let paths = Paths {
        etc: args.etc_dir,
        share: args.share_dir,
        state: args.state_dir,
    };
    let config = load_config(&paths)?;
    if config.files.is_empty() {
        tracing::warn!("no configuration files found; nothing will be managed");
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
            "process events unavailable ({e}); falling back to rescans every {}s",
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
            .name(BUS_NAME)
            .serve_at(OBJECT_PATH, iface)
            .build()
            .await
            .context("claiming org.scdr.Daemon1 on the system bus (is scdrd already running?)")?;
        Some(conn)
    };

    // A session agent leaving the bus (crash, logout) drops its roles at once;
    // the periodic check in the rescan arm stays as a backstop.
    let bus_proxy = match &bus {
        Some(conn) => zbus::fdo::DBusProxy::new(conn).await.ok(),
        None => None,
    };
    let mut owner_changes = match &bus_proxy {
        Some(p) => p.receive_name_owner_changed().await.ok(),
        None => None,
    };

    // Follow power-profiles-daemon when it is running; roles can depend on it.
    let ppd = match &bus {
        Some(conn) => power_profiles(conn, &service).await,
        None => None,
    };
    let mut ppd_changes = match &ppd {
        Some(p) => Some(p.receive_active_profile_changed().await),
        None => None,
    };

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
    let mut last_state = String::new();
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
                    drop_dead_agents(conn, &service).await;
                }
                service.lock().unwrap().rescan();
                next_rescan = rescan_every.map(|d| Instant::now() + d);
            }
            Some(sig) = next_item(&mut owner_changes) => {
                if let Ok(args) = sig.args() {
                    if args.new_owner().is_none() {
                        service.lock().unwrap().drop_agent(args.name().as_str());
                    }
                }
            }
            Some(change) = next_item(&mut ppd_changes) => {
                if let Ok(profile) = change.get().await {
                    service.lock().unwrap().set_power_profile(Some(profile));
                    policy_step(&mut policy, bus.as_ref(), &service).await;
                }
            }
            _ = tick.tick() => {
                service.lock().unwrap().save_ledger();
                policy_step(&mut policy, bus.as_ref(), &service).await;
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
                    Err(e) => tracing::error!("reload failed, keeping current config: {e}"),
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
            tracing::error!("{err}; moved the widget's settings to {} and started without them", bad.display());
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
    let report = policy.sync(bus, &sched_ext, &eevdf, power.as_deref(), gaming).await;
    service.lock().unwrap().policy_report = report;
}

async fn power_profiles(
    conn: &zbus::Connection,
    service: &Arc<Mutex<Service>>,
) -> Option<PowerProfilesProxy<'static>> {
    let proxy = PowerProfilesProxy::new(conn).await.ok()?;
    match proxy.active_profile().await {
        Ok(p) => {
            service.lock().unwrap().set_power_profile(Some(p));
            Some(proxy)
        }
        Err(e) => {
            tracing::info!("power-profiles-daemon not available ({e})");
            None
        }
    }
}

/// Forgets sessions whose agent has left the bus (crashed or logged out).
async fn drop_dead_agents(conn: &zbus::Connection, service: &Arc<Mutex<Service>>) {
    let senders = service.lock().unwrap().agent_senders();
    if senders.is_empty() {
        return;
    }
    let Ok(dbus) = zbus::fdo::DBusProxy::new(conn).await else {
        return;
    };
    for sender in senders {
        let alive = match zbus::names::BusName::try_from(sender.as_str()) {
            Ok(name) => dbus.name_has_owner(name).await.unwrap_or(true),
            Err(_) => false,
        };
        if !alive {
            service.lock().unwrap().drop_agent(&sender);
        }
    }
}

async fn next_item<S: Stream + Unpin>(stream: &mut Option<S>) -> Option<S::Item> {
    match stream {
        Some(s) => s.next().await,
        None => std::future::pending().await,
    }
}

/// Emits PropertiesChanged for `State` when the summary differs from the last one.
async fn publish_state(conn: &zbus::Connection, service: &Arc<Mutex<Service>>, last: &mut String) {
    let now = serde_json::to_string(&service.lock().unwrap().widget_state()).unwrap_or_default();
    if now == *last {
        return;
    }
    *last = now;
    if let Ok(iface) = conn.object_server().interface::<_, Daemon>(OBJECT_PATH).await {
        let _ = iface.get().await.state_changed(iface.signal_emitter()).await;
    }
}
