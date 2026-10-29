//! System bus interface `org.scdr.Daemon1`.
//!
//! Explain, Status, Processes, Settings and the `State` property return
//! JSON (see `explain`, `service::control` and `config::layer`); the format
//! may still change before 1.0. Reading is open to everyone. Reload, Pause,
//! Resume, SetRoleEnabled, SetSettings, TryScheduler and EndSchedulerTrial
//! need root or the polkit action `org.scdr.control`. The
//! Set* methods come from each user's session agent; scdrd asks the bus for
//! the caller's uid and only accepts that user's processes.

use crate::config::{Config, Paths, Role, SchedChoice};
use crate::policy;
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
    /// A scheduler being tried from the widget, and the one it replaced.
    pub trial: Mutex<Option<(SchedChoice, SchedChoice)>>,
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

    /// See [`settings_json`]. Open to everyone, like Status.
    async fn settings(&self, #[zbus(connection)] conn: &zbus::Connection) -> zbus::fdo::Result<String> {
        settings_json(self, conn).await
    }

    /// Replaces the settings layer with `json` (a document in the config
    /// file's schema), after checking it on top of the other files, then
    /// reloads. On an error nothing changes and the error is returned.
    async fn set_settings(
        &self,
        json: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<String> {
        authorize(&header, conn).await?;
        let doc: serde_json::Value =
            serde_json::from_str(&json).map_err(|e| zbus::fdo::Error::InvalidArgs(e.to_string()))?;
        let config = self.paths.write_layer(&doc).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        let summary = format!("{} profiles, {} rules", config.profiles.len(), config.rules.len());
        self.service.lock().unwrap().reload(config);
        tracing::info!("settings saved from the widget: {summary}");
        Ok(summary)
    }

    /// Runs `scheduler` ("bpfland:gaming", or "eevdf" for none) until
    /// EndSchedulerTrial, without saving it. scdr leaves it running as a
    /// hand pick meanwhile.
    async fn try_scheduler(
        &self,
        scheduler: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        let choice: SchedChoice = scheduler.parse().map_err(zbus::fdo::Error::InvalidArgs)?;
        authorize(&header, conn).await?;
        let was = self.trial.lock().unwrap().as_ref().map(|(_, was)| was.clone());
        let was = match was {
            Some(w) => w,
            None => policy::current_scheduler(conn).await?,
        };
        policy::switch_scheduler(conn, &choice).await?;
        tracing::info!("trying scheduler {choice} from the widget (was {was})");
        *self.trial.lock().unwrap() = Some((choice, was));
        Ok(())
    }

    /// Ends a trial: puts back the scheduler it replaced and lets scdr's
    /// policy choose again.
    async fn end_scheduler_trial(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        authorize(&header, conn).await?;
        let Some((_, was)) = self.trial.lock().unwrap().take() else {
            return Ok(());
        };
        policy::switch_scheduler(conn, &was).await?;
        policy::FORGET_OVERRIDE.store(true, std::sync::atomic::Ordering::Relaxed);
        tracing::info!("scheduler trial ended; back to {was}");
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

/// Everything the widget's settings pages show, as JSON: the settings layer,
/// the merged files below it, and the schedulers scx_loader offers.
async fn settings_json(d: &Daemon, conn: &zbus::Connection) -> zbus::fdo::Result<String> {
    let failed = |e: crate::config::ConfigError| zbus::fdo::Error::Failed(e.to_string());
    let (schedulers, scx_error) = match policy::supported_schedulers(conn).await {
        Ok(s) => (crate::schedulers::inventory(s), None),
        Err(e) => (Vec::new(), Some(e.to_string())),
    };
    let current = policy::current_scheduler(conn).await.ok().map(|c| c.to_string());
    let trial = d.trial.lock().unwrap().as_ref().map(|(t, w)| (t.to_string(), w.to_string()));
    let json = serde_json::json!({
        "layer_file": d.paths.layer(),
        "layer": d.paths.layer_document().map_err(failed)?,
        "base": d.paths.base_document().map_err(failed)?,
        "schedulers": schedulers,
        "scx_loader_error": scx_error,
        "current_scheduler": current,
        "trial": trial.map(|(trying, was)| serde_json::json!({"trying": trying, "was": was})),
        "modes": crate::config::policy::MODES,
        "power_profiles": ["power-saver", "balanced", "performance"],
    });
    Ok(json.to_string())
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
    fn settings(&self) -> zbus::Result<String>;
    fn set_settings(&self, json: &str) -> zbus::Result<String>;
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
