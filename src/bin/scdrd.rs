//! scdrd: the SCDR daemon.

use anyhow::Context;
use clap::Parser;
use scdr::config::{Config, Paths};
use scdr::dbus::{BUS_NAME, Daemon, OBJECT_PATH, PowerProfilesProxy};
use futures_util::{Stream, StreamExt};
use scdr::service::Service;
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
    let paths = Paths {
        etc: args.etc_dir,
        share: args.share_dir,
    };
    let config = Config::load(&paths).context("loading configuration")?;
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

    if args.once {
        service.startup();
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
    let polling = service.watcher == "polling";

    let service = Arc::new(Mutex::new(service));
    let bus = if args.no_dbus {
        None
    } else {
        let iface = Daemon {
            service: service.clone(),
            paths: paths.clone(),
        };
        let conn = zbus::connection::Builder::system()?
            .name(BUS_NAME)?
            .serve_at(OBJECT_PATH, iface)?
            .build()
            .await
            .context("claiming org.scdr.Daemon1 on the system bus (is scdrd already running?)")?;
        Some(conn)
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

    let rescan_every = {
        let secs = service.lock().unwrap().config.system.rescan_secs;
        match (secs, polling) {
            (0, false) => None,
            (s, true) => Some(Duration::from_secs(s.clamp(1, 5))),
            (s, false) => Some(Duration::from_secs(s)),
        }
    };
    let mut next_rescan = rescan_every.map(|d| Instant::now() + d);
    let far = || Instant::now() + Duration::from_secs(3600);

    // Exec re-checks all use the same delay, so a FIFO queue stays ordered.
    let mut rechecks: VecDeque<(Instant, u32, u64)> = VecDeque::new();

    let mut hup = signal(SignalKind::hangup())?;
    let mut term = signal(SignalKind::terminate())?;
    let mut int = signal(SignalKind::interrupt())?;
    tracing::info!("running");

    loop {
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
            Some(change) = next_item(&mut ppd_changes) => {
                if let Ok(profile) = change.get().await {
                    service.lock().unwrap().set_power_profile(Some(profile));
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
    service.lock().unwrap().shutdown();
    Ok(())
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
