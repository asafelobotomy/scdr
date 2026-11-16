//! `scdrctl config`: the settings a person can change without writing rules, read from and saved to
//! the settings layer (see [`super::layer`]). Every key is a dotted path in the config file's own
//! schema, so what the file would accept is decided by the loader, not by a second list here: the
//! table below only says how to show a key and how to read a typed value for it.

use super::{Config, ConfigError, Paths};
use crate::plan::{Change, Plan};
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    /// A true/false that can also be left to the kernel (`system.autogroup`).
    OptionalBool,
    Uint,
    Float,
    Text,
    List,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Bool | Kind::OptionalBool => "bool",
            Kind::Uint => "uint",
            Kind::Float => "float",
            Kind::Text => "text",
            Kind::List => "list",
        }
    }
}

pub struct Key {
    pub key: &'static str,
    pub kind: Kind,
    pub group: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub unit: &'static str,
    pub choices: &'static [&'static str],
    /// Lists of files below the layer are added to, not replaced (`[exceptions]`).
    pub adds: bool,
    /// What the key is without any file that sets it.
    pub default: Option<&'static str>,
}

const fn key(key: &'static str, kind: Kind, group: &'static str, label: &'static str, help: &'static str) -> Key {
    Key { key, kind, group, label, help, min: None, max: None, unit: "", choices: &[], adds: false, default: None }
}

const PREEMPT: &[&str] = &["none", "voluntary", "full", "lazy"];

pub const KEYS: &[Key] = &[
    Key { default: Some("false"), ..key("system.iocost", Kind::Bool, "System", "Disk weights on every disk", "Turn on the kernel's iocost controller on disks without bfq, so IOWeight takes effect. Put back when scdr stops.") },
    key("system.autogroup", Kind::OptionalBool, "System", "Kernel autogroup", "Off makes nice values apply system-wide. Unset leaves the kernel setting alone."),
    Key { min: Some(0.0), max: Some(86400.0), unit: "s", default: Some("60"), ..key("system.rescan_secs", Kind::Uint, "System", "Full rescan every", "A safety net for missed process events. 0 turns it off.") },
    Key { min: Some(0.0), max: Some(60000.0), unit: "ms", default: Some("1500"), ..key("system.exec_recheck_ms", Kind::Uint, "System", "Look again after a start", "Check a new process again this long after it starts, once systemd has put it in its final group. 0 turns it off.") },
    Key { min: Some(0.0), max: Some(100.0), unit: "%", default: Some("20"), ..key("pressure.high", Kind::Float, "CPU pressure", "Busy above", "Background apps get lowered priority above this CPU pressure.") },
    Key { min: Some(0.0), max: Some(100.0), unit: "%", default: Some("5"), ..key("pressure.low", Kind::Float, "CPU pressure", "Calm below", "Background apps go back to normal below this CPU pressure.") },
    Key { min: Some(0.0), max: Some(3600.0), unit: "s", default: Some("30"), ..key("pressure.hold_secs", Kind::Uint, "CPU pressure", "Wait between switches", "The least time between lowering and raising background apps.") },
    Key { choices: PREEMPT, ..key("eevdf.preempt", Kind::Text, "Kernel scheduler", "Preemption", "How eagerly the kernel scheduler interrupts a running task. Used while no sched_ext scheduler runs.") },
    Key { min: Some(100.0), max: Some(100000.0), unit: "µs", ..key("eevdf.base_slice_us", Kind::Uint, "Kernel scheduler", "Base time slice", "The kernel scheduler's base slice. Used while no sched_ext scheduler runs.") },
    Key { default: Some("false"), ..key("sched_ext.enable", Kind::Bool, "CPU scheduler", "Use a sched_ext scheduler", "Needs scx_loader (scx-tools).") },
    key("sched_ext.default", Kind::Text, "CPU scheduler", "Scheduler", "A name like bpfland:gaming, or eevdf for the kernel's own."),
    key("sched_ext.gaming", Kind::Text, "CPU scheduler", "While a game runs", "A name like lavd:gaming. Empty uses the scheduler above."),
    Key { adds: true, ..key("exceptions.name", Kind::List, "Left alone", "Never touch these", "Process names scdr never changes.") },
    Key { adds: true, ..key("exceptions.launchers", Kind::List, "Left alone", "Launchers", "Programs that start other programs: what they start is judged by itself, not by its parent.") },
    key("split.apps", Kind::List, "Agent tools", "Apps whose tools get their own group", "The helper processes of these apps move to a group of their own."),
    key("split.keep", Kind::List, "Agent tools", "Helpers that stay with the app", "Crash reporters, sandbox helpers."),
    key("split.memory_high", Kind::Text, "Agent tools", "Memory soft limit", "Bytes with K, M, G or T, a percentage, or infinity."),
    Key { min: Some(1.0), max: Some(10000.0), ..key("split.io_weight", Kind::Uint, "Agent tools", "Disk weight", "") },
    Key { min: Some(1.0), max: Some(10000.0), ..key("split.cpu_weight", Kind::Uint, "Agent tools", "CPU weight", "") },
];

/// How to read `text` for a dotted `key` that isn't in [`KEYS`] but that the file takes:
/// the per-power-profile choices.
fn free_kind(key: &str) -> Option<Kind> {
    let parts: Vec<&str> = key.split('.').collect();
    match parts.as_slice() {
        ["sched_ext", "power", p] if !p.is_empty() => Some(Kind::Text),
        ["eevdf", "power", p, "preempt"] if !p.is_empty() => Some(Kind::Text),
        ["eevdf", "power", p, "base_slice_us"] if !p.is_empty() => Some(Kind::Uint),
        ["roles", r] if ["foreground", "audio", "game", "background"].contains(r) => Some(Kind::Text),
        ["roles", "power", p, r] if !p.is_empty() && ["foreground", "audio", "game", "background"].contains(r) => Some(Kind::Text),
        _ => None,
    }
}

pub fn lookup(name: &str) -> Option<&'static Key> {
    KEYS.iter().find(|k| k.key == name)
}

fn kind_of(name: &str) -> Result<Kind, String> {
    lookup(name).map(|k| k.kind).or_else(|| free_kind(name)).ok_or_else(|| {
        format!("{name:?} isn't a setting `scdrctl config` changes (see `scdrctl config show`). Profiles and rules are edited in the widget or in /etc/scdr")
    })
}

/// `text` as the JSON value for `name`. Lists are comma separated; an empty text, `[]` or `none` is an empty list.
pub fn parse_value(name: &str, text: &str) -> Result<Value, String> {
    let text = text.trim();
    Ok(match kind_of(name)? {
        Kind::Bool | Kind::OptionalBool => match text {
            "true" | "on" | "yes" | "1" => json!(true),
            "false" | "off" | "no" | "0" => json!(false),
            _ => return Err(format!("{name} is on or off, not {text:?}")),
        },
        Kind::Uint => json!(text.parse::<u64>().map_err(|_| format!("{name} is a whole number, not {text:?}"))?),
        Kind::Float => match text.parse::<f64>() {
            Ok(v) if v.is_finite() => json!(v),
            _ => return Err(format!("{name} is a number, not {text:?}")),
        },
        Kind::Text => json!(text),
        Kind::List if text == "[]" || text == "none" => json!([]),
        Kind::List => json!(text.split(',').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>()),
    })
}

fn at<'a>(doc: &'a Value, name: &str) -> Option<&'a Value> {
    name.split('.').try_fold(doc, |v, part| v.get(part))
}

pub fn set_path(doc: &mut Value, name: &str, value: Value) {
    let mut cur = doc;
    let mut parts = name.split('.').peekable();
    while let Some(part) = parts.next() {
        if !cur.is_object() {
            *cur = Value::Object(Map::new());
        }
        let map = cur.as_object_mut().expect("just made an object");
        if parts.peek().is_none() {
            map.insert(part.to_owned(), value);
            return;
        }
        cur = map.entry(part).or_insert_with(|| Value::Object(Map::new()));
    }
}

/// Removes `name` and any table it leaves empty. False when it wasn't there.
pub fn unset_path(doc: &mut Value, name: &str) -> bool {
    fn go(v: &mut Value, parts: &[&str]) -> bool {
        let Some(map) = v.as_object_mut() else { return false };
        let removed = match parts {
            [last] => map.remove(*last).is_some(),
            [first, rest @ ..] => map.get_mut(*first).is_some_and(|c| go(c, rest)),
            [] => false,
        };
        if let [first, _, ..] = parts {
            if map.get(*first).is_some_and(|c| c.as_object().is_some_and(Map::is_empty)) {
                map.remove(*first);
            }
        }
        removed
    }
    go(doc, &name.split('.').collect::<Vec<_>>())
}

/// Every setting as the page shows it: its value now, where that comes from and what it can be.
/// Fails (as the daemon would) when the files don't load together.
pub fn show(paths: &Paths) -> Result<Value, ConfigError> {
    let layer = paths.layer_document()?;
    let base = paths.base_document()?;
    // The files must load, or the numbers would be guesses.
    Config::load(paths)?;
    let settings: Vec<Value> = KEYS
        .iter()
        .map(|k| {
            let mine = at(&layer, k.key);
            let below = at(&base, k.key);
            let (value, source) = match (mine, below) {
                (Some(v), _) => (v.clone(), "you"),
                (None, Some(v)) => (v.clone(), "files"),
                (None, None) => (Value::Null, "default"),
            };
            let default = k.default.map(|d| parse_value(k.key, d).expect("table defaults parse"));
            let mut row = json!({
                "key": k.key, "type": k.kind.name(), "group": k.group, "label": k.label, "help": k.help,
                "value": value, "source": source, "default": default,
                "nullable": matches!(k.kind, Kind::OptionalBool),
            });
            let o = row.as_object_mut().expect("a row is an object");
            for (name, v) in [("min", k.min), ("max", k.max)] {
                if let Some(v) = v {
                    o.insert(name.into(), json!(v));
                }
            }
            if !k.unit.is_empty() {
                o.insert("unit".into(), json!(k.unit));
            }
            if !k.choices.is_empty() {
                o.insert("choices".into(), json!(k.choices));
            }
            if k.adds {
                o.insert("inherited".into(), below.cloned().unwrap_or_else(|| json!([])));
                o.insert("value".into(), mine.cloned().unwrap_or_else(|| json!([])));
            }
            row
        })
        .collect();
    let rules = layer.get("rule").and_then(Value::as_array).map_or(0, Vec::len);
    let profiles = layer.get("profile").and_then(Value::as_object).map_or(0, Map::len);
    Ok(json!({
        "ok": true,
        "layer_file": paths.layer(),
        "settings": settings,
        "layer": { "rules": rules, "profiles": profiles, "empty": layer.as_object().is_none_or(Map::is_empty) },
    }))
}

/// The settings layer itself, for the commands that change it.
pub fn layer(paths: &Paths) -> Result<Value, ConfigError> {
    paths.layer_document()
}

/// The settings in `v` as (dotted path, value) leaves. Rules and profiles are counted apart.
fn flatten(prefix: &str, v: &Value, out: &mut Vec<(String, Value)>) {
    match v {
        Value::Object(m) => {
            for (k, c) in m {
                if prefix.is_empty() && (k == "rule" || k == "profile") {
                    continue;
                }
                let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten(&p, c, out);
            }
        }
        _ => out.push((prefix.to_owned(), v.clone())),
    }
}

fn show_value(v: Option<&Value>) -> String {
    match v {
        None => "not set".into(),
        Some(Value::Array(a)) => {
            let items: Vec<String> = a.iter().map(|i| i.as_str().map_or_else(|| i.to_string(), str::to_owned)).collect();
            if items.is_empty() { "(none)".into() } else { items.join(", ") }
        }
        Some(Value::String(s)) if s.is_empty() => "(empty)".into(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

fn show_value_of(v: &Value) -> String {
    show_value(Some(v))
}

/// What saving `new` instead of `old` changes, and whether the files would still load.
pub fn plan(paths: &Paths, old: &Value, new: &Value, summary_of_nothing: &str) -> Plan {
    if let Err(e) = paths.check_layer(new) {
        return Plan::refused("scdr would not save this", e.to_string());
    }
    let base = paths.base_document().ok();
    let (mut before, mut after) = (Vec::new(), Vec::new());
    flatten("", old, &mut before);
    flatten("", new, &mut after);
    let mut changes = Vec::new();
    let named = |key: &str| lookup(key).map_or_else(|| key.to_owned(), |k| k.label.to_owned());
    for (path, was) in &before {
        let now = after.iter().find(|(p, _)| p == path).map(|(_, v)| v);
        if now != Some(was) {
            let group = lookup(path).map_or("Settings", |k| k.group);
            let detail = match now {
                Some(_) => format!("{} → {}", show_value(Some(was)), show_value(now)),
                None => format!("back to the value from scdr's files (was {})", show_value(Some(was))),
            };
            changes.push(Change::new(group, named(path), detail, Some(true)));
        }
    }
    for (path, now) in &after {
        if before.iter().any(|(p, _)| p == path) {
            continue;
        }
        let group = lookup(path).map_or("Settings", |k| k.group);
        let was = base.as_ref().and_then(|b| at(b, path)).map(show_value_of).or_else(|| lookup(path).and_then(|k| k.default).map(str::to_owned));
        let detail = match was {
            Some(was) => format!("{was} → {}", show_value(Some(now))),
            None => format!("now {}", show_value(Some(now))),
        };
        changes.push(Change::new(group, named(path), detail, Some(true)));
    }
    let count = |v: &Value, k: &str| match v.get(k) {
        Some(Value::Array(a)) => a.len(),
        Some(Value::Object(m)) => m.len(),
        _ => 0,
    };
    for (key, noun) in [("rule", "rule"), ("profile", "profile")] {
        let (a, b) = (count(old, key), count(new, key));
        if a != b || (old.get(key) != new.get(key)) {
            let detail = match (a, b) {
                (a, 0) => format!("your {a} {noun}{} go", if a == 1 { "" } else { "s" }),
                (a, b) if a == b => format!("{b} {noun}{} edited", if b == 1 { "" } else { "s" }),
                (a, b) => format!("{a} → {b}"),
            };
            changes.push(Change::new("Rules and profiles", format!("Your {noun}s"), detail, Some(true)));
        }
    }
    if changes.is_empty() {
        return Plan::new(summary_of_nothing, changes);
    }
    let n = changes.len();
    Plan::new(format!("{n} change{}", if n == 1 { "" } else { "s" }), changes)
}

#[cfg(test)]
mod tests;
