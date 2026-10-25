//! ananicy-cpp rules: `*.types` and `*.rules`, one JSON object per line,
//! found recursively (e.g. /etc/ananicy.d or a CachyOS ananicy-rules
//! checkout). Types become `ananicy-<type>` profiles; rules that only name a
//! type are grouped into one rule per type; rules with their own settings
//! get a profile inheriting their type. Later definitions win, as in ananicy.

use super::{Import, Value, glob_literal, quote};
use serde_json::{Map, Value as Json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// `only`: import just these types (e.g. `Game`); empty imports everything.
pub fn import(dir: &Path, only: &[String], covered: &dyn Fn(&str) -> bool) -> anyhow::Result<Import> {
    let mut files = Vec::new();
    collect(dir, &mut files)?;
    files.sort();
    if files.is_empty() {
        anyhow::bail!("no .types or .rules files under {}", dir.display());
    }

    let mut imp = Import::new(format!("ananicy rules in {}", dir.display()));
    let mut types: BTreeMap<String, Map<String, Json>> = BTreeMap::new();
    // name -> (type, own settings), last definition wins
    let mut rules: BTreeMap<String, (Option<String>, Map<String, Json>)> = BTreeMap::new();
    for path in &files {
        let is_types = path.extension().is_some_and(|e| e == "types");
        for (n, line) in std::fs::read_to_string(path)?.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Ok(Json::Object(mut obj)) = serde_json::from_str::<Json>(line) else {
                imp.skipped.push(format!("{}:{}: not a JSON object", path.display(), n + 1));
                continue;
            };
            let ty = obj.remove("type").and_then(|t| t.as_str().map(str::to_owned));
            if is_types {
                if let Some(ty) = ty {
                    types.insert(ty, obj);
                }
            } else if let Some(Json::String(name)) = obj.remove("name") {
                rules.insert(name, (ty, obj));
            }
        }
    }

    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut overlaps = 0;
    let mut filtered = 0;
    for (name, (ty, own)) in &rules {
        if !only.is_empty() && !ty.as_ref().is_some_and(|t| only.contains(t)) {
            filtered += 1;
            continue;
        }
        if covered(name) {
            overlaps += 1;
            continue;
        }
        if let Some(t) = ty.as_deref().filter(|t| !types.contains_key(*t)) {
            imp.skipped.push(format!("{name}: unknown type {t:?}"));
            continue;
        }
        let parent = ty.as_deref().map(|t| ensure_type(&mut imp, t, &types[t]));
        let fields = convert(own, &mut imp.skipped, name);
        match (parent, fields.is_empty()) {
            (Some(parent), true) => grouped.entry(parent).or_default().push(glob_literal(name)),
            (parent, false) => {
                let profile = format!("ananicy-{}", slug(name));
                let mut fields = fields;
                if let Some(p) = parent {
                    fields.insert(0, ("inherit".into(), quote(&p)));
                }
                imp.profile(&profile, fields);
                imp.rule(&profile, vec![("name".into(), vec![glob_literal(name)])]);
            }
            (None, true) => imp.skipped.push(format!("{name}: no type and no settings")),
        }
    }
    for (profile, names) in grouped {
        imp.rule(&profile, vec![("name".into(), names)]);
    }
    if filtered > 0 {
        imp.skipped.push(format!("{filtered} rules of other types (--only {})", only.join(",")));
    }
    if overlaps > 0 {
        imp.skipped.push(format!("{overlaps} names already matched by your scdr rules (kept yours)"));
    }
    Ok(imp)
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "types" || e == "rules") {
            out.push(path);
        }
    }
    Ok(())
}

fn ensure_type(imp: &mut Import, ty: &str, settings: &Map<String, Json>) -> String {
    let name = format!("ananicy-{}", slug(ty));
    if !imp.has_profile(&name) {
        let mut skipped = Vec::new();
        let fields = convert(settings, &mut skipped, ty);
        imp.skipped.extend(skipped);
        imp.profile(&name, fields);
    }
    name
}

/// ananicy settings -> scdr profile fields.
fn convert(obj: &Map<String, Json>, skipped: &mut Vec<String>, what: &str) -> Vec<(String, Value)> {
    let mut f = Vec::new();
    let int = |k: &str| obj.get(k).and_then(Json::as_i64);
    if let Some(n) = int("nice").filter(|n| (-20..=19).contains(n)) {
        f.push(("nice".into(), n.to_string()));
    }
    match obj.get("sched").and_then(Json::as_str) {
        Some("normal" | "other") => f.push(("sched".into(), quote("other"))),
        Some(s @ ("batch" | "idle")) => f.push(("sched".into(), quote(s))),
        Some(s @ ("rr" | "fifo")) => match int("rtprio").filter(|p| (1..=99).contains(p)) {
            Some(p) => f.push(("sched".into(), quote(&format!("{s}:{p}")))),
            None => skipped.push(format!("{what}: sched={s} without rtprio")),
        },
        Some(s) => skipped.push(format!("{what}: unsupported sched={s}")),
        None => {}
    }
    let level = int("ionice").filter(|l| (0..=7).contains(l));
    let io = match obj.get("ioclass").and_then(Json::as_str) {
        Some("idle") => Some("idle".to_owned()),
        Some("best-effort") => Some(format!("be:{}", level.unwrap_or(4))),
        Some("realtime") => Some(format!("rt:{}", level.unwrap_or(4))),
        Some(_) => None,
        None => level.map(|l| format!("be:{l}")),
    };
    if let Some(io) = io {
        f.push(("io".into(), quote(&io)));
    }
    if let Some(o) = int("oom_score_adj").filter(|o| (-1000..=1000).contains(o)) {
        f.push(("oom_score_adj".into(), o.to_string()));
    }
    for key in ["latency_nice", "cgroup", "cpuset", "name_regex"] {
        if obj.contains_key(key) {
            skipped.push(format!("{what}: {key} isn't supported, ignored"));
        }
    }
    f
}

fn slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_types_rules_and_overrides() {
        let dir = std::env::temp_dir().join(format!("scdr-ananicy-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Games")).unwrap();
        std::fs::write(
            dir.join("00-types.types"),
            "# comment\n{ \"type\": \"Game\", \"nice\": -5, \"ioclass\": \"best-effort\", \"sched\": \"normal\" }\n\
             { \"type\": \"BG_CPUIO\", \"nice\": 16, \"ioclass\": \"idle\", \"sched\": \"idle\", \"latency_nice\": 11 }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("Games/games.rules"),
            "{ \"name\": \"doom.exe\", \"type\": \"Game\" }\n{ \"name\": \"q*3\", \"type\": \"Game\" }\n\
             { \"name\": \"makepkg\", \"type\": \"BG_CPUIO\", \"ionice\": 6 }\n\
             { \"name\": \"pacman\", \"type\": \"BG_CPUIO\" }\n{ \"name\": \"x\", \"type\": \"Nope\" }\nnot json\n",
        )
        .unwrap();

        let only_games = import(&dir, &["Game".into()], &|_| false).unwrap();
        assert_eq!(only_games.pattern_count(), 2);
        let imp = import(&dir, &[], &|n| n == "pacman").unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let toml = imp.to_toml();
        let c = imp.validate(&[]).unwrap();

        let game = &c.profiles["ananicy-game"].profile;
        assert_eq!(game.nice, Some(-5));
        assert_eq!(game.io.unwrap().to_string(), "be:4");
        let mk = &c.profiles["ananicy-makepkg"];
        assert_eq!(mk.chain, ["ananicy-bg-cpuio", "ananicy-makepkg"]);
        assert_eq!(mk.profile.io.unwrap().to_string(), "be:6");

        let find = |n: &str| c.rules.find(&super::super::synthetic(n)).map(|r| r.profile.clone());
        assert_eq!(find("doom.exe").as_deref(), Some("ananicy-game"));
        assert_eq!(find("q*3").as_deref(), Some("ananicy-game"));
        assert_eq!(find("qux3"), None, "the * in q*3 is literal");
        assert_eq!(find("makepkg").as_deref(), Some("ananicy-makepkg"));
        assert_eq!(find("pacman"), None, "covered by the existing config");

        let skipped = imp.skipped.join("\n");
        for expect in ["unknown type \"Nope\"", "not a JSON object", "latency_nice", "1 names already matched"] {
            assert!(skipped.contains(expect), "{expect} missing from:\n{skipped}\n---\n{toml}");
        }
    }
}
