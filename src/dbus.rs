//! System bus interface `org.scdr.Daemon1`.
//!
//! Explain, Status, Processes and the `State` property return JSON (see
//! `explain` and `service::control`); the format may still change before
//! 1.0. Reading is open to everyone. Reload, Pause, Resume and
//! SetRoleEnabled need root or the polkit action `org.scdr.control`. The
//! Set* methods come from each user's session agent; scdrd asks the bus for
//! the caller's uid and only accepts that user's processes.

use crate::config::{Config, Paths, Role};
use crate::service::Service;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zbus::message::Header;
use zbus::names::BusName;
use zbus::zvariant::Value;

pub const POLKIT_ACTION: &str = "org.scdr.control";

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

    /// Managed processes: pid, name, profile, and the rule or role behind it.
    fn processes(&self) -> String {
        serde_json::to_string(&self.service.lock().unwrap().processes()).unwrap_or_default()
    }

    /// A compact summary for the panel widget; changes are signalled.
    #[zbus(property)]
    fn state(&self) -> String {
        serde_json::to_string(&self.service.lock().unwrap().widget_state()).unwrap_or_default()
    }

    /// Restores everything scdr changed and stops changing anything.
    async fn pause(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        authorize(&header, conn).await?;
        self.service.lock().unwrap().pause();
        Ok(())
    }

    async fn resume(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        authorize(&header, conn).await?;
        self.service.lock().unwrap().resume();
        Ok(())
    }

    /// Turns a role (game, audio, foreground, background) on or off.
    async fn set_role_enabled(
        &self,
        role: String,
        enabled: bool,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        let role: Role = role.parse().map_err(zbus::fdo::Error::InvalidArgs)?;
        authorize(&header, conn).await?;
        self.service.lock().unwrap().set_role_enabled(role, enabled);
        Ok(())
    }

    /// Re-reads the configuration and re-evaluates every process. On a config
    /// error the running configuration is kept and the error returned.
    async fn reload(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<String> {
        authorize(&header, conn).await?;
        let config = Config::load(&self.paths).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        let summary = format!("{} profiles, {} rules", config.profiles.len(), config.rules.len());
        self.service.lock().unwrap().reload(config);
        tracing::info!("configuration reloaded: {summary}");
        Ok(summary)
    }
}

/// Root passes; anyone else needs polkit's `org.scdr.control`, which may
/// show an authentication dialog.
async fn authorize(header: &Header<'_>, conn: &zbus::Connection) -> zbus::fdo::Result<()> {
    let (uid, sender) = caller(header, conn).await?;
    if uid == 0 {
        return Ok(());
    }
    let polkit = zbus::Proxy::new(
        conn,
        "org.freedesktop.PolicyKit1",
        "/org/freedesktop/PolicyKit1/Authority",
        "org.freedesktop.PolicyKit1.Authority",
    )
    .await?;
    let subject = ("system-bus-name", HashMap::from([("name", Value::from(sender))]));
    let details: HashMap<&str, &str> = HashMap::new();
    const ALLOW_USER_INTERACTION: u32 = 1;
    let reply = polkit
        .call_method("CheckAuthorization", &(subject, POLKIT_ACTION, details, ALLOW_USER_INTERACTION, ""))
        .await
        .map_err(|e| zbus::fdo::Error::Failed(format!("polkit: {e}")))?;
    // The reply is one struct (bba{ss}); accept it nested or flattened.
    type Result3 = (bool, bool, HashMap<String, String>);
    let body = reply.body();
    let (authorized, _challenge, _details) = body
        .deserialize::<(Result3,)>()
        .map(|r| r.0)
        .or_else(|_| body.deserialize::<Result3>())
        .map_err(|e| zbus::fdo::Error::Failed(format!("polkit reply: {e}")))?;
    if authorized {
        Ok(())
    } else {
        Err(zbus::fdo::Error::AccessDenied(format!("not authorized for {POLKIT_ACTION}")))
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
    fn processes(&self) -> zbus::Result<String>;
    fn pause(&self) -> zbus::Result<()>;
    fn resume(&self) -> zbus::Result<()>;
    fn set_role_enabled(&self, role: &str, enabled: bool) -> zbus::Result<()>;
    #[zbus(property)]
    fn state(&self) -> zbus::Result<String>;
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
