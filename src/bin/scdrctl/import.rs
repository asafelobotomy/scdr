//! `scdrctl import`: ananicy-cpp and system76-scheduler rules to a rules.d file.

use crate::{ImportFrom, PathArgs};
use scdr::plan::{Change, Plan};
use anyhow::Context;
use std::path::{Path, PathBuf};

/// Writes `text` to `path`, refusing to replace an existing file unless `force`.
fn write_new(path: &Path, text: &str, force: bool) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true);
    if force {
        opts.create(true).truncate(true);
    } else {
        opts.create_new(true);
    }
    opts.open(path)?.write_all(text.as_bytes())
}

pub struct Mode {
    pub dry_run: bool,
    pub json: bool,
}

pub fn import(from: ImportFrom, output: Option<PathBuf>, force: bool, paths: &PathArgs, mode: Mode) -> anyhow::Result<()> {
    // Imported rules go first, so by default skip names our rules already cover.
    let current = paths.load().ok();
    let imp = match from {
        ImportFrom::Ananicy { dir, only, keep_overlaps } => {
            let covered = |name: &str| {
                !keep_overlaps
                    && current
                        .as_ref()
                        .is_some_and(|c| c.rules.find(&scdr::import::synthetic(name)).is_some())
            };
            scdr::import::ananicy::import(&dir, &only, &covered)?
        }
        ImportFrom::System76 { files } => scdr::import::system76::import(&files)?,
    };
    let files = current.map(|c| c.files).unwrap_or_default();
    imp.validate(&files)
        .map_err(|e| anyhow::anyhow!("the converted rules don't load: {e}"))?;

    if mode.dry_run {
        let plan = plan(&imp, output.as_deref(), force);
        if mode.json {
            plan.print();
        } else {
            plan.print_text();
        }
        return Ok(());
    }
    let toml = imp.to_toml();
    match &output {
        Some(path) if !force && path.exists() => {
            anyhow::bail!("{} already exists; pass --force to overwrite it", path.display())
        }
        Some(path) => write_new(path, &toml, force).with_context(|| format!("writing {}", path.display()))?,
        None => print!("{toml}"),
    }
    if mode.json {
        let written = output.as_ref().map(|p| p.display().to_string());
        println!(
            "{}",
            serde_json::json!({ "ok": true, "profiles": imp.profile_count(), "rules": imp.rule_count(), "patterns": imp.pattern_count(), "skipped": imp.skipped.len(), "written": written })
        );
        return Ok(());
    }
    eprintln!(
        "imported {} profiles, {} rules ({} patterns){}",
        imp.profile_count(),
        imp.rule_count(),
        imp.pattern_count(),
        output.map(|p| format!(" into {}", p.display())).unwrap_or_default()
    );
    if !imp.skipped.is_empty() {
        eprintln!("not imported ({}):", imp.skipped.len());
        for s in imp.skipped.iter().take(12) {
            eprintln!("  {s}");
        }
        if imp.skipped.len() > 12 {
            eprintln!("  … and {} more", imp.skipped.len() - 12);
        }
    }
    Ok(())
}

/// What the import would write, and what it would leave out.
fn plan(imp: &scdr::import::Import, output: Option<&Path>, force: bool) -> Plan {
    let plural = |n: usize, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
    let summary = format!("{}, {}", plural(imp.profile_count(), "profile"), plural(imp.rule_count(), "rule"));
    let mut changes = vec![
        Change::new("Added", plural(imp.profile_count(), "profile"), "new profiles in the rules file", Some(true)),
        Change::new("Added", plural(imp.rule_count(), "rule"), plural(imp.pattern_count(), "name pattern"), Some(true)),
    ];
    changes.extend(imp.skipped.iter().map(|s| Change::new("Left out", s.clone(), "", None)));
    if let Some(path) = output.filter(|p| p.exists() && !force) {
        return Plan::refused(summary, format!("{} already exists. Pass --force to replace it.", path.display()));
    }
    Plan::new(summary, changes)
}
