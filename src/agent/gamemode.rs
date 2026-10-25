//! Follows GameMode's registered games on the session bus. Never activates
//! gamemoded: properties aren't cached and ListGames is only called while
//! it is already running.

use super::Shared;
use futures_util::StreamExt;
use zbus::names::BusName;
use zbus::zvariant::OwnedObjectPath;

const SERVICE: &str = "com.feralinteractive.GameMode";

#[zbus::proxy(
    interface = "com.feralinteractive.GameMode",
    default_service = "com.feralinteractive.GameMode",
    default_path = "/com/feralinteractive/GameMode"
)]
trait GameMode {
    fn list_games(&self) -> zbus::Result<Vec<(i32, OwnedObjectPath)>>;
    #[zbus(signal)]
    fn game_registered(&self, pid: i32, path: OwnedObjectPath) -> zbus::Result<()>;
    #[zbus(signal)]
    fn game_unregistered(&self, pid: i32, path: OwnedObjectPath) -> zbus::Result<()>;
}

pub async fn watch(session: zbus::Connection, shared: Shared) -> anyhow::Result<()> {
    let gm = GameModeProxy::builder(&session)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await?;
    let mut registered = gm.receive_game_registered().await?;
    let mut unregistered = gm.receive_game_unregistered().await?;

    let dbus = zbus::fdo::DBusProxy::new(&session).await?;
    if dbus.name_has_owner(BusName::try_from(SERVICE)?).await.unwrap_or(false) {
        if let Ok(games) = gm.list_games().await {
            shared.update(|s| s.games = games.iter().map(|(pid, _)| *pid as u32).collect());
        }
    }

    loop {
        tokio::select! {
            Some(sig) = registered.next() => {
                if let Ok(args) = sig.args() {
                    let pid = args.pid as u32;
                    tracing::info!("GameMode registered game {pid}");
                    shared.update(|s| { s.games.insert(pid); });
                }
            }
            Some(sig) = unregistered.next() => {
                if let Ok(args) = sig.args() {
                    let pid = args.pid as u32;
                    tracing::info!("GameMode unregistered game {pid}");
                    shared.update(|s| { s.games.remove(&pid); });
                }
            }
            else => return Ok(()),
        }
    }
}
