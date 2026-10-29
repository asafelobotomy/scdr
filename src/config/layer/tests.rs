use super::super::Paths;
use serde_json::json;

/// A throwaway share/etc/state tree under the target directory.
fn tree(name: &str, share: &str, etc: Option<&str>) -> Paths {
    let root = std::env::temp_dir().join(format!("scdr-layer-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths { etc: root.join("etc"), share: root.join("share"), state: root.join("state") };
    for d in [&paths.etc, &paths.share, &paths.state] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(paths.share.join("scdr.toml"), share).unwrap();
    if let Some(etc) = etc {
        std::fs::write(paths.etc.join("scdr.toml"), etc).unwrap();
    }
    paths
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
    assert_eq!(doc["profile"]["fg"]["nice"], json!(-4), "whole profile replaced");
    assert!(doc["profile"]["fg"]["_source"].as_str().unwrap().ends_with("etc/scdr.toml"));
    let names: Vec<_> = doc["rule"].as_array().unwrap().iter().map(|r| r["name"].clone()).collect();
    assert_eq!(names, vec![json!("b"), json!("a")], "later files' rules first");
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
    assert!(text.contains("hold_secs = 10\n") && !text.contains("_source"), "{text}");
    assert_eq!(paths.layer_document().unwrap()["pressure"], json!({"hold_secs": 10}));
    assert_eq!(paths.files().last(), Some(&paths.layer()));
    assert!(!paths.lower_files().contains(&paths.layer()));

    let bad = json!({"rule": [{"profile": "missing", "name": "x"}]});
    assert!(paths.write_layer(&bad).is_err());
    assert!(std::fs::read_to_string(paths.layer()).unwrap().contains("hold_secs"), "kept on error");

    paths.write_layer(&json!({"pressure": {}})).unwrap();
    assert!(!paths.layer().exists());
}
