//! `scdrctl agent`: the per-user session agent.
//!
//! scdrd runs as root on the system bus and can't see a desktop session, so
//! this agent runs as the user (scdr-session.service) and relays:
//!
//! - the focused window's pid: from a KWin script it loads, Hyprland's or
//!   niri's IPC, or the scdr GNOME Shell extension,
//! - processes registered with GameMode,
//! - processes with a running PipeWire audio stream,
//! - whether the screen is locked (logind's `LockedHint`): while it is, no
//!   app counts as focused.
//!
//! It re-sends everything whenever scdrd (re)appears on the system bus.

mod gamemode;
mod hyprland;
mod ipc;
mod kwin;
mod lock;
mod niri;
mod pipewire;
mod push;

use crate::dbus::Daemon1Proxy;
use futures_util::StreamExt;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::Notify;

pub const SESSION_NAME: &str = "org.scdr.Session1";
pub const SESSION_PATH: &str = "/org/scdr/Session1";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct State {
    pub foreground: Option<u32>,
    pub audio: BTreeSet<u32>,
    pub games: BTreeSet<u32>,
    /// The screen is locked: scdrd is told no app is focused.
    pub locked: bool,
}

/// State shared between the watchers and the task that pushes to scdrd.
#[derive(Clone, Default)]
pub struct Shared {
    state: Arc<Mutex<State>>,
    changed: Arc<Notify>,
}

impl Shared {
    fn notify_if(&self, changed: bool) {
        if changed {
            self.changed.notify_one();
        }
    }

    /// Records the focused window's pid (ignores `None`).
    pub fn set_foreground(&self, pid: Option<u32>) {
        let Some(pid) = pid else { return };
        let mut s = self.state.lock().unwrap();
        let changed = s.foreground != Some(pid);
        s.foreground = Some(pid);
        drop(s);
        self.notify_if(changed);
    }

    pub fn set_locked(&self, locked: bool) {
        let mut s = self.state.lock().unwrap();
        let changed = s.locked != locked;
        s.locked = locked;
        drop(s);
        self.notify_if(changed);
    }

    pub fn set_audio(&self, pids: BTreeSet<u32>) {
        let mut s = self.state.lock().unwrap();
        let changed = s.audio != pids;
        s.audio = pids;
        drop(s);
        self.notify_if(changed);
    }

    pub fn set_games(&self, pids: BTreeSet<u32>) {
        let mut s = self.state.lock().unwrap();
        let changed = s.games != pids;
        s.games = pids;
        drop(s);
        self.notify_if(changed);
    }

    pub fn add_game(&self, pid: u32) {
        let changed = self.state.lock().unwrap().games.insert(pid);
        self.notify_if(changed);
    }

    pub fn remove_game(&self, pid: u32) {
        let changed = self.state.lock().unwrap().games.remove(&pid);
        self.notify_if(changed);
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
            self.shared.set_foreground(Some(pid));
        }
    }
}

pub async fn run() -> anyhow::Result<()> {
    let shared = Shared::default();
    let session = zbus::connection::Builder::session()?
        .name(SESSION_NAME)?
        .serve_at(
            SESSION_PATH,
            Session1 {
                shared: shared.clone(),
            },
        )?
        .build()
        .await?;
    let system = zbus::Connection::system().await?;

    let kwin_path = start_foreground(&session, &shared).await;
    tokio::spawn({
        let (session, shared) = (session.clone(), shared.clone());
        async move {
            if let Err(e) = gamemode::watch(session, shared).await {
                tracing::warn!("not following GameMode: {e:#}");
            }
        }
    });
    pipewire::spawn(shared.clone());
    tokio::spawn({
        let (system, shared) = (system.clone(), shared.clone());
        async move {
            if let Err(e) = lock::watch(system, shared).await {
                tracing::warn!("not following the screen lock: {e:#}");
            }
        }
    });

    let daemon = Daemon1Proxy::new(&system).await?;
    let mut daemon_restarts = zbus::fdo::DBusProxy::new(&system)
        .await?
        .receive_name_owner_changed_with_args(&[(0, crate::dbus::BUS_NAME)])
        .await?;
    let mut term = signal(SignalKind::terminate())?;
    let mut int = signal(SignalKind::interrupt())?;

    let mut pusher = Pusher::default();
    pusher.push(&daemon, &shared.snapshot()).await;
    loop {
        // Retry unsent fields soon, but only while something is outstanding.
        let retry = async {
            if pusher.pending(&shared.snapshot()) {
                tokio::time::sleep(PUSH_RETRY).await;
            } else {
                std::future::pending::<()>().await;
            }
        };
        tokio::select! {
            _ = shared.changed.notified() => pusher.push(&daemon, &shared.snapshot()).await,
            _ = retry => pusher.push(&daemon, &shared.snapshot()).await,
            Some(_) = daemon_restarts.next() => {
                tracing::info!("scdrd (re)appeared; re-sending session state");
                pusher.sent = push::Sent::default();
                pusher.push(&daemon, &shared.snapshot()).await;
            }
            _ = term.recv() => break,
            _ = int.recv() => break,
        }
    }
    if kwin_path {
        kwin::unload(&session).await;
    }
    Ok(())
}

pub const GNOME_EXTENSION: &str = "scdr-foreground@scdr";

/// Picks how to learn the focused window for this desktop. Returns whether
/// the KWin path was chosen (so the script is unloaded on exit).
async fn start_foreground(session: &zbus::Connection, shared: &Shared) -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let spawn = |name: &'static str,
                 fut: std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>,
    >| {
        tracing::info!("following focus through {name}");
        tokio::spawn(async move {
            if let Err(e) = fut.await {
                tracing::warn!("{name} focus tracking stopped: {e:#}");
            }
        });
    };
    if hyprland::available() {
        let sh = shared.clone();
        spawn(
            "Hyprland IPC",
            Box::pin(ipc::retry("Hyprland IPC", move || {
                hyprland::watch(sh.clone())
            })),
        );
        false
    } else if niri::available() {
        let sh = shared.clone();
        spawn(
            "niri IPC",
            Box::pin(ipc::retry("niri IPC", move || niri::watch(sh.clone()))),
        );
        false
    } else if desktop.split(':').any(|d| d == "GNOME") {
        tracing::info!(
            "GNOME: focus comes from the Shell extension; enable it with `gnome-extensions enable {GNOME_EXTENSION}`"
        );
        false
    } else {
        tracing::info!(
            "following focus through a KWin script (loaded whenever KWin is on the bus)"
        );
        let session = session.clone();
        tokio::spawn(async move {
            if let Err(e) = kwin::watch(session).await {
                tracing::warn!("KWin script watcher stopped: {e:#}");
            }
        });
        true
    }
}

const PUSH_RETRY: Duration = Duration::from_secs(5);

/// Pushes state to scdrd and remembers what it accepted.
#[derive(Default)]
struct Pusher {
    sent: push::Sent,
    failing: bool,
}

impl Pusher {
    fn pending(&self, now: &State) -> bool {
        !push::plan(now, &self.sent).is_empty()
    }

    /// Sends what scdrd hasn't acknowledged; a failed call stays pending.
    async fn push(&mut self, daemon: &Daemon1Proxy<'_>, now: &State) {
        let plan = push::plan(now, &self.sent);
        if plan.is_empty() {
            return;
        }
        let mut errors = Vec::new();
        if let Some(pid) = plan.foreground {
            match daemon.set_foreground(pid).await {
                Ok(()) => self.sent.mark_foreground(pid),
                Err(e) => errors.push(format!("SetForeground: {e}")),
            }
        }
        if let Some(pids) = &plan.audio {
            match daemon.set_audio_clients(pids).await {
                Ok(()) => self.sent.mark_audio(pids),
                Err(e) => errors.push(format!("SetAudioClients: {e}")),
            }
        }
        if let Some(pids) = &plan.games {
            match daemon.set_games(pids).await {
                Ok(()) => self.sent.mark_games(pids),
                Err(e) => errors.push(format!("SetGames: {e}")),
            }
        }
        if errors.is_empty() {
            if self.failing {
                tracing::info!("session state delivered to scdrd again");
            }
            self.failing = false;
        } else {
            let msg = errors.join("; ");
            if self.failing {
                tracing::debug!("still failing: {msg}");
            } else {
                tracing::warn!("could not update scdrd, retrying every {PUSH_RETRY:?}: {msg}");
            }
            self.failing = true;
        }
    }
}
