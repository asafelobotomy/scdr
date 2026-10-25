//! Roles: profiles a process gets because of what it is doing right now,
//! rather than what it is. They only apply to processes no rule matches.
//!
//! ```toml
//! [roles]
//! foreground = "foreground"     # the focused window's app
//! audio = "audio-client"        # playing or recording through PipeWire
//! game = "game"                 # registered with GameMode
//!
//! [roles.power.power-saver]     # per power-profiles-daemon profile
//! foreground = ""               # "" turns a role off
//! ```

use super::{ConfigError, NamedProfile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

/// Highest precedence first. `Background` only applies under CPU pressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Game,
    Audio,
    Foreground,
    Background,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Game, Role::Audio, Role::Foreground, Role::Background];

    pub fn describe(self) -> &'static str {
        match self {
            Role::Game => "registered with GameMode",
            Role::Audio => "playing or recording audio",
            Role::Foreground => "in the focused window's app",
            Role::Background => "in an app that isn't focused, while the CPU is under pressure",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Role::Game => "game",
            Role::Audio => "audio",
            Role::Foreground => "foreground",
            Role::Background => "background",
        })
    }
}

/// Which profile each role gets; `None` means the role is off.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleSet {
    pub foreground: Option<String>,
    pub audio: Option<String>,
    pub game: Option<String>,
    pub background: Option<String>,
}

impl RoleSet {
    pub fn get(&self, role: Role) -> Option<&str> {
        match role {
            Role::Game => self.game.as_deref(),
            Role::Audio => self.audio.as_deref(),
            Role::Foreground => self.foreground.as_deref(),
            Role::Background => self.background.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Roles {
    base: RoleSet,
    power: BTreeMap<String, RawRoleSet>,
}

impl Roles {
    /// The roles in effect under a power-profiles-daemon profile.
    pub fn for_power(&self, power_profile: Option<&str>) -> RoleSet {
        let mut set = self.base.clone();
        if let Some(over) = power_profile.and_then(|p| self.power.get(p)) {
            let pick = |base: &mut Option<String>, o: &Option<String>| {
                if let Some(v) = o {
                    *base = (!v.is_empty()).then(|| v.clone());
                }
            };
            pick(&mut set.foreground, &over.foreground);
            pick(&mut set.audio, &over.audio);
            pick(&mut set.game, &over.game);
            pick(&mut set.background, &over.background);
        }
        set
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawRoleSet {
    foreground: Option<String>,
    audio: Option<String>,
    game: Option<String>,
    background: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawRoles {
    foreground: Option<String>,
    audio: Option<String>,
    game: Option<String>,
    background: Option<String>,
    #[serde(default)]
    power: BTreeMap<String, RawRoleSet>,
}

/// Layers raw role tables (lowest precedence first) and checks profile names.
pub(super) fn resolve(
    layers: Vec<(&Path, RawRoles)>,
    profiles: &BTreeMap<String, NamedProfile>,
) -> Result<Roles, ConfigError> {
    let mut raw_base = RawRoleSet::default();
    let mut power: BTreeMap<String, RawRoleSet> = BTreeMap::new();
    for (path, layer) in layers {
        let set = RawRoleSet {
            foreground: layer.foreground,
            audio: layer.audio,
            game: layer.game,
            background: layer.background,
        };
        let check = |v: &Option<String>| -> Result<(), ConfigError> {
            match v.as_deref() {
                Some(name) if !name.is_empty() && !profiles.contains_key(name) => Err(
                    ConfigError::invalid(path, format!("[roles] uses unknown profile {name:?}")),
                ),
                _ => Ok(()),
            }
        };
        for s in std::iter::once(&set).chain(layer.power.values()) {
            check(&s.foreground)?;
            check(&s.audio)?;
            check(&s.game)?;
            check(&s.background)?;
        }
        overlay(&mut raw_base, set);
        for (name, set) in layer.power {
            overlay(power.entry(name).or_default(), set);
        }
    }
    let nonempty = |v: Option<String>| v.filter(|s| !s.is_empty());
    Ok(Roles {
        base: RoleSet {
            foreground: nonempty(raw_base.foreground),
            audio: nonempty(raw_base.audio),
            game: nonempty(raw_base.game),
            background: nonempty(raw_base.background),
        },
        power,
    })
}

fn overlay(into: &mut RawRoleSet, from: RawRoleSet) {
    if from.foreground.is_some() {
        into.foreground = from.foreground;
    }
    if from.audio.is_some() {
        into.audio = from.audio;
    }
    if from.game.is_some() {
        into.game = from.game;
    }
    if from.background.is_some() {
        into.background = from.background;
    }
}
