//! Configuration: layered TOML files, profile inheritance and validation.
//!
//! Files are read lowest precedence first:
//!
//! 1. `/usr/share/scdr/scdr.toml` (shipped defaults)
//! 2. `rules.d/*.toml` from `/usr/share/scdr` and `/etc/scdr`, sorted by file
//!    name; a file in `/etc` masks the same name in `/usr/share`
//! 3. `/etc/scdr/scdr.toml` (admin overrides)
//!
//! Higher-precedence files override `[system]` keys and whole profiles of the
//! same name, add to `[exceptions]`, and have their rules tried first.

pub(crate) mod policy;
mod profile;
mod roles;

use crate::rules::{Exceptions, RuleSet};
pub use profile::{NamedProfile, Profile};
pub use policy::{Eevdf, EevdfConfig, Pressure, SchedChoice, SchedExt};
pub use roles::{Role, RoleSet, Roles};
use profile::{RawProfile, resolve_profile};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub const ETC_DIR: &str = "/etc/scdr";
pub const SHARE_DIR: &str = "/usr/share/scdr";
const MAIN_FILE: &str = "scdr.toml";
const RULES_DIR: &str = "rules.d";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{}: {message}", path.display())]
    Invalid { path: PathBuf, message: String },
}

impl ConfigError {
    pub(crate) fn invalid(path: &Path, message: impl Into<String>) -> Self {
        ConfigError::Invalid {
            path: path.to_owned(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct System {
    /// `Some(false)` disables sched_autogroup so nice values apply system-wide;
    /// `None` leaves the kernel setting alone.
    pub autogroup: Option<bool>,
    /// Full /proc rescan interval, as a safety net for missed events. 0 disables.
    pub rescan_secs: u64,
    /// Re-check a process this long after exec, once systemd has moved it
    /// into its final cgroup. 0 disables.
    pub exec_recheck_ms: u64,
    /// Enable the iocost controller on disks without bfq, so `IOWeight=`
    /// takes effect. Restored on shutdown.
    pub iocost: bool,
}

impl Default for System {
    fn default() -> Self {
        System {
            autogroup: None,
            rescan_secs: 60,
            exec_recheck_ms: 1500,
            iocost: false,
        }
    }
}

#[derive(Debug)]
pub struct Config {
    pub system: System,
    pub profiles: BTreeMap<String, NamedProfile>,
    pub rules: RuleSet,
    pub exceptions: Exceptions,
    pub roles: Roles,
    pub sched_ext: SchedExt,
    pub eevdf: EevdfConfig,
    pub pressure: Pressure,
    /// Files that were loaded, lowest precedence first.
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Paths {
    pub etc: PathBuf,
    pub share: PathBuf,
}

impl Default for Paths {
    fn default() -> Self {
        Paths {
            etc: ETC_DIR.into(),
            share: SHARE_DIR.into(),
        }
    }
}

impl Paths {
    /// Config files that exist, lowest precedence first.
    pub fn files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();

        let share_main = self.share.join(MAIN_FILE);
        if share_main.is_file() {
            files.push(share_main);
        }

        let mut drop_ins = BTreeMap::new();
        for dir in [self.share.join(RULES_DIR), self.etc.join(RULES_DIR)] {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "toml") && path.is_file() {
                    drop_ins.insert(entry.file_name(), path);
                }
            }
        }
        files.extend(drop_ins.into_values());

        let etc_main = self.etc.join(MAIN_FILE);
        if etc_main.is_file() {
            files.push(etc_main);
        }

        files
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    system: Option<RawSystem>,
    #[serde(default)]
    profile: BTreeMap<String, RawProfile>,
    #[serde(default)]
    rule: Vec<RawRule>,
    exceptions: Option<RawExceptions>,
    roles: Option<roles::RawRoles>,
    sched_ext: Option<policy::RawSchedExt>,
    eevdf: Option<policy::RawEevdf>,
    pressure: Option<policy::RawPressure>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSystem {
    autogroup: Option<bool>,
    rescan_secs: Option<u64>,
    exec_recheck_ms: Option<u64>,
    iocost: Option<bool>,
}

/// A string or a list of strings.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum Patterns {
    One(String),
    Many(Vec<String>),
}

impl Patterns {
    pub(crate) fn as_slice(&self) -> &[String] {
        match self {
            Patterns::One(s) => std::slice::from_ref(s),
            Patterns::Many(v) => v,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawRule {
    pub profile: String,
    pub name: Option<Patterns>,
    pub exe: Option<Patterns>,
    pub cmdline: Option<Patterns>,
    pub cgroup: Option<Patterns>,
    pub parent: Option<Patterns>,
    pub uid: Option<Vec<u32>>,
    pub not_name: Option<Patterns>,
    pub not_exe: Option<Patterns>,
    pub not_cmdline: Option<Patterns>,
    pub not_cgroup: Option<Patterns>,
    pub not_parent: Option<Patterns>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawExceptions {
    #[serde(default)]
    pub name: Vec<String>,
    #[serde(default)]
    pub launchers: Vec<String>,
}

impl Config {
    pub fn load(paths: &Paths) -> Result<Self, ConfigError> {
        let files = paths.files();
        let mut sources = Vec::with_capacity(files.len());
        for path in files {
            let text = std::fs::read_to_string(&path).map_err(|source| ConfigError::Io {
                path: path.clone(),
                source,
            })?;
            sources.push((path, text));
        }
        Self::from_sources(sources)
    }

    /// Builds a config from `(path, contents)` pairs, lowest precedence first.
    pub fn from_sources(sources: Vec<(PathBuf, String)>) -> Result<Self, ConfigError> {
        let mut system = System::default();
        let mut raw_profiles: HashMap<String, (PathBuf, RawProfile)> = HashMap::new();
        let mut rule_chunks: Vec<(PathBuf, Vec<RawRule>)> = Vec::new();
        let mut exceptions = RawExceptions::default();
        let mut raw_roles = Vec::new();
        let mut sched_ext = SchedExt::default();
        let mut eevdf = EevdfConfig::default();
        let mut pressure = Pressure::default();
        let mut files = Vec::new();

        for (path, text) in sources {
            let raw: RawFile =
                toml::from_str(&text).map_err(|e| ConfigError::invalid(&path, e.to_string()))?;

            if let Some(s) = raw.system {
                system.autogroup = s.autogroup.or(system.autogroup);
                system.rescan_secs = s.rescan_secs.unwrap_or(system.rescan_secs);
                system.exec_recheck_ms = s.exec_recheck_ms.unwrap_or(system.exec_recheck_ms);
                system.iocost = s.iocost.unwrap_or(system.iocost);
            }
            for (name, profile) in raw.profile {
                raw_profiles.insert(name, (path.clone(), profile));
            }
            if let Some(ex) = raw.exceptions {
                exceptions.name.extend(ex.name);
                exceptions.launchers.extend(ex.launchers);
            }
            if let Some(r) = raw.roles {
                raw_roles.push((path.clone(), r));
            }
            if let Some(s) = raw.sched_ext {
                policy::merge_sched_ext(&mut sched_ext, &path, s)?;
            }
            if let Some(e) = raw.eevdf {
                policy::merge_eevdf(&mut eevdf, &path, e)?;
            }
            if let Some(p) = raw.pressure {
                policy::merge_pressure(&mut pressure, &path, p)?;
            }
            rule_chunks.push((path.clone(), raw.rule));
            files.push(path);
        }

        let mut profiles = BTreeMap::new();
        for name in raw_profiles.keys() {
            resolve_profile(name, &raw_profiles, &mut profiles, &mut Vec::new())?;
        }

        // Highest-precedence file's rules are tried first.
        rule_chunks.reverse();
        let rules = RuleSet::compile(rule_chunks, &profiles)?;
        let exceptions = Exceptions::compile(&exceptions)
            .map_err(|e| ConfigError::invalid(Path::new("[exceptions]"), e))?;
        let roles = roles::resolve(
            raw_roles.iter_mut().map(|(p, r)| (p.as_path(), std::mem::take(r))).collect(),
            &profiles,
        )?;

        Ok(Config {
            system,
            profiles,
            rules,
            exceptions,
            roles,
            sched_ext,
            eevdf,
            pressure,
            files,
        })
    }
}

#[cfg(test)]
mod tests;
