use super::*;
use crate::config::Paths;

struct Tree {
    root: std::path::PathBuf,
    paths: Paths,
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn tree(name: &str, share: &str) -> Tree {
    let root = std::env::temp_dir().join(format!("scdr-edit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths { etc: root.join("etc"), share: root.join("share"), state: root.join("state") };
    for d in [&paths.etc, &paths.share, &paths.state] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(paths.share.join("scdr.toml"), share).unwrap();
    Tree { root, paths }
}

const BASE: &str = "[profile.desktop]\nnice = 0\n[[rule]]\nprofile = \"desktop\"\nname = \"x\"\n[pressure]\nhigh = 25.0\n[exceptions]\nname = [\"a\"]\n";

#[test]
fn values_are_read_by_the_keys_type() {
    assert_eq!(parse_value("pressure.high", "30").unwrap(), json!(30.0));
    assert_eq!(parse_value("system.iocost", "on").unwrap(), json!(true));
    assert_eq!(parse_value("system.rescan_secs", "60").unwrap(), json!(60));
    assert_eq!(parse_value("split.apps", "a, b,,c").unwrap(), json!(["a", "b", "c"]));
    assert_eq!(parse_value("split.apps", "").unwrap(), json!([]));
    assert_eq!(parse_value("roles.game", "game").unwrap(), json!("game"));
    assert_eq!(parse_value("eevdf.power.balanced.base_slice_us", "3000").unwrap(), json!(3000));
    assert!(parse_value("system.rescan_secs", "soon").is_err());
    assert!(parse_value("system.iocost", "maybe").is_err());
    assert!(parse_value("pressure.high", "NaN").is_err());
    assert!(parse_value("profile.desktop.nice", "1").unwrap_err().contains("isn't a setting"));
}

#[test]
fn set_and_unset_prune_empty_tables() {
    let mut doc = json!({});
    set_path(&mut doc, "eevdf.power.balanced.preempt", json!("lazy"));
    assert_eq!(doc, json!({"eevdf": {"power": {"balanced": {"preempt": "lazy"}}}}));
    assert!(unset_path(&mut doc, "eevdf.power.balanced.preempt"));
    assert_eq!(doc, json!({}));
    assert!(!unset_path(&mut doc, "eevdf.preempt"));
}

#[test]
fn show_says_where_each_value_comes_from() {
    let t = tree("show", BASE);
    std::fs::write(t.paths.layer(), "[pressure]\nlow = 2.0\n").unwrap();
    let doc = show(&t.paths).unwrap();
    let row = |k: &str| doc["settings"].as_array().unwrap().iter().find(|r| r["key"] == k).unwrap().clone();
    assert_eq!(row("pressure.low")["source"], "you");
    assert_eq!(row("pressure.low")["value"], json!(2.0));
    assert_eq!(row("pressure.high")["source"], "files");
    assert_eq!(row("pressure.hold_secs")["source"], "default");
    assert_eq!(row("pressure.hold_secs")["default"], json!(30));
    assert_eq!(row("exceptions.name")["inherited"], json!(["a"]));
    assert_eq!(row("exceptions.name")["value"], json!([]));
    assert_eq!(doc["layer"]["empty"], false);
}

#[test]
fn show_fails_when_the_files_do_not_load() {
    let t = tree("broken", "[pressure]\nhigh = 1.0\nlow = 9.0\n");
    assert!(show(&t.paths).is_err());
}

#[test]
fn a_plan_lists_what_changes_and_refuses_what_would_not_load() {
    let t = tree("plan", BASE);
    let old = json!({});
    let mut new = old.clone();
    set_path(&mut new, "pressure.high", json!(40.0));
    let p = plan(&t.paths, &old, &new, "nothing");
    assert!(!p.would_refuse);
    assert_eq!(p.changes.len(), 1);
    assert_eq!(p.changes[0].label, "Busy above");
    assert_eq!(p.changes[0].detail, "25.0 → 40.0");

    let mut bad = old.clone();
    set_path(&mut bad, "pressure.low", json!(90.0));
    let p = plan(&t.paths, &old, &bad, "nothing");
    assert!(p.would_refuse);
    assert!(p.reason.contains("low < high"), "{}", p.reason);

    assert_eq!(plan(&t.paths, &old, &old, "nothing").summary, "nothing");
}

#[test]
fn a_plan_for_a_reset_counts_rules_and_profiles() {
    let t = tree("reset", BASE);
    let old = json!({"pressure": {"low": 2.0}, "rule": [{"profile": "desktop", "name": "y"}], "profile": {"mine": {"nice": 1}}});
    let p = plan(&t.paths, &old, &json!({}), "nothing");
    let labels: Vec<_> = p.changes.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["Calm below", "Your rules", "Your profiles"]);
    assert_eq!(p.changes[1].detail, "your 1 rule go");
}
