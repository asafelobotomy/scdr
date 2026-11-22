//! scdrd's calls to its bus peers, and the signal streams it follows. Each stream is drained by
//! a task of its own, never by the main loop: zbus queues at most 64 messages per stream and then
//! stops reading the connection until there is room, so a loop busy with a slow call (a scheduler
//! switch takes seconds) while services start at boot filled the NameOwnerChanged queue. That
//! stopped every reply and method call, the one the loop waited for included, and scdrd answered
//! nothing, not even SIGTERM (0.5.2 on 2026-11-22; tests/busy_bus.rs).

use futures_util::StreamExt;
use scdr::dbus::PowerProfilesProxy;
use scdr::service::Service;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// How long a bus call may take while scdrd starts or looks for a peer.
pub const BUS_CALL_LIMIT: Duration = Duration::from_secs(3);
/// The longest any call scdrd makes may wait for its answer (the connection's method timeout).
pub const BUS_CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// `fut`, or `None` once it has taken longer than `BUS_CALL_LIMIT`.
async fn bounded<T>(fut: impl std::future::Future<Output = T>) -> Option<T> {
    tokio::time::timeout(BUS_CALL_LIMIT, fut).await.ok()
}

/// Reads power-profiles-daemon's profile and follows its changes on a task of their own, which
/// sends each new profile to `tx` and `None` when the daemon goes away. Returns whether it
/// follows; false when power-profiles-daemon isn't there or doesn't answer in time. It is only
/// asked when it already owns its bus name, so looking never starts it and never waits for it.
pub async fn follow_power_profiles(
    conn: &zbus::Connection,
    service: &Arc<Mutex<Service>>,
    tx: UnboundedSender<Option<String>>,
) -> bool {
    let Some(mut changes) = power_profile_changes(conn, service).await else {
        return false;
    };
    tokio::spawn(async move {
        while let Some(change) = changes.next().await {
            if let Ok(profile) = change.get().await {
                if tx.send(Some(profile)).is_err() {
                    return;
                }
            }
        }
        let _ = tx.send(None);
    });
    true
}

async fn power_profile_changes(
    conn: &zbus::Connection,
    service: &Arc<Mutex<Service>>,
) -> Option<zbus::proxy::PropertyStream<'static, String>> {
    let dbus = bounded(zbus::fdo::DBusProxy::new(conn)).await?.ok()?;
    let name = zbus::names::BusName::try_from("net.hadess.PowerProfiles").ok()?;
    if !bounded(dbus.name_has_owner(name)).await?.unwrap_or(false) {
        tracing::info!("power-profiles-daemon not running; looking again later");
        return None;
    }
    let proxy = bounded(PowerProfilesProxy::new(conn)).await?.ok()?;
    match bounded(proxy.active_profile()).await {
        Some(Ok(p)) => {
            service.lock().unwrap().set_power_profile(Some(p));
            bounded(proxy.receive_active_profile_changed()).await
        }
        Some(Err(e)) => {
            tracing::info!("power-profiles-daemon not available ({e})");
            None
        }
        None => {
            tracing::warn!("[SCDR-027] power-profiles-daemon didn't answer in {BUS_CALL_LIMIT:?}; looking again later");
            None
        }
    }
}

/// Forgets sessions whose agent has left the bus (crashed or logged out).
pub async fn drop_dead_agents(conn: &zbus::Connection, service: &Arc<Mutex<Service>>) {
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


/// Names that left the bus (a session agent that crashed or logged out, say), from a task that
/// follows NameOwnerChanged. Nothing arrives when the subscription couldn't be made in time.
pub async fn gone_names(conn: &zbus::Connection) -> UnboundedReceiver<String> {
    let (tx, rx) = unbounded_channel();
    let changes = match bounded(zbus::fdo::DBusProxy::new(conn)).await.and_then(Result::ok) {
        Some(p) => bounded(p.receive_name_owner_changed()).await.and_then(Result::ok),
        None => None,
    };
    if let Some(mut changes) = changes {
        tokio::spawn(async move {
            while let Some(sig) = changes.next().await {
                if let Ok(args) = sig.args() {
                    if args.new_owner().is_none() && tx.send(args.name().to_string()).is_err() {
                        return;
                    }
                }
            }
        });
    }
    rx
}
