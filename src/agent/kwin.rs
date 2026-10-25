//! Loads the foreground-reporting script into KWin for this session only,
//! through KWin's scripting D-Bus API. Nothing is written to kwinrc.

use anyhow::{Context, bail};

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
    std::fs::write(&path, SCRIPT)?;

    let kwin = ScriptingProxy::new(session).await?;
    if kwin.is_script_loaded(NAME).await.context("KWin is not running")? {
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
