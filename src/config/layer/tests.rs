use super::super::Paths;
use serde_json::json;

/// A throwaway share/etc/state tree in the temp directory, removed when
/// the test ends (pass or fail).
struct Tree {
    root: std::path::PathBuf,
    paths: Paths,
}

impl std::ops::Deref for Tree {
    type Target = Paths;
    fn deref(&self) -> &Paths {
        &self.paths
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn tree(name: &str, share: &str, etc: Option<&str>) -> Tree {
    let root = std::env::temp_dir().join(format!("scdr-layer-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths {
        etc: root.join("etc"),
        share: root.join("share"),
        state: root.join("state"),
    };
    for d in [&paths.etc, &paths.share, &paths.state] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(paths.share.join("scdr.toml"), share).unwrap();
    if let Some(etc) = etc {
        std::fs::write(paths.etc.join("scdr.toml"), etc).unwrap();
    }
    Tree { root, paths }
}

const SHARE: &str = r#"
[pressure]
high = 20.0
[profile.base]
nice = 5
[profile.fg]
nice = -2
[[rule]]
profile = "base"
name = "a"
[roles]
foreground = "fg"
"#;

#[test]
fn base_document_layers_like_the_loader() {
    let etc = "[pressure]\nlow = 2.0\n[profile.fg]\nnice = -4\n[[rule]]\nprofile = \"fg\"\nname = \"b\"\n";
    let paths = tree("base", SHARE, Some(etc));
    let doc = paths.base_document().unwrap();
    assert_eq!(doc["pressure"], json!({"high": 20.0, "low": 2.0}));
    assert_eq!(
        doc["profile"]["fg"]["nice"],
        json!(-4),
        "whole profile replaced"
    );
    assert!(
        doc["profile"]["fg"]["_source"]
            .as_str()
            .unwrap()
            .ends_with("etc/scdr.toml")
    );
    let names: Vec<_> = doc["rule"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].clone())
        .collect();
    assert_eq!(
        names,
        vec![json!("b"), json!("a")],
        "later files' rules first"
    );
}

#[test]
fn writes_validated_layer_and_removes_it_when_empty() {
    let paths = tree("write", SHARE, None);
    assert_eq!(paths.layer_document().unwrap(), json!({}));

    let doc = json!({
        "pressure": {"hold_secs": 10.0, "high": null},
        "roles": {"power": {"power-saver": {"foreground": ""}}},
        "rule": [{"profile": "fg", "name": ["x*"], "_source": "ignored"}],
    });
    let config = paths.write_layer(&doc).unwrap();
    assert_eq!(config.pressure.hold_secs, 10);
    assert_eq!(config.rules.len(), 2);
    let text = std::fs::read_to_string(paths.layer()).unwrap();
    assert!(text.starts_with("# Written by scdrd"));
    assert!(
        text.contains("hold_secs = 10\n") && !text.contains("_source"),
        "{text}"
    );
    assert_eq!(
        paths.layer_document().unwrap()["pressure"],
        json!({"hold_secs": 10})
    );
    assert_eq!(paths.files().last(), Some(&paths.layer()));
    assert!(!paths.lower_files().contains(&paths.layer()));

    let bad = json!({"rule": [{"profile": "missing", "name": "x"}]});
    assert!(paths.write_layer(&bad).is_err());
    assert!(
        std::fs::read_to_string(paths.layer())
            .unwrap()
            .contains("hold_secs"),
        "kept on error"
    );

    paths.write_layer(&json!({"pressure": {}})).unwrap();
    assert!(!paths.layer().exists());
}

#[test]
fn rejects_oversized_documents() {
    let paths = tree("big", SHARE, None);
    let big = json!({"exceptions": {"name": vec!["x".repeat(1000); 300]}});
    let e = paths.write_layer(&big).unwrap_err().to_string();
    assert!(e.contains("limit"), "{e}");
    assert!(!paths.layer().exists());
}

#[test]
fn strips_only_the_widgets_source_annotation() {
    let paths = tree("meta", SHARE, None);
    let ok = json!({
        "profile": {"p": {"nice": 3, "_source": "/etc/x.toml"}},
        "rule": [{"profile": "p", "name": "x", "_source": "/etc/x.toml"}],
    });
    paths.write_layer(&ok).unwrap();
    let text = std::fs::read_to_string(paths.layer()).unwrap();
    assert!(!text.contains("_source"), "{text}");
    // Other underscore keys are no longer dropped silently; they reach the validator.
    let other = json!({"rule": [{"profile": "fg", "name": "x", "_other": 1}]});
    assert!(paths.write_layer(&other).is_err());
    let nested = json!({"exceptions": {"_source": ["x"]}});
    assert!(paths.write_layer(&nested).is_err());
    let dir = paths.layer().parent().unwrap().to_owned();
    let stray = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    assert_eq!(stray, 0);
}
