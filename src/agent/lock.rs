//! Follows logind's `LockedHint` for the user's graphical session. While the
//! screen is locked no app is in front, so the agent tells scdrd there is no
//! focused app (pid 0) and the last window stops being boosted. Lock screens
//! set the hint through logind (uQ's uqctl bridge, KDE, GNOME, hyprlock with
//! a logind-aware idle daemon), so this works the same on every desktop.

use super::Shared;
use futures_util::StreamExt;
use zbus::OwnedObjectPath;

#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait Manager {
    fn get_session(&self, id: &str) -> zbus::Result<OwnedObjectPath>;
    fn get_user(&self, uid: u32) -> zbus::Result<OwnedObjectPath>;
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.User",
    default_service = "org.freedesktop.login1"
)]
trait User {
    /// The user's graphical session: (id, object path).
    #[zbus(property)]
    fn display(&self) -> zbus::Result<(String, OwnedObjectPath)>;
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1"
)]
trait Session {
    #[zbus(property)]
    fn locked_hint(&self) -> zbus::Result<bool>;
}

/// The session to follow: `XDG_SESSION_ID` when the agent has it (uQ passes
/// the session's environment), else the user's graphical ("display") session.
async fn session_path(system: &zbus::Connection) -> anyhow::Result<OwnedObjectPath> {
    let manager = ManagerProxy::new(system).await?;
    if let Some(id) = std::env::var("XDG_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty())
        && let Ok(path) = manager.get_session(&id).await
    {
        return Ok(path);
    }
    let user = UserProxy::builder(system)
        .path(manager.get_user(current_uid()).await?)
        .build()
        .await?;
    let (id, path) = user.display().await?;
    anyhow::ensure!(!id.is_empty(), "no graphical session for this user");
    Ok(path)
}

fn current_uid() -> u32 {
    // SAFETY: getuid cannot fail.
    unsafe { libc::getuid() }
}

/// Watches the session's lock state until the bus connection ends.
pub async fn watch(system: zbus::Connection, shared: Shared) -> anyhow::Result<()> {
    let path = session_path(&system).await?;
    let session = SessionProxy::builder(&system)
        .path(path.clone())
        .build()
        .await?;
    tracing::info!(
        "following the screen lock through logind ({})",
        path.as_str()
    );
    shared.set_locked(session.locked_hint().await.unwrap_or(false));
    let mut changes = session.receive_locked_hint_changed().await;
    while let Some(change) = changes.next().await {
        if let Ok(locked) = change.get().await {
            tracing::info!("screen {}", if locked { "locked" } else { "unlocked" });
            shared.set_locked(locked);
        }
    }
    Ok(())
}
