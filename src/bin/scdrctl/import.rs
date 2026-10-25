//! `scdrctl import`: ananicy-cpp and system76-scheduler rules to a rules.d file.

use crate::{ImportFrom, PathArgs};
use anyhow::Context;
use std::path::PathBuf;

pub fn import(from: ImportFrom, output: Option<PathBuf>, paths: &PathArgs) -> anyhow::Result<()> {
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

    let toml = imp.to_toml();
    match &output {
        Some(path) => std::fs::write(path, &toml).with_context(|| format!("writing {}", path.display()))?,
        None => print!("{toml}"),
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
