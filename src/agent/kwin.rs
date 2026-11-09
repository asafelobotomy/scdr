//! Loads the foreground-reporting script into KWin for this session only,
//! through KWin's scripting D-Bus API. Nothing is written to kwinrc.

use anyhow::{Context, bail};
use futures_util::StreamExt;
use std::time::Duration;

const KWIN: &str = "org.kde.KWin";
const MAX_ATTEMPTS: u32 = 8;

const SCRIPT: &str = include_str!("../../data/kwin/foreground.js");
const NAME: &str = "scdr-foreground";

#[zbus::proxy(
    interface = "org.kde.kwin.Scripting",
    default_service = "org.kde.KWin",
    default_path = "/Scripting"
)]
trait Scripting {
    // KWin's methods are camelCase; zbus would otherwise derive PascalCase.
    #[zbus(name = "loadScript")]
    fn load_script(&self, path: &str, plugin_name: &str) -> zbus::Result<i32>;
    #[zbus(name = "unloadScript")]
    fn unload_script(&self, plugin_name: &str) -> zbus::Result<bool>;
    #[zbus(name = "isScriptLoaded")]
    fn is_script_loaded(&self, plugin_name: &str) -> zbus::Result<bool>;
    #[zbus(name = "start")]
    fn start(&self) -> zbus::Result<()>;
}

pub async fn load(session: &zbus::Connection) -> anyhow::Result<()> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set")?
        .join("scdr");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("kwin-foreground.js");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(SCRIPT) {
        std::fs::write(&path, SCRIPT)?;
    }

    let kwin = ScriptingProxy::new(session).await?;
    if kwin
        .is_script_loaded(NAME)
        .await
        .context("KWin is not running")?
    {
        kwin.unload_script(NAME).await?;
    }
    let id = kwin.load_script(&path.to_string_lossy(), NAME).await?;
    if id < 0 {
        bail!("KWin refused the script ({id})");
    }
    kwin.start().await?;
    Ok(())
}

pub async fn unload(session: &zbus::Connection) {
    if let Ok(kwin) = ScriptingProxy::new(session).await {
        let _ = kwin.unload_script(NAME).await;
    }
}

/// Delay after failed load attempt `attempt` (1-based): 0.5s doubling, max 30s.
fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500u64 << attempt.saturating_sub(1).min(6)).min(Duration::from_secs(30))
}

/// Keeps the script loaded: whenever `org.kde.KWin` gains an owner (at
/// startup if already present, after a crash, or `kwin_wayland --replace`)
/// the script is (re)loaded, retrying with backoff while KWin's scripting
/// interface isn't ready yet. Waits idle while KWin is absent.
pub async fn watch(session: zbus::Connection) -> anyhow::Result<()> {
    let dbus = zbus::fdo::DBusProxy::new(&session).await?;
    // Subscribe before checking, so an appearance in between isn't missed.
    let mut changes = dbus
        .receive_name_owner_changed_with_args(&[(0, KWIN)])
        .await?;
    let mut pending = dbus.name_has_owner(KWIN.try_into()?).await?;
    let mut attempt = 0u32;
    loop {
        if pending {
            match load(&session).await {
                Ok(()) => {
                    tracing::info!("KWin foreground script loaded");
                    pending = false;
                    attempt = 0;
                }
                Err(e) => {
                    attempt += 1;
                    if attempt == 1 {
                        tracing::warn!("loading the KWin script failed, will retry: {e:#}");
                    } else {
                        tracing::debug!("KWin script load attempt {attempt}: {e:#}");
                    }
                    if attempt >= MAX_ATTEMPTS {
                        tracing::warn!("giving up on the KWin script until KWin restarts");
                        pending = false;
                        attempt = 0;
                    }
                }
            }
        }
        let retry = async {
            if pending {
                tokio::time::sleep(backoff(attempt.max(1))).await;
            } else {
                std::future::pending::<()>().await;
            }
        };
        tokio::select! {
            sig = changes.next() => {
                let Some(sig) = sig else { bail!("session bus stream ended") };
                let gained = sig.args()?.new_owner().is_some();
                if gained {
                    tracing::info!("KWin appeared on the session bus");
                }
                pending = gained;
                attempt = 0;
            }
            _ = retry => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff(1), Duration::from_millis(500));
        assert_eq!(backoff(2), Duration::from_secs(1));
        assert_eq!(backoff(3), Duration::from_secs(2));
        assert!(backoff(100) <= Duration::from_secs(30));
    }
}
