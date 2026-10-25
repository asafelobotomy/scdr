use crate::config::Config;
use crate::procfs::ProcInfo;
use std::path::PathBuf;

fn proc(comm: &str, exe: &str, cgroup: &str) -> ProcInfo {
    ProcInfo {
        pid: 4242,
        ppid: 1,
        start_time: 1,
        comm: comm.into(),
        exe: Some(exe.into()),
        cmdline: vec![exe.into(), "--flag".into()],
        cgroup: cgroup.into(),
        uid: 1000,
        kthread: false,
    }
}

fn rules(toml: &str) -> Config {
    let base = "[profile.a]\nnice = 1\n[profile.b]\nnice = 2\n";
    Config::from_sources(vec![(PathBuf::from("t.toml"), format!("{base}{toml}"))]).unwrap()
}

#[test]
fn first_match_wins_and_globs_cross_slashes() {
    let c = rules(
        r#"
        [[rule]]
        profile = "a"
        name = ["kwin_*"]
        [[rule]]
        profile = "b"
        cgroup = "/user.slice/*"
        "#,
    );
    let kwin = proc("kwin_wayland", "/usr/bin/kwin_wayland", "/user.slice/user-1000.slice/session.slice/plasma-kwin_wayland.service");
    let app = proc("firefox", "/usr/lib/firefox/firefox", "/user.slice/user-1000.slice/user@1000.service/app.slice/app-firefox.scope");
    assert_eq!(c.rules.find(&kwin).unwrap().profile, "a");
    assert_eq!(c.rules.find(&app).unwrap().profile, "b");
    assert!(c.rules.find(&proc("sshd", "/usr/bin/sshd", "/system.slice/sshd.service")).is_none());
}

#[test]
fn negations_and_uid() {
    let c = rules(
        r#"
        [[rule]]
        profile = "a"
        cgroup = "/system.slice/*"
        not_cgroup = "/system.slice/sddm.service"
        uid = [0]
        "#,
    );
    let mut nm = proc("NetworkManager", "/usr/bin/NetworkManager", "/system.slice/NetworkManager.service");
    nm.uid = 0;
    assert!(c.rules.find(&nm).is_some());
    let mut xorg = proc("Xorg", "/usr/lib/Xorg", "/system.slice/sddm.service");
    xorg.uid = 0;
    assert!(c.rules.find(&xorg).is_none());
    nm.uid = 1000;
    assert!(c.rules.find(&nm).is_none());
}

#[test]
fn exe_and_cmdline() {
    let c = rules(
        r#"
        [[rule]]
        profile = "a"
        cmdline = "*/steamapps/common/*"
        [[rule]]
        profile = "b"
        exe = "/usr/bin/pipewire*"
        "#,
    );
    let mut game = proc("wine64-preloade", "/usr/bin/wine64-preloader", "");
    game.cmdline = vec!["/home/u/.steam/steam/steamapps/common/Foo/foo.exe".into()];
    assert_eq!(c.rules.find(&game).unwrap().profile, "a");
    assert_eq!(c.rules.find(&proc("pipewire-pulse", "/usr/bin/pipewire-pulse", "")).unwrap().profile, "b");
}

#[test]
fn ancestor_matches_anywhere_up_the_chain() {
    use super::Lineage;
    let c = rules(
        r#"
        [[rule]]
        profile = "a"
        ancestor = "steam"
        not_name = "steamwebhelper"
        "#,
    );
    let game = proc("game.x86_64", "/games/game.x86_64", "");
    let chain = |v: &[&str]| Lineage::known(v.iter().map(|s| s.to_string()).collect());
    assert!(c.rules.find_with(&game, &mut chain(&["reaper", "steam", "systemd"])).is_some());
    assert!(c.rules.find_with(&game, &mut chain(&["bash", "systemd"])).is_none());
    let helper = proc("steamwebhelper", "/x/steamwebhelper", "");
    assert!(c.rules.find_with(&helper, &mut chain(&["steam"])).is_none());
}

#[test]
fn rule_without_positive_condition_is_rejected() {
    let e = Config::from_sources(vec![(
        PathBuf::from("t.toml"),
        "[profile.a]\nnice = 1\n[[rule]]\nprofile = \"a\"\nnot_name = \"x\"\n".into(),
    )])
    .unwrap_err();
    assert!(e.to_string().contains("at least one"), "{e}");
}

#[test]
fn exceptions_and_launchers() {
    let c = Config::from_sources(vec![(
        PathBuf::from("t.toml"),
        "[exceptions]\nname = [\"systemd\"]\nlaunchers = [\"gamemoderun\", \"chrt\"]\n".into(),
    )])
    .unwrap();
    assert_eq!(c.exceptions.exception(&proc("systemd", "/usr/lib/systemd/systemd", "")), Some("systemd"));
    assert_eq!(c.exceptions.launcher(&proc("chrt", "/usr/bin/chrt", "")), Some("chrt"));
    assert!(c.exceptions.launcher(&proc("bash", "/usr/bin/bash", "")).is_none());
}
