//! `scdrctl agent`: the per-user session agent.
//!
//! scdrd runs as root on the system bus and can't see a desktop session, so
//! this agent runs as the user (scdr-session.service) and relays:
//!
//! - the focused window's pid: from a KWin script it loads, Hyprland's or
//!   niri's IPC, or the scdr GNOME Shell extension,
//! - processes registered with GameMode,
//! - processes with a running PipeWire audio stream.
//!
//! It re-sends everything whenever scdrd (re)appears on the system bus.

mod gamemode;
mod hyprland;
mod kwin;
mod niri;
mod pipewire;

use crate::dbus::Daemon1Proxy;
use futures_util::StreamExt;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::Notify;

pub const SESSION_NAME: &str = "org.scdr.Session1";
pub const SESSION_PATH: &str = "/org/scdr/Session1";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct State {
    pub foreground: Option<u32>,
    pub audio: BTreeSet<u32>,
    pub games: BTreeSet<u32>,
}

/// State shared between the watchers and the task that pushes to scdrd.
#[derive(Clone, Default)]
pub struct Shared {
    state: Arc<Mutex<State>>,
    changed: Arc<Notify>,
}

impl Shared {
    pub fn update(&self, f: impl FnOnce(&mut State)) {
        let mut s = self.state.lock().unwrap();
        let before = s.clone();
        f(&mut s);
        if *s != before {
            self.changed.notify_one();
        }
    }

    fn snapshot(&self) -> State {
        self.state.lock().unwrap().clone()
    }
}

/// What the KWin script calls on the session bus.
struct Session1 {
    shared: Shared,
}

#[zbus::interface(name = "org.scdr.Session1")]
impl Session1 {
    /// A string, because KWin's callDBus marshals JS numbers by its own rules.
    fn set_foreground(&self, pid: String) {
        if let Ok(pid) = pid.trim().parse::<u32>() {
            self.shared.update(|s| s.foreground = Some(pid));
        }
    }
}

pub async fn run() -> anyhow::Result<()> {
    let shared = Shared::default();
    let session = zbus::connection::Builder::session()?
        .name(SESSION_NAME)?
        .serve_at(SESSION_PATH, Session1 { shared: shared.clone() })?
        .build()
        .await?;
    let system = zbus::Connection::system().await?;

    let kwin_loaded = start_foreground(&session, &shared).await;
    tokio::spawn({
        let (session, shared) = (session.clone(), shared.clone());
        async move {
            if let Err(e) = gamemode::watch(session, shared).await {
                tracing::warn!("not following GameMode: {e:#}");
            }
        }
    });
    pipewire::spawn(shared.clone());

    let daemon = Daemon1Proxy::new(&system).await?;
    let mut daemon_restarts = zbus::fdo::DBusProxy::new(&system)
        .await?
        .receive_name_owner_changed_with_args(&[(0, crate::dbus::BUS_NAME)])
        .await?;
    let mut term = signal(SignalKind::terminate())?;
    let mut int = signal(SignalKind::interrupt())?;

    let mut sent = State::default();
    push(&daemon, &shared.snapshot(), &mut sent, true).await;
    loop {
        tokio::select! {
            _ = shared.changed.notified() => push(&daemon, &shared.snapshot(), &mut sent, false).await,
            Some(_) = daemon_restarts.next() => {
                tracing::info!("scdrd (re)appeared; re-sending session state");
                push(&daemon, &shared.snapshot(), &mut sent, true).await;
            }
            _ = term.recv() => break,
            _ = int.recv() => break,
        }
    }
    if kwin_loaded {
        kwin::unload(&session).await;
    }
    Ok(())
}

pub const GNOME_EXTENSION: &str = "scdr-foreground@scdr";

/// Picks how to learn the focused window for this desktop. Returns whether
/// a KWin script was loaded (to unload on exit).
async fn start_foreground(session: &zbus::Connection, shared: &Shared) -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let spawn = |name: &'static str, fut: std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>>| {
        tracing::info!("following focus through {name}");
        tokio::spawn(async move {
            if let Err(e) = fut.await {
                tracing::warn!("{name} focus tracking stopped: {e:#}");
            }
        });
    };
    if hyprland::available() {
        spawn("Hyprland IPC", Box::pin(hyprland::watch(shared.clone())));
        false
    } else if niri::available() {
        spawn("niri IPC", Box::pin(niri::watch(shared.clone())));
        false
    } else if desktop.split(':').any(|d| d == "GNOME") {
        tracing::info!(
            "GNOME: focus comes from the Shell extension; enable it with `gnome-extensions enable {GNOME_EXTENSION}`"
        );
        false
    } else {
        match kwin::load(session).await {
            Ok(()) => {
                tracing::info!("following focus through a KWin script");
                true
            }
            Err(e) => {
                tracing::warn!("no foreground tracking on {desktop:?}: {e:#}");
                false
            }
        }
    }
}

/// Sends what changed since `sent` (everything with `all`).
async fn push(daemon: &Daemon1Proxy<'_>, now: &State, sent: &mut State, all: bool) {
    let log = |what: &str, r: zbus::Result<()>| {
        if let Err(e) = r {
            tracing::debug!("{what}: {e}");
        }
    };
    if let Some(pid) = now.foreground.filter(|_| all || now.foreground != sent.foreground) {
        log("SetForeground", daemon.set_foreground(pid).await);
    }
    if all || now.audio != sent.audio {
        let pids: Vec<u32> = now.audio.iter().copied().collect();
        log("SetAudioClients", daemon.set_audio_clients(&pids).await);
    }
    if all || now.games != sent.games {
        let pids: Vec<u32> = now.games.iter().copied().collect();
        log("SetGames", daemon.set_games(&pids).await);
    }
    *sent = now.clone();
}
