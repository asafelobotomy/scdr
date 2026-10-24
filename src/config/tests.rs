use super::*;
use crate::sched::{IoPrio, Policy};

fn cfg(files: &[(&str, &str)]) -> Result<Config, ConfigError> {
    Config::from_sources(
        files
            .iter()
            .map(|(p, t)| (PathBuf::from(p), t.to_string()))
            .collect(),
    )
}

#[test]
fn inheritance_overlays_fields() {
    let c = cfg(&[(
        "a.toml",
        r#"
        [profile.base]
        nice = 5
        io = "be:6"
        [profile.child]
        inherit = "base"
        nice = 10
        sched = "batch"
        "#,
    )])
    .unwrap();
    let child = &c.profiles["child"];
    assert_eq!(child.chain, ["base", "child"]);
    assert_eq!(child.profile.nice, Some(10));
    assert_eq!(child.profile.io, Some(IoPrio::BestEffort(6)));
    assert_eq!(child.profile.sched, Some(Policy::Batch));
}

#[test]
fn cycles_and_unknown_parents_are_errors() {
    let e = cfg(&[(
        "a.toml",
        "[profile.x]\ninherit = \"y\"\n[profile.y]\ninherit = \"x\"\n",
    )])
    .unwrap_err();
    assert!(e.to_string().contains("cycle"), "{e}");

    let e = cfg(&[("a.toml", "[profile.x]\ninherit = \"nope\"\n")]).unwrap_err();
    assert!(e.to_string().contains("unknown profile"), "{e}");
}

#[test]
fn validation_reports_file() {
    let e = cfg(&[("bad.toml", "[profile.x]\nnice = 40\n")]).unwrap_err();
    assert!(e.to_string().starts_with("bad.toml:"), "{e}");
    let e = cfg(&[("bad.toml", "[profile.x]\nslice_us = 5\n")]).unwrap_err();
    assert!(e.to_string().contains("slice_us"), "{e}");
    let e = cfg(&[("bad.toml", "[profile.x]\nnicee = 4\n")]).unwrap_err();
    assert!(e.to_string().contains("nicee"), "{e}");
}

#[test]
fn later_files_override_and_rules_go_first() {
    let c = cfg(&[
        (
            "share.toml",
            r#"
            [system]
            rescan_secs = 30
            [profile.p]
            nice = 5
            [[rule]]
            profile = "p"
            name = "foo"
            "#,
        ),
        (
            "etc.toml",
            r#"
            [system]
            autogroup = false
            [profile.p]
            nice = 7
            [[rule]]
            profile = "p"
            name = "bar"
            "#,
        ),
    ])
    .unwrap();
    assert_eq!(c.system.rescan_secs, 30);
    assert_eq!(c.system.autogroup, Some(false));
    assert_eq!(c.profiles["p"].profile.nice, Some(7));
    let sources: Vec<_> = c.rules.iter().map(|r| r.source.to_str().unwrap()).collect();
    assert_eq!(sources, ["etc.toml", "share.toml"]);
}

#[test]
fn rule_needs_known_profile() {
    let e = cfg(&[("a.toml", "[[rule]]\nprofile = \"ghost\"\nname = \"x\"\n")]).unwrap_err();
    assert!(e.to_string().contains("ghost"), "{e}");
}

#[test]
fn shipped_default_config_is_valid() {
    let text = include_str!("../../data/scdr.toml");
    let c = cfg(&[("data/scdr.toml", text)]).unwrap();
    assert!(c.rules.len() > 5);
    assert!(c.profiles.contains_key("audio"));
}
