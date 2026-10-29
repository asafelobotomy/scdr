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

#[test]
fn m2_fields_validate_and_inherit() {
    let c = cfg(&[(
        "a.toml",
        r#"
        [profile.base]
        cpu_weight = 300
        uclamp_min = 128
        [profile.child]
        inherit = "base"
        io_weight = 50
        uclamp_max = 512
        "#,
    )])
    .unwrap();
    let p = &c.profiles["child"].profile;
    assert_eq!((p.cpu_weight, p.io_weight), (Some(300), Some(50)));
    assert_eq!((p.uclamp_min, p.uclamp_max), (Some(128), Some(512)));
    assert!(p.has_unit_weights() && p.has_uclamp());

    for (bad, what) in [
        ("cpu_weight = 0", "cpu_weight"),
        ("io_weight = 10001", "io_weight"),
        ("uclamp_max = 2000", "uclamp_max"),
        ("uclamp_min = 600\nuclamp_max = 500", "above uclamp_max"),
    ] {
        let e = cfg(&[("b.toml", &format!("[profile.x]\n{bad}\n"))]).unwrap_err();
        assert!(e.to_string().contains(what), "{bad}: {e}");
    }

    let c = cfg(&[("s.toml", "[system]\niocost = true\n")]).unwrap();
    assert!(c.system.iocost);
}

#[test]
fn roles_layer_and_follow_power_profile() {
    let c = cfg(&[
        (
            "share.toml",
            r#"
            [profile.fg]
            nice = -2
            [profile.au]
            nice = -4
            [roles]
            foreground = "fg"
            audio = "au"
            [roles.power.power-saver]
            foreground = ""
            "#,
        ),
        ("etc.toml", "[roles]\naudio = \"\"\n"),
    ])
    .unwrap();
    let on_ac = c.roles.for_power(Some("performance"));
    assert_eq!(on_ac.get(Role::Foreground), Some("fg"));
    assert_eq!(on_ac.get(Role::Audio), None, "etc.toml turned audio off");
    assert_eq!(c.roles.for_power(Some("power-saver")).get(Role::Foreground), None);
    assert_eq!(c.roles.for_power(None).get(Role::Game), None);

    let e = cfg(&[("x.toml", "[roles]\ngame = \"nope\"\n")]).unwrap_err();
    assert!(e.to_string().contains("unknown profile"), "{e}");
}

#[test]
fn scheduler_policy_and_pressure() {
    use super::policy::SchedChoice;
    let c = cfg(&[(
        "p.toml",
        r#"
        [sched_ext]
        enable = true
        default = "eevdf"
        gaming = "lavd:gaming"
        [sched_ext.power]
        power-saver = "scx_lavd:powersave"
        performance = "bpfland"
        [eevdf]
        preempt = "full"
        [eevdf.power.power-saver]
        preempt = "lazy"
        base_slice_us = 4000
        [pressure]
        high = 30.0
        hold_secs = 10
        "#,
    )])
    .unwrap();
    let ext = |name: &str, mode| SchedChoice::Ext { name: name.into(), mode };
    let s = &c.sched_ext;
    assert_eq!(s.choose(Some("balanced"), false), SchedChoice::Eevdf);
    assert_eq!(s.choose(Some("performance"), false), ext("scx_bpfland", 0));
    assert_eq!(s.choose(Some("power-saver"), true), ext("scx_lavd", 1), "gaming wins");
    assert_eq!(s.choose(Some("power-saver"), false), ext("scx_lavd", 2));
    assert_eq!(ext("scx_lavd", 2).to_string(), "scx_lavd:powersave");

    let ps = c.eevdf.for_power(Some("power-saver"));
    assert_eq!((ps.preempt.as_deref(), ps.base_slice_us), (Some("lazy"), Some(4000)));
    assert_eq!(c.eevdf.for_power(None).preempt.as_deref(), Some("full"));
    assert_eq!((c.pressure.high, c.pressure.low, c.pressure.hold_secs), (30.0, 5.0, 10));

    for bad in [
        "[sched_ext]\ndefault = \"lavd:turbo\"\n",
        "[eevdf]\npreempt = \"always\"\n",
        "[pressure]\nhigh = 4.0\n",
        "[pressure]\nhold_secs = 86400\n",
    ] {
        assert!(cfg(&[("b.toml", bad)]).is_err(), "{bad}");
    }
}
