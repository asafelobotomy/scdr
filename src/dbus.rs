//! System bus interface `org.scdr.Daemon1`.
//!
//! Explain and Status return JSON (see `explain::Explain` / `explain::Status`);
//! the format may still change before 1.0. Reload is root-only via the bus
//! policy in `data/org.scdr.Daemon1.conf`.

use crate::config::{Config, Paths};
use crate::service::Service;
use std::sync::{Arc, Mutex};

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
    fn reload(&self) -> zbus::fdo::Result<String> {
        let config = Config::load(&self.paths).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        let summary = format!("{} profiles, {} rules", config.profiles.len(), config.rules.len());
        self.service.lock().unwrap().reload(config);
        tracing::info!("configuration reloaded: {summary}");
        Ok(summary)
    }
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
}
