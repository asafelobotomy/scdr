//! `scdrctl`'s JSON documents, run against a throwaway config tree: the shapes uQ's Scheduling
//! page reads (`uq/pages/scdr/page.toml`).

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

struct Tree(PathBuf);

impl Tree {
    fn new(name: &str) -> Tree {
        let root = std::env::temp_dir().join(format!("scdr-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["etc", "share", "state"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("share/scdr.toml"), "[profile.desktop]\nnice = 0\n[[rule]]\nprofile = \"desktop\"\nname = \"x\"\n[pressure]\nhigh = 25.0\n").unwrap();
        Tree(root)
    }

    /// Runs `scdrctl args… --json` against this tree: (exit success, the JSON line it printed).
    fn run(&self, args: &[&str]) -> (bool, Value) {
        let out = Command::new(env!("CARGO_BIN_EXE_scdrctl"))
            .args(args)
            .arg("--json")
            .arg("--etc-dir")
            .arg(self.0.join("etc"))
            .arg("--share-dir")
            .arg(self.0.join("share"))
            .arg("--state-dir")
            .arg(self.0.join("state"))
            .output()
            .unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        let doc = serde_json::from_str(text.lines().next().unwrap_or("null")).unwrap_or_else(|e| panic!("{e}: {text:?}"));
        (out.status.success(), doc)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn config_set_previews_then_saves_and_show_follows() {
    let t = Tree::new("set");
    let (ok, plan) = t.run(&["config", "set", "pressure.high", "40", "--dry-run"]);
    assert!(ok);
    assert_eq!(plan["would_refuse"], false);
    assert_eq!(plan["changes"][0]["label"], "Busy above");
    assert!(!t.0.join("state/settings.toml").exists(), "a preview writes nothing");

    let (ok, saved) = t.run(&["config", "set", "pressure.high", "40", "--offline"]);
    assert!(ok && saved["ok"] == true, "{saved}");
    let (_, shown) = t.run(&["config", "show"]);
    let row = shown["settings"].as_array().unwrap().iter().find(|r| r["key"] == "pressure.high").unwrap();
    assert_eq!((row["value"].as_f64(), row["source"].as_str()), (Some(40.0), Some("you")));

    let (ok, reset) = t.run(&["config", "reset", "--offline"]);
    assert!(ok, "{reset}");
    assert!(!t.0.join("state/settings.toml").exists());
}

#[test]
fn config_refusals_are_json_with_a_failing_exit_code() {
    let t = Tree::new("refuse");
    let (ok, doc) = t.run(&["config", "set", "pressure.low", "90", "--dry-run"]);
    assert!(!ok);
    assert_eq!(doc["ok"], false);
    assert!(doc["error"].as_str().unwrap().contains("low < high"), "{doc}");
    let (ok, doc) = t.run(&["config", "set", "profile.desktop.nice", "5", "--dry-run"]);
    assert!(!ok && doc["error"].as_str().unwrap().contains("isn't a setting"), "{doc}");
}

#[test]
fn check_and_list_answer_in_json() {
    let t = Tree::new("check");
    let (ok, doc) = t.run(&["check"]);
    assert!(ok);
    assert_eq!((doc["profiles"].as_u64(), doc["rules"].as_u64()), (Some(1), Some(1)));
    let (ok, doc) = t.run(&["list"]);
    assert!(ok && doc["processes"].is_array());
}
