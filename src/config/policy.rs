//! System-wide policy: which CPU scheduler runs, EEVDF tuning, and the CPU
//! pressure thresholds for the background role.
//!
//! ```toml
//! [sched_ext]                    # opt-in; needs scx-tools (scx_loader)
//! enable = true
//! default = "eevdf"              # "eevdf" = no sched_ext scheduler
//! gaming = "lavd:gaming"         # while a GameMode game runs
//! [sched_ext.power]
//! power-saver = "lavd:powersave"
//!
//! [eevdf]                        # used while no sched_ext scheduler runs
//! preempt = "full"
//! [eevdf.power.power-saver]
//! preempt = "lazy"
//!
//! [pressure]                     # CPU PSI "some" avg10, in percent
//! high = 20.0
//! low = 5.0
//! hold_secs = 30                 # minimum time between switches
//! ```

use super::ConfigError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

/// A scheduler choice: the kernel's EEVDF, or a sched_ext scheduler and mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchedChoice {
    Eevdf,
    Ext { name: String, mode: u32 },
}

pub const MODES: [&str; 5] = ["auto", "gaming", "powersave", "lowlatency", "server"];

impl FromStr for SchedChoice {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "eevdf" {
            return Ok(SchedChoice::Eevdf);
        }
        let (name, mode) = s.split_once(':').unwrap_or((s, "auto"));
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!("bad scheduler name {name:?}"));
        }
        let mode = MODES
            .iter()
            .position(|m| *m == mode)
            .ok_or_else(|| format!("unknown mode {mode:?} (one of {})", MODES.join(", ")))?;
        let name = if name.starts_with("scx_") { name.to_owned() } else { format!("scx_{name}") };
        Ok(SchedChoice::Ext { name, mode: mode as u32 })
    }
}

impl fmt::Display for SchedChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchedChoice::Eevdf => f.write_str("eevdf"),
            SchedChoice::Ext { name, mode } => {
                write!(f, "{name}:{}", MODES.get(*mode as usize).unwrap_or(&"?"))
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SchedExt {
    pub enable: bool,
    pub default: Option<SchedChoice>,
    pub gaming: Option<SchedChoice>,
    pub power: BTreeMap<String, SchedChoice>,
}

impl SchedExt {
    /// Gaming first, then the power profile, then the default.
    pub fn choose(&self, power_profile: Option<&str>, gaming: bool) -> SchedChoice {
        let by_power = power_profile.and_then(|p| self.power.get(p));
        gaming
            .then_some(self.gaming.as_ref())
            .flatten()
            .or(by_power)
            .or(self.default.as_ref())
            .cloned()
            .unwrap_or(SchedChoice::Eevdf)
    }
}

/// EEVDF knobs; unset fields are left alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Eevdf {
    pub base_slice_us: Option<u32>,
    pub preempt: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct EevdfConfig {
    base: Eevdf,
    power: BTreeMap<String, Eevdf>,
}

impl EevdfConfig {
    pub fn for_power(&self, power_profile: Option<&str>) -> Eevdf {
        let mut e = self.base.clone();
        if let Some(over) = power_profile.and_then(|p| self.power.get(p)) {
            e.base_slice_us = over.base_slice_us.or(e.base_slice_us);
            e.preempt = over.preempt.clone().or(e.preempt);
        }
        e
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Pressure {
    pub high: f64,
    pub low: f64,
    /// Minimum seconds between switching the background role on and off.
    pub hold_secs: u64,
}

impl Default for Pressure {
    fn default() -> Self {
        Pressure { high: 20.0, low: 5.0, hold_secs: 30 }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawSchedExt {
    enable: Option<bool>,
    default: Option<String>,
    gaming: Option<String>,
    #[serde(default)]
    power: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawEevdf {
    base_slice_us: Option<u32>,
    preempt: Option<String>,
    #[serde(default)]
    power: BTreeMap<String, RawEevdfKnobs>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawEevdfKnobs {
    base_slice_us: Option<u32>,
    preempt: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawPressure {
    high: Option<f64>,
    low: Option<f64>,
    hold_secs: Option<u64>,
}

pub(super) fn merge_sched_ext(into: &mut SchedExt, path: &Path, raw: RawSchedExt) -> Result<(), ConfigError> {
    let parse = |s: &str| s.parse::<SchedChoice>().map_err(|e| ConfigError::invalid(path, format!("[sched_ext]: {e}")));
    if let Some(e) = raw.enable {
        into.enable = e;
    }
    if let Some(d) = raw.default {
        into.default = Some(parse(&d)?);
    }
    if let Some(g) = raw.gaming {
        into.gaming = Some(parse(&g)?);
    }
    for (profile, choice) in raw.power {
        into.power.insert(profile, parse(&choice)?);
    }
    Ok(())
}

pub(super) fn merge_eevdf(into: &mut EevdfConfig, path: &Path, raw: RawEevdf) -> Result<(), ConfigError> {
    let check = |k: &RawEevdfKnobs| -> Result<Eevdf, ConfigError> {
        if let Some(s) = k.base_slice_us {
            if !(100..=100_000).contains(&s) {
                return Err(ConfigError::invalid(path, format!("[eevdf] base_slice_us must be 100..100000, got {s}")));
            }
        }
        if let Some(p) = &k.preempt {
            if !["none", "voluntary", "full", "lazy"].contains(&p.as_str()) {
                return Err(ConfigError::invalid(path, format!("[eevdf] preempt must be none, voluntary, full or lazy, got {p:?}")));
            }
        }
        Ok(Eevdf { base_slice_us: k.base_slice_us, preempt: k.preempt.clone() })
    };
    let base = check(&RawEevdfKnobs { base_slice_us: raw.base_slice_us, preempt: raw.preempt })?;
    into.base.base_slice_us = base.base_slice_us.or(into.base.base_slice_us);
    into.base.preempt = base.preempt.or(into.base.preempt.take());
    for (profile, knobs) in raw.power {
        let k = check(&knobs)?;
        let e = into.power.entry(profile).or_default();
        e.base_slice_us = k.base_slice_us.or(e.base_slice_us);
        e.preempt = k.preempt.or(e.preempt.take());
    }
    Ok(())
}

pub(super) fn merge_pressure(into: &mut Pressure, path: &Path, raw: RawPressure) -> Result<(), ConfigError> {
    into.high = raw.high.unwrap_or(into.high);
    into.low = raw.low.unwrap_or(into.low);
    into.hold_secs = raw.hold_secs.unwrap_or(into.hold_secs);
    if into.hold_secs > 3600 {
        return Err(ConfigError::invalid(path, format!("[pressure] hold_secs must be 0..3600, got {}", into.hold_secs)));
    }
    if !(0.0..=100.0).contains(&into.low) || !(0.0..=100.0).contains(&into.high) || into.low >= into.high {
        return Err(ConfigError::invalid(path, format!(
            "[pressure] needs 0 <= low < high <= 100, got low={} high={}", into.low, into.high
        )));
    }
    Ok(())
}
