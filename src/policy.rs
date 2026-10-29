//! System-wide scheduler policy: which sched_ext scheduler runs (through
//! scx_loader), and EEVDF's own knobs while none does. Both are restored on
//! shutdown, and a scheduler someone picks by hand is left running until
//! scdr's own choice changes.

use crate::config::{Eevdf, EevdfConfig, SchedChoice, SchedExt};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set when a scheduler trial from the widget ends; the main loop then
/// calls [`Policy::forget_override`] before its next sync.
pub static FORGET_OVERRIDE: AtomicBool = AtomicBool::new(false);

const BASE_SLICE: &str = "/sys/kernel/debug/sched/base_slice_ns";
const PREEMPT: &str = "/sys/kernel/debug/sched/preempt";

#[zbus::proxy(
    interface = "org.scx.Loader",
    default_service = "org.scx.Loader",
    default_path = "/org/scx/Loader"
)]
trait Loader {
    #[zbus(property)]
    fn current_scheduler(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn scheduler_mode(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn supported_schedulers(&self) -> zbus::Result<Vec<String>>;
    fn scheduler_modes(&self, scx_name: &str) -> zbus::Result<Vec<u32>>;
    fn switch_scheduler(&self, scx_name: &str, sched_mode: u32) -> zbus::Result<()>;
    fn stop_scheduler(&self) -> zbus::Result<()>;
}

#[derive(Default)]
pub struct Policy {
    scx_original: Option<SchedChoice>,
    scx_applied: Option<SchedChoice>,
    /// Our choice at the moment someone else switched schedulers.
    scx_overridden: Option<SchedChoice>,
    scx_error: Option<String>,
    eevdf_original: Option<Eevdf>,
    eevdf_applied: Eevdf,
}

impl Policy {
    /// Brings the system in line with the config. Returns status lines.
    pub async fn sync(
        &mut self,
        bus: Option<&zbus::Connection>,
        sched_ext: &SchedExt,
        eevdf: &EevdfConfig,
        power_profile: Option<&str>,
        gaming: bool,
    ) -> Vec<String> {
        let mut report = Vec::new();
        let ext_running = match (bus, sched_ext.enable) {
            (Some(bus), true) => {
                let want = sched_ext.choose(power_profile, gaming);
                report.push(self.sync_scx(bus, want).await);
                crate::system::sched_ext().is_some_and(|s| s.is_active())
            }
            (_, enabled) => {
                if let (Some(bus), false) = (bus, enabled) {
                    self.restore_scx(bus).await;
                }
                crate::system::sched_ext().is_some_and(|s| s.is_active())
            }
        };
        // EEVDF knobs only matter while no sched_ext scheduler runs.
        let want = if ext_running { Eevdf::default() } else { eevdf.for_power(power_profile) };
        report.push(self.sync_eevdf(&want));
        report
    }

    async fn sync_scx(&mut self, bus: &zbus::Connection, want: SchedChoice) -> String {
        let loader = match loader(bus).await {
            Ok(l) => l,
            Err(e) => {
                let msg = format!("sched_ext: scx_loader unavailable ({e}); install scx-tools");
                if self.scx_error.as_deref() != Some(&msg) {
                    tracing::warn!("{msg}");
                    self.scx_error = Some(msg.clone());
                }
                return msg;
            }
        };
        self.scx_error = None;
        let Ok(current) = current(&loader).await else {
            return "sched_ext: can't read scx_loader state".into();
        };
        self.scx_original.get_or_insert_with(|| current.clone());

        if self.scx_applied.as_ref().is_some_and(|a| *a != current) {
            tracing::info!("sched_ext: switched to {current} by hand; leaving it until scdr's choice changes");
            self.scx_applied = None;
            self.scx_overridden = Some(want.clone());
        }
        if self.scx_overridden.as_ref() == Some(&want) {
            return format!("sched_ext: {current} (chosen by hand; scdr would pick {want})");
        }
        self.scx_overridden = None;

        if current != want {
            match switch(&loader, &want).await {
                Ok(()) => tracing::info!("sched_ext: {current} -> {want}"),
                Err(e) => {
                    tracing::warn!("sched_ext: can't switch to {want}: {e}");
                    return format!("sched_ext: {current} (switch to {want} failed: {e})");
                }
            }
        }
        self.scx_applied = Some(want.clone());
        format!("sched_ext: {want} (by scdr)")
    }

    async fn restore_scx(&mut self, bus: &zbus::Connection) {
        let (Some(applied), Some(original)) = (self.scx_applied.take(), self.scx_original.clone()) else {
            return;
        };
        let Ok(loader) = loader(bus).await else { return };
        if current(&loader).await.ok().as_ref() == Some(&applied) && applied != original {
            match switch(&loader, &original).await {
                Ok(()) => tracing::info!("sched_ext: restored {original}"),
                Err(e) => tracing::warn!("sched_ext: can't restore {original}: {e}"),
            }
        }
    }

    fn sync_eevdf(&mut self, want: &Eevdf) -> String {
        let current = read_eevdf();
        let Some(current) = current else {
            return "eevdf: debugfs not available".into();
        };
        let original = self.eevdf_original.get_or_insert_with(|| current.clone()).clone();
        // Fields we no longer want go back to their original values.
        let target = Eevdf {
            base_slice_us: want.base_slice_us.or_else(|| {
                self.eevdf_applied.base_slice_us.and(original.base_slice_us)
            }),
            preempt: want.preempt.clone().or_else(|| {
                self.eevdf_applied.preempt.as_ref().and(original.preempt.clone())
            }),
        };
        write_eevdf(&current, &target);
        self.eevdf_applied = want.clone();
        let now = read_eevdf().unwrap_or_default();
        format!(
            "eevdf: base_slice={}us preempt={}{}",
            now.base_slice_us.map_or("?".into(), |s| s.to_string()),
            now.preempt.as_deref().unwrap_or("?"),
            if *want == Eevdf::default() { "" } else { " (by scdr)" }
        )
    }

    /// Forgets a hand-picked scheduler (e.g. when a trial from the widget
    /// ends), so the next sync applies scdr's own choice again.
    pub fn forget_override_if_asked(&mut self) {
        if FORGET_OVERRIDE.swap(false, Ordering::Relaxed) {
            self.forget_override();
        }
    }

    pub fn forget_override(&mut self) {
        self.scx_overridden = None;
        self.scx_applied = None;
    }

    /// Puts back the scheduler and EEVDF knobs scdr changed.
    pub async fn restore(&mut self, bus: Option<&zbus::Connection>) {
        if let Some(bus) = bus {
            self.restore_scx(bus).await;
        }
        if let (Some(original), Some(current)) = (self.eevdf_original.take(), read_eevdf()) {
            if self.eevdf_applied != Eevdf::default() {
                write_eevdf(&current, &original);
                tracing::info!("eevdf: restored base_slice and preempt");
            }
        }
    }
}

/// The scheduler scx_loader runs now (`Eevdf` for none).
pub async fn current_scheduler(bus: &zbus::Connection) -> zbus::Result<SchedChoice> {
    current(&loader(bus).await?).await
}

/// Switches through scx_loader; scdr's policy treats it as a hand pick.
pub async fn switch_scheduler(bus: &zbus::Connection, to: &SchedChoice) -> zbus::Result<()> {
    switch(&loader(bus).await?, to).await
}

/// Schedulers scx_loader can start (e.g. `scx_bpfland`), each with the
/// modes it has configured.
pub async fn supported_schedulers(bus: &zbus::Connection) -> zbus::Result<Vec<(String, Vec<u32>)>> {
    let loader = loader(bus).await?;
    let mut out = Vec::new();
    for name in loader.supported_schedulers().await? {
        let modes = loader.scheduler_modes(&name).await.unwrap_or_default();
        out.push((name, modes));
    }
    Ok(out)
}

async fn loader(bus: &zbus::Connection) -> zbus::Result<LoaderProxy<'static>> {
    LoaderProxy::builder(bus)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

async fn current(loader: &LoaderProxy<'_>) -> zbus::Result<SchedChoice> {
    let name = loader.current_scheduler().await?;
    if name == "unknown" || name.is_empty() {
        return Ok(SchedChoice::Eevdf);
    }
    Ok(SchedChoice::Ext { name, mode: loader.scheduler_mode().await? })
}

async fn switch(loader: &LoaderProxy<'_>, to: &SchedChoice) -> zbus::Result<()> {
    match to {
        SchedChoice::Eevdf => loader.stop_scheduler().await,
        SchedChoice::Ext { name, mode } => loader.switch_scheduler(name, *mode).await,
    }
}

fn read_eevdf() -> Option<Eevdf> {
    let slice = fs::read_to_string(BASE_SLICE).ok()?;
    let preempt = fs::read_to_string(PREEMPT).ok();
    Some(Eevdf {
        base_slice_us: slice.trim().parse::<u32>().ok().map(|ns| ns / 1000),
        preempt: preempt.as_deref().and_then(active_preempt),
    })
}

/// `none voluntary (full) lazy` -> `full`
fn active_preempt(s: &str) -> Option<String> {
    let start = s.find('(')?;
    let end = s[start..].find(')')? + start;
    Some(s[start + 1..end].to_owned())
}

fn write_eevdf(current: &Eevdf, target: &Eevdf) {
    if let Some(us) = target.base_slice_us.filter(|t| Some(*t) != current.base_slice_us) {
        if let Err(e) = fs::write(BASE_SLICE, (u64::from(us) * 1000).to_string()) {
            tracing::warn!("eevdf: can't set base_slice_ns: {e}");
        }
    }
    if let Some(p) = target.preempt.as_ref().filter(|p| Some(*p) != current.preempt.as_ref()) {
        if let Err(e) = fs::write(PREEMPT, p) {
            tracing::warn!("eevdf: can't set preempt={p}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_preempt_mode() {
        assert_eq!(super::active_preempt("none voluntary (full) lazy\n").as_deref(), Some("full"));
        assert_eq!(super::active_preempt("none"), None);
    }
}
