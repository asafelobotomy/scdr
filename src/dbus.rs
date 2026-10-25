//! System bus interface `org.scdr.Daemon1`.
//!
//! Explain and Status return JSON (see `explain::Explain` / `explain::Status`);
//! the format may still change before 1.0. Reload is root-only via the bus
//! policy in `data/org.scdr.Daemon1.conf`. The Set* methods come from each
//! user's session agent; scdrd asks the bus for the caller's uid and only
//! accepts that user's processes.

use crate::config::{Config, Paths};
use crate::service::Service;
use std::sync::{Arc, Mutex};
use zbus::message::Header;
use zbus::names::BusName;

pub const BUS_NAME: &str = "org.scdr.Daemon1";
pub const OBJECT_PATH: &str = "/org/scdr/Daemon1";

pub struct Daemon {
    pub service: Arc<Mutex<Service>>,
    pub paths: Paths,
}

#[zbus::interface(name = "org.scdr.Daemon1")]
impl Daemon {
    /// Why a process has (or doesn't have) the priorities it has.
    fn explain(&self, pid: u32) -> String {
        let ex = self.service.lock().unwrap().explain(pid);
        serde_json::to_string(&ex).unwrap_or_default()
    }

    fn status(&self) -> String {
        let st = self.service.lock().unwrap().status();
        serde_json::to_string(&st).unwrap_or_default()
    }

    /// Re-reads the configuration and re-evaluates every process. On a config
    /// error the running configuration is kept and the error returned.
    /// The focused window's process (from the session agent's KWin script).
    async fn set_foreground(
        &self,
        pid: u32,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        let (uid, sender) = caller(&header, conn).await?;
        let mut svc = self.service.lock().unwrap();
        svc.set_foreground(uid, &sender, pid).map_err(zbus::fdo::Error::InvalidArgs)
    }

    /// Processes with a running PipeWire audio stream (replaces the set).
    async fn set_audio_clients(
        &self,
        pids: Vec<u32>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        let (uid, sender) = caller(&header, conn).await?;
        let mut svc = self.service.lock().unwrap();
        svc.set_audio(uid, &sender, pids).map_err(zbus::fdo::Error::InvalidArgs)
    }

    /// Processes registered with GameMode (replaces the set).
    async fn set_games(
        &self,
        pids: Vec<u32>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        let (uid, sender) = caller(&header, conn).await?;
        let mut svc = self.service.lock().unwrap();
        svc.set_games(uid, &sender, pids).map_err(zbus::fdo::Error::InvalidArgs)
    }

    fn reload(&self) -> zbus::fdo::Result<String> {
        let config = Config::load(&self.paths).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        let summary = format!("{} profiles, {} rules", config.profiles.len(), config.rules.len());
        self.service.lock().unwrap().reload(config);
        tracing::info!("configuration reloaded: {summary}");
        Ok(summary)
    }
}

/// The calling connection's uid and unique name, as the bus reports them.
async fn caller(header: &Header<'_>, conn: &zbus::Connection) -> zbus::fdo::Result<(u32, String)> {
    let sender = header
        .sender()
        .ok_or_else(|| zbus::fdo::Error::AccessDenied("no sender".into()))?
        .to_owned();
    let uid = zbus::fdo::DBusProxy::new(conn)
        .await?
        .get_connection_unix_user(BusName::Unique(sender.as_ref()))
        .await?;
    Ok((uid, sender.to_string()))
}

#[zbus::proxy(
    interface = "org.scdr.Daemon1",
    default_service = "org.scdr.Daemon1",
    default_path = "/org/scdr/Daemon1"
)]
pub trait Daemon1 {
    fn explain(&self, pid: u32) -> zbus::Result<String>;
    fn status(&self) -> zbus::Result<String>;
    fn reload(&self) -> zbus::Result<String>;
    fn set_foreground(&self, pid: u32) -> zbus::Result<()>;
    fn set_audio_clients(&self, pids: &[u32]) -> zbus::Result<()>;
    fn set_games(&self, pids: &[u32]) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "net.hadess.PowerProfiles",
    default_service = "net.hadess.PowerProfiles",
    default_path = "/net/hadess/PowerProfiles"
)]
pub trait PowerProfiles {
    #[zbus(property)]
    fn active_profile(&self) -> zbus::Result<String>;
}
