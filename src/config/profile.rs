//! Profiles: parsing, validation and inheritance.

use super::ConfigError;
use crate::sched::{IoPrio, Policy, SLICE_MAX_US, SLICE_MIN_US};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// The settings a profile applies. Unset fields are left as they are.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Profile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nice: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sched: Option<Policy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slice_us: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub io: Option<IoPrio>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oom_score_adj: Option<i32>,
    /// Per-task CPU frequency hints (0..1024); only used with schedutil.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uclamp_min: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uclamp_max: Option<u32>,
    /// systemd CPUWeight= / IOWeight= for the process's unit (1..10000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_weight: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub io_weight: Option<u64>,
}

impl Profile {
    fn overlay(&mut self, child: &Profile) {
        self.nice = child.nice.or(self.nice);
        self.sched = child.sched.or(self.sched);
        self.slice_us = child.slice_us.or(self.slice_us);
        self.io = child.io.or(self.io);
        self.oom_score_adj = child.oom_score_adj.or(self.oom_score_adj);
        self.uclamp_min = child.uclamp_min.or(self.uclamp_min);
        self.uclamp_max = child.uclamp_max.or(self.uclamp_max);
        self.cpu_weight = child.cpu_weight.or(self.cpu_weight);
        self.io_weight = child.io_weight.or(self.io_weight);
    }

    pub fn has_uclamp(&self) -> bool {
        self.uclamp_min.is_some() || self.uclamp_max.is_some()
    }

    pub fn has_unit_weights(&self) -> bool {
        self.cpu_weight.is_some() || self.io_weight.is_some()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NamedProfile {
    pub name: String,
    /// Inheritance chain, root first, ending with this profile.
    pub chain: Vec<String>,
    pub source: PathBuf,
    #[serde(flatten)]
    pub profile: Profile,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawProfile {
    inherit: Option<String>,
    nice: Option<i32>,
    sched: Option<String>,
    slice_us: Option<u32>,
    io: Option<String>,
    oom_score_adj: Option<i32>,
    uclamp_min: Option<u32>,
    uclamp_max: Option<u32>,
    cpu_weight: Option<u64>,
    io_weight: Option<u64>,
}

pub(super) fn resolve_profile(
    name: &str,
    raw: &HashMap<String, (PathBuf, RawProfile)>,
    done: &mut BTreeMap<String, NamedProfile>,
    stack: &mut Vec<String>,
) -> Result<(), ConfigError> {
    if done.contains_key(name) {
        return Ok(());
    }
    let (path, rp) = &raw[name];
    if stack.iter().any(|s| s == name) {
        stack.push(name.to_owned());
        return Err(ConfigError::invalid(
            path,
            format!("profile inheritance cycle: {}", stack.join(" -> ")),
        ));
    }

    let own = parse_profile(name, path, rp)?;
    let (mut profile, mut chain) = match &rp.inherit {
        None => (Profile::default(), Vec::new()),
        Some(parent) => {
            if !raw.contains_key(parent) {
                return Err(ConfigError::invalid(
                    path,
                    format!("profile {name:?} inherits unknown profile {parent:?}"),
                ));
            }
            stack.push(name.to_owned());
            resolve_profile(parent, raw, done, stack)?;
            stack.pop();
            let p = &done[parent];
            (p.profile.clone(), p.chain.clone())
        }
    };
    profile.overlay(&own);
    if let (Some(lo), Some(hi)) = (profile.uclamp_min, profile.uclamp_max) {
        if lo > hi {
            return Err(ConfigError::invalid(
                path,
                format!("profile {name:?}: uclamp_min {lo} is above uclamp_max {hi}"),
            ));
        }
    }
    chain.push(name.to_owned());

    done.insert(
        name.to_owned(),
        NamedProfile {
            name: name.to_owned(),
            chain,
            source: path.clone(),
            profile,
        },
    );
    Ok(())
}

fn parse_profile(name: &str, path: &Path, rp: &RawProfile) -> Result<Profile, ConfigError> {
    let err = |m: String| ConfigError::invalid(path, format!("profile {name:?}: {m}"));

    if let Some(n) = rp.nice {
        if !(-20..=19).contains(&n) {
            return Err(err(format!("nice must be -20..19, got {n}")));
        }
    }
    if let Some(s) = rp.slice_us {
        if !(SLICE_MIN_US..=SLICE_MAX_US).contains(&s) {
            return Err(err(format!(
                "slice_us must be {SLICE_MIN_US}..{SLICE_MAX_US}, got {s}"
            )));
        }
    }
    if let Some(o) = rp.oom_score_adj {
        if !(-1000..=1000).contains(&o) {
            return Err(err(format!("oom_score_adj must be -1000..1000, got {o}")));
        }
    }
    for (key, v) in [("uclamp_min", rp.uclamp_min), ("uclamp_max", rp.uclamp_max)] {
        if v.is_some_and(|v| v > 1024) {
            return Err(err(format!("{key} must be 0..1024, got {}", v.unwrap())));
        }
    }
    for (key, v) in [("cpu_weight", rp.cpu_weight), ("io_weight", rp.io_weight)] {
        if v.is_some_and(|v| !(1..=10_000).contains(&v)) {
            return Err(err(format!("{key} must be 1..10000, got {}", v.unwrap())));
        }
    }
    let sched = rp
        .sched
        .as_deref()
        .map(str::parse::<Policy>)
        .transpose()
        .map_err(err)?;
    let io = rp
        .io
        .as_deref()
        .map(str::parse::<IoPrio>)
        .transpose()
        .map_err(err)?;

    Ok(Profile {
        nice: rp.nice,
        sched,
        slice_us: rp.slice_us,
        io,
        oom_score_adj: rp.oom_score_adj,
        uclamp_min: rp.uclamp_min,
        uclamp_max: rp.uclamp_max,
        cpu_weight: rp.cpu_weight,
        io_weight: rp.io_weight,
    })
}
