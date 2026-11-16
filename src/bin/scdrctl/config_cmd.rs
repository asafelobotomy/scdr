//! `scdrctl config`: the settings the widget's Preferences tab shows, from the command line.
//! Changes go through scdrd (polkit asks for the password), like every other write; `--offline`
//! writes the settings file directly, for root and for tests.

use crate::{PathArgs, daemon, finish, not_running};
use anyhow::{Context, bail};
use clap::{Args, Subcommand};
use scdr::config::edit;
use serde_json::{Value, json};

#[derive(Subcommand)]
pub(crate) enum ConfigCmd {
    /// Every setting: its value now, where it comes from, what it can be.
    Show {
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        paths: PathArgs,
    },
    /// Where the settings file is, and the files below it.
    Path {
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        paths: PathArgs,
    },
    /// Change one setting (a dotted key such as `pressure.high`; lists are comma separated).
    Set {
        key: String,
        value: String,
        #[command(flatten)]
        how: Change,
    },
    /// Drop one setting made here: the value from scdr's files applies again.
    Unset {
        key: String,
        #[command(flatten)]
        how: Change,
    },
    /// Drop every setting, rule and profile made in the widget.
    Reset {
        #[command(flatten)]
        how: Change,
    },
}

#[derive(Args)]
pub(crate) struct Change {
    /// Print what would change and save nothing.
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json: bool,
    /// Write the settings file directly instead of asking scdrd (root).
    #[arg(long)]
    offline: bool,
    #[command(flatten)]
    paths: PathArgs,
}

pub(crate) async fn run(cmd: ConfigCmd) -> anyhow::Result<()> {
    match cmd {
        ConfigCmd::Show { json, paths } => finish(json, show(json, &paths)),
        ConfigCmd::Path { json, paths } => {
            let p = paths.paths();
            let files: Vec<_> = p.files().iter().map(|f| f.display().to_string()).collect();
            if json {
                println!("{}", json!({ "ok": true, "layer_file": p.layer(), "files": files }));
            } else {
                println!("{}", p.layer().display());
                files.iter().for_each(|f| println!("  {f}"));
            }
            Ok(())
        }
        ConfigCmd::Set { key, value, how } => {
            let json = how.json;
            let result = async {
                let value = edit::parse_value(&key, &value).map_err(anyhow::Error::msg)?;
                change(&how, |doc| {
                    edit::set_path(doc, &key, value.clone());
                    Ok(())
                })
                .await
            };
            finish(json, result.await)
        }
        ConfigCmd::Unset { key, how } => {
            let json = how.json;
            let result = change(&how, |doc| {
                if !edit::unset_path(doc, &key) {
                    bail!("{key} isn't set here, so there is nothing to drop");
                }
                Ok(())
            });
            finish(json, result.await)
        }
        ConfigCmd::Reset { how } => {
            let json = how.json;
            finish(json, change(&how, |doc| {
                *doc = json!({});
                Ok(())
            }).await)
        }
    }
}

fn show(json: bool, paths: &PathArgs) -> anyhow::Result<()> {
    let doc = edit::show(&paths.paths())?;
    if json {
        println!("{doc}");
        return Ok(());
    }
    let mut group = "";
    for s in doc["settings"].as_array().into_iter().flatten() {
        let g = s["group"].as_str().unwrap_or_default();
        if g != group {
            println!("{g}:");
            group = g;
        }
        let value = match &s["value"] {
            Value::Null => "-".to_owned(),
            Value::String(t) => t.clone(),
            other => other.to_string(),
        };
        println!("  {:<24} {value}  ({})", s["key"].as_str().unwrap_or_default(), s["source"].as_str().unwrap_or_default());
    }
    Ok(())
}

/// Edits the settings layer with `edit`, checks it against the other files, and saves it (or, with
/// `--dry-run`, only says what would change).
async fn change(how: &Change, edit_doc: impl FnOnce(&mut Value) -> anyhow::Result<()>) -> anyhow::Result<()> {
    let paths = how.paths.paths();
    let old = edit::layer(&paths)?;
    let mut new = old.clone();
    edit_doc(&mut new)?;
    let plan = edit::plan(&paths, &old, &new, "nothing to change");
    if plan.would_refuse {
        bail!("{}", plan.reason);
    }
    if how.dry_run {
        if how.json {
            plan.print();
        } else {
            plan.print_text();
        }
        return Ok(());
    }
    let saved = if how.offline {
        let config = paths.write_layer(&new).context("saving the settings")?;
        format!("{} profiles, {} rules", config.profiles.len(), config.rules.len())
    } else {
        daemon().await?.set_settings(&new.to_string()).await.map_err(not_running)?
    };
    if how.json {
        println!("{}", json!({ "ok": true, "summary": plan.summary, "saved": saved, "changes": plan.changes }));
    } else {
        println!("{} ({saved})", plan.summary);
    }
    Ok(())
}
