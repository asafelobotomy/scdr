//! system76-scheduler KDL (config.kdl and process-scheduler/*.kdl). Each
//! assignment becomes an `s76-<name>` profile; a later file's assignment of
//! the same name adds to it, as in system76-scheduler.
//!
//! - `"/usr/bin/foo"` matches by executable, a bare word by name
//! - `include`/`*` with `cgroup=`, `parent=`, `descends=`, `name=` become
//!   rules (`descends` maps to `ancestor`); a `!` prefix negates
//! - `exclude` conditions are added as `not_*` to every rule of the profile
//! - exceptions become `[exceptions]`; `include descends=` become launchers

use super::{Import, Value, glob_literal, quote};
use kdl::{KdlDocument, KdlEntry, KdlNode};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// system76 profiles scdr covers with roles instead.
const ROLE_PROFILES: [&str; 3] = ["foreground", "background", "pipewire"];

#[derive(Default)]
struct Profile {
    fields: BTreeMap<String, Value>,
    rules: Vec<Vec<(String, Vec<String>)>>,
    excludes: Vec<(String, Vec<String>)>,
}

pub fn import(paths: &[PathBuf]) -> anyhow::Result<Import> {
    let names: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
    let mut imp = Import::new(format!("system76-scheduler {}", names.join(", ")));
    let mut profiles: BTreeMap<String, Profile> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();

    for path in paths {
        let text = std::fs::read_to_string(path)?;
        let doc = KdlDocument::parse_v1(&text)
            .or_else(|_| KdlDocument::parse(&text))
            .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        for node in find(&doc, "assignments") {
            for a in node.children().map(KdlDocument::nodes).unwrap_or_default() {
                let name = a.name().value().to_owned();
                if ROLE_PROFILES.contains(&name.as_str()) {
                    imp.skipped.push(format!("{name}: scdr covers this with [roles]"));
                    continue;
                }
                if !profiles.contains_key(&name) {
                    order.push(name.clone());
                }
                let p = profiles.entry(name.clone()).or_default();
                p.fields.extend(settings(a.entries(), &mut imp.skipped, &name));
                for child in a.children().map(KdlDocument::nodes).unwrap_or_default() {
                    add_condition(p, child, &mut imp.skipped, &name);
                }
            }
        }
        for node in find(&doc, "exceptions") {
            for e in node.children().map(KdlDocument::nodes).unwrap_or_default() {
                match e.name().value() {
                    "include" => match prop(e, "descends") {
                        Some(d) => imp.launcher(&d),
                        None => imp.skipped.push("exceptions: include without descends=".into()),
                    },
                    name => imp.exception(name),
                }
            }
        }
    }

    for name in order {
        let p = profiles.remove(&name).unwrap();
        if p.rules.is_empty() {
            imp.skipped.push(format!("{name}: no processes assigned"));
            continue;
        }
        let profile = format!("s76-{name}");
        imp.profile(&profile, p.fields.into_iter().collect());
        for mut rule in p.rules {
            for (key, values) in &p.excludes {
                rule.push((key.clone(), values.clone()));
            }
            imp.rule(&profile, rule);
        }
    }
    Ok(imp)
}

fn find<'a>(doc: &'a KdlDocument, name: &str) -> Vec<&'a KdlNode> {
    let mut out = Vec::new();
    for node in doc.nodes() {
        if node.name().value() == name {
            out.push(node);
        } else if let Some(children) = node.children() {
            out.extend(find(children, name));
        }
    }
    out
}

fn prop(node: &KdlNode, key: &str) -> Option<String> {
    node.entries()
        .iter()
        .find(|e| e.name().is_some_and(|n| n.value() == key))
        .and_then(|e| e.value().as_string().map(str::to_owned))
}

fn add_condition(p: &mut Profile, child: &KdlNode, skipped: &mut Vec<String>, profile: &str) {
    let name = child.name().value();
    if child.entries().iter().any(|e| ["nice", "sched", "io"].contains(&e.name().map_or("", |n| n.value()))) {
        skipped.push(format!("{profile}: per-process settings on {name:?} ignored; the profile's apply"));
    }
    let keys = [("cgroup", "cgroup"), ("parent", "parent"), ("descends", "ancestor"), ("name", "name")];
    match name {
        "include" | "*" | "exclude" => {
            let mut include = Vec::new();
            for (s76, ours) in keys {
                let Some(v) = prop(child, s76) else { continue };
                let (negated, pattern) = match v.strip_prefix('!') {
                    Some(rest) => (true, rest.to_owned()),
                    None => (false, v),
                };
                if name == "exclude" || negated {
                    p.excludes.push((format!("not_{ours}"), vec![pattern]));
                } else {
                    include.push((ours.to_owned(), vec![pattern]));
                }
            }
            if !include.is_empty() {
                p.rules.push(include);
            } else if name != "exclude" && p.excludes.is_empty() {
                skipped.push(format!("{profile}: {name} without conditions"));
            }
        }
        path if path.starts_with('/') => p.rules.push(vec![("exe".into(), vec![glob_literal(path)])]),
        bare => p.rules.push(vec![("name".into(), vec![glob_literal(bare)])]),
    }
}

/// `nice=-5 sched=(fifo)10 io=(best-effort)0` -> profile fields.
fn settings(entries: &[KdlEntry], skipped: &mut Vec<String>, what: &str) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for e in entries {
        let Some(key) = e.name().map(|n| n.value()) else { continue };
        let ty = e.ty().map(|t| t.value());
        let v = e.value();
        let field = match (key, ty) {
            ("nice", _) => v.as_integer().filter(|n| (-20..=19).contains(n)).map(|n| ("nice", n.to_string())),
            ("sched", None) => match v.as_string() {
                Some(s @ ("batch" | "idle" | "other")) => Some(("sched", quote(s))),
                _ => None,
            },
            ("sched", Some(t @ ("fifo" | "rr"))) => v
                .as_integer()
                .filter(|p| (1..=99).contains(p))
                .map(|p| ("sched", quote(&format!("{t}:{p}")))),
            ("io", None) if v.as_string() == Some("idle") => Some(("io", quote("idle"))),
            ("io", Some(t @ ("best-effort" | "realtime"))) => v.as_integer().filter(|l| (0..=7).contains(l)).map(|l| {
                let class = if t == "realtime" { "rt" } else { "be" };
                ("io", quote(&format!("{class}:{l}")))
            }),
            _ => None,
        };
        match field {
            Some((k, val)) => out.push((k.to_owned(), val)),
            None => skipped.push(format!("{what}: can't convert {e}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_assignments_and_exceptions() {
        let path = std::env::temp_dir().join(format!("scdr-s76-{}.kdl", std::process::id()));
        std::fs::write(
            &path,
            r#"
            process-scheduler enable=true {
                assignments {
                    sound-server nice=-15 io=(realtime)0 {
                        "/usr/bin/pipewire"
                        jackd
                    }
                    games nice=-5 sched=(rr)5 io=(best-effort)0 {
                        include descends="steam"
                        exclude name="steamwebhelper"
                    }
                    system-services nice=12 io="idle" {
                        include cgroup="/system.slice/*"
                    }
                    foreground nice=0 io=(best-effort)0
                }
                exceptions {
                    include descends="gamemoderun"
                    dbus
                }
            }
            "#,
        )
        .unwrap();
        let imp = import(std::slice::from_ref(&path)).unwrap();
        std::fs::remove_file(&path).unwrap();
        let c = imp.validate(&[]).unwrap();

        let snd = &c.profiles["s76-sound-server"].profile;
        assert_eq!((snd.nice, snd.io.unwrap().to_string()), (Some(-15), "rt:0".into()));
        assert_eq!(c.profiles["s76-games"].profile.sched.unwrap().to_string(), "rr:5");

        let mut pw = crate::import::synthetic("pipewire");
        pw.exe = Some("/usr/bin/pipewire".into());
        assert_eq!(c.rules.find(&pw).unwrap().profile, "s76-sound-server");
        let conds: Vec<&str> = c.rules.iter().map(|r| r.conditions.as_str()).collect();
        assert!(conds.iter().any(|c| c.contains("ancestor=[\"steam\"]") && c.contains("not_name=[\"steamwebhelper\"]")), "{conds:?}");

        let toml = imp.to_toml();
        assert!(toml.contains("launchers = [\"gamemoderun\"]"), "{toml}");
        assert!(imp.skipped.iter().any(|s| s.contains("foreground")), "{:?}", imp.skipped);
    }
}
