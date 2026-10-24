//! Compiled rules: first match wins.
//!
//! Within a rule, every condition that is present must hold (AND); within a
//! condition, any listed pattern may match (OR). Patterns are globs in which
//! `*` also crosses `/`, so `cgroup = "/system.slice/*"` covers nested units.

use crate::config::{ConfigError, NamedProfile, Patterns, RawExceptions, RawRule};
use crate::procfs::ProcInfo;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

#[derive(Debug, Clone)]
struct Matcher(GlobSet);

impl Matcher {
    fn compile(patterns: &[String]) -> Result<Self, String> {
        let mut b = GlobSetBuilder::new();
        for p in patterns {
            let glob = GlobBuilder::new(p)
                .literal_separator(false)
                .backslash_escape(true)
                .build()
                .map_err(|e| format!("bad pattern {p:?}: {e}"))?;
            b.add(glob);
        }
        b.build().map(Matcher).map_err(|e| e.to_string())
    }

    fn is_match(&self, s: &str) -> bool {
        self.0.is_match(s)
    }

    fn any(&self, items: &[&str]) -> bool {
        items.iter().any(|s| self.is_match(s))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Rule {
    pub source: PathBuf,
    /// 1-based position within its file.
    pub index: usize,
    pub profile: String,
    /// Human-readable conditions, for `scdrctl explain`.
    pub conditions: String,
    #[serde(skip)]
    m: RuleMatchers,
}

#[derive(Debug, Clone, Default)]
struct RuleMatchers {
    name: Option<Matcher>,
    exe: Option<Matcher>,
    cmdline: Option<Matcher>,
    cgroup: Option<Matcher>,
    parent: Option<Matcher>,
    uid: Option<Vec<u32>>,
    not_name: Option<Matcher>,
    not_exe: Option<Matcher>,
    not_cmdline: Option<Matcher>,
    not_cgroup: Option<Matcher>,
    not_parent: Option<Matcher>,
}

impl Rule {
    fn compile(source: PathBuf, index: usize, raw: &RawRule) -> Result<Self, String> {
        let mut conditions = String::new();
        let mut field = |key: &str, p: &Option<Patterns>| -> Result<Option<Matcher>, String> {
            let Some(p) = p else { return Ok(None) };
            let list = p.as_slice();
            if list.is_empty() {
                return Err(format!("{key} is an empty list"));
            }
            let _ = write!(conditions, "{}{key}={list:?}", if conditions.is_empty() { "" } else { " " });
            Matcher::compile(list).map(Some)
        };

        let m = RuleMatchers {
            name: field("name", &raw.name)?,
            exe: field("exe", &raw.exe)?,
            cmdline: field("cmdline", &raw.cmdline)?,
            cgroup: field("cgroup", &raw.cgroup)?,
            parent: field("parent", &raw.parent)?,
            uid: raw.uid.clone(),
            not_name: field("not_name", &raw.not_name)?,
            not_exe: field("not_exe", &raw.not_exe)?,
            not_cmdline: field("not_cmdline", &raw.not_cmdline)?,
            not_cgroup: field("not_cgroup", &raw.not_cgroup)?,
            not_parent: field("not_parent", &raw.not_parent)?,
        };
        if let Some(uids) = &raw.uid {
            let _ = write!(conditions, "{}uid={uids:?}", if conditions.is_empty() { "" } else { " " });
        }

        let positive = m.name.is_some()
            || m.exe.is_some()
            || m.cmdline.is_some()
            || m.cgroup.is_some()
            || m.parent.is_some()
            || m.uid.is_some();
        if !positive {
            return Err("rule needs at least one of name, exe, cmdline, cgroup, parent, uid".into());
        }

        Ok(Rule {
            source,
            index,
            profile: raw.profile.clone(),
            conditions,
            m,
        })
    }

    /// `parent` is the parent's comm; only read when the rule asks for it.
    pub fn matches(&self, p: &ProcInfo, parent: &mut dyn FnMut() -> Option<String>) -> bool {
        let m = &self.m;
        let names = p.names();
        let exe = p.exe.as_deref().unwrap_or("");
        let pos = |mm: &Option<Matcher>, f: &dyn Fn(&Matcher) -> bool| mm.as_ref().is_none_or(f);
        let neg = |mm: &Option<Matcher>, f: &dyn Fn(&Matcher) -> bool| !mm.as_ref().is_some_and(f);

        if !pos(&m.name, &|x| x.any(&names)) || !neg(&m.not_name, &|x| x.any(&names)) {
            return false;
        }
        if !pos(&m.exe, &|x| x.is_match(exe)) || !neg(&m.not_exe, &|x| x.is_match(exe)) {
            return false;
        }
        if m.uid.as_ref().is_some_and(|u| !u.contains(&p.uid)) {
            return false;
        }
        if !pos(&m.cgroup, &|x| x.is_match(&p.cgroup)) || !neg(&m.not_cgroup, &|x| x.is_match(&p.cgroup)) {
            return false;
        }
        if m.cmdline.is_some() || m.not_cmdline.is_some() {
            let cmd = p.cmdline_string();
            if !pos(&m.cmdline, &|x| x.is_match(&cmd)) || !neg(&m.not_cmdline, &|x| x.is_match(&cmd)) {
                return false;
            }
        }
        if m.parent.is_some() || m.not_parent.is_some() {
            let parent = parent().unwrap_or_default();
            if !pos(&m.parent, &|x| x.is_match(&parent)) || !neg(&m.not_parent, &|x| x.is_match(&parent)) {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Default)]
pub struct RuleSet {
    rules: Vec<Rule>,
}

impl RuleSet {
    pub(crate) fn compile(
        chunks: Vec<(PathBuf, Vec<RawRule>)>,
        profiles: &BTreeMap<String, NamedProfile>,
    ) -> Result<Self, ConfigError> {
        let mut rules = Vec::new();
        for (path, raws) in chunks {
            for (i, raw) in raws.iter().enumerate() {
                if !profiles.contains_key(&raw.profile) {
                    return Err(ConfigError::invalid(
                        &path,
                        format!("rule #{}: unknown profile {:?}", i + 1, raw.profile),
                    ));
                }
                let rule = Rule::compile(path.clone(), i + 1, raw)
                    .map_err(|e| ConfigError::invalid(&path, format!("rule #{}: {e}", i + 1)))?;
                rules.push(rule);
            }
        }
        Ok(RuleSet { rules })
    }

    pub fn find(&self, p: &ProcInfo) -> Option<&Rule> {
        let mut parent_cache: Option<Option<String>> = None;
        let mut parent = || {
            parent_cache
                .get_or_insert_with(|| crate::procfs::comm(p.ppid))
                .clone()
        };
        self.rules.iter().find(|r| r.matches(p, &mut parent))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Rule> {
        self.rules.iter()
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// Processes scdr must never touch.
#[derive(Debug)]
pub struct Exceptions {
    name: Matcher,
    launchers: Matcher,
}

impl Default for Exceptions {
    fn default() -> Self {
        Exceptions::compile(&RawExceptions::default()).expect("empty exceptions compile")
    }
}

impl Exceptions {
    pub(crate) fn compile(raw: &RawExceptions) -> Result<Self, String> {
        Ok(Exceptions {
            name: Matcher::compile(&raw.name)?,
            launchers: Matcher::compile(&raw.launchers)?,
        })
    }

    /// The name that made `p` an exception, if any.
    pub fn exception<'a>(&self, p: &'a ProcInfo) -> Option<&'a str> {
        p.names().into_iter().find(|n| self.name.is_match(n))
    }

    /// Launchers (`chrt`, `nice`, `gamemoderun`, …) hand-tune what they start:
    /// the launcher, whatever it execs into, and all its descendants are left alone.
    pub fn launcher<'a>(&self, p: &'a ProcInfo) -> Option<&'a str> {
        p.names().into_iter().find(|n| self.launchers.is_match(n))
    }

    pub fn is_launcher_name(&self, name: &str) -> bool {
        self.launchers.is_match(name)
    }
}

#[cfg(test)]
mod tests {
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
}
