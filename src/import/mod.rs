//! `scdrctl import`: converts ananicy-cpp and system76-scheduler rules into
//! an scdr rules.d file. Imported profiles are prefixed (`ananicy-`, `s76-`)
//! so they never replace shipped ones.

pub mod ananicy;
pub mod system76;

use crate::config::Config;
use crate::procfs::ProcInfo;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

/// A TOML value as text, quoted where needed.
pub type Value = String;
/// A rule's conditions: key and patterns, e.g. `("name", ["foo", "bar"])`.
pub type Conditions = Vec<(String, Vec<String>)>;

#[derive(Debug, Default)]
pub struct Import {
    pub source: String,
    profiles: BTreeMap<String, Vec<(String, Value)>>,
    rules: Vec<(String, Conditions)>,
    exception_names: Vec<String>,
    launchers: Vec<String>,
    /// What couldn't be converted, one line each.
    pub skipped: Vec<String>,
}

impl Import {
    pub fn new(source: impl Into<String>) -> Self {
        Import { source: source.into(), ..Default::default() }
    }

    pub fn profile(&mut self, name: &str, fields: Vec<(String, Value)>) {
        self.profiles.insert(name.to_owned(), fields);
    }

    pub fn has_profile(&self, name: &str) -> bool {
        self.profiles.contains_key(name)
    }

    /// One rule; each condition is a key and its patterns.
    pub fn rule(&mut self, profile: &str, conditions: Conditions) {
        self.rules.push((profile.to_owned(), conditions));
    }

    pub fn exception(&mut self, name: &str) {
        self.exception_names.push(glob_literal(name));
    }

    pub fn launcher(&mut self, name: &str) {
        self.launchers.push(glob_literal(name));
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    pub fn pattern_count(&self) -> usize {
        self.rules.iter().flat_map(|(_, c)| c).map(|(_, v)| v.len()).sum()
    }

    pub fn profile_count(&self) -> usize {
        self.profiles.len()
    }

    pub fn to_toml(&self) -> String {
        let mut out = format!(
            "# Imported by `scdrctl import` from {}.\n# Regenerate rather than edit; put your own rules in another file.\n",
            self.source
        );
        if !self.exception_names.is_empty() || !self.launchers.is_empty() {
            out.push_str("\n[exceptions]\n");
            let _ = writeln!(out, "name = {}", array(&self.exception_names));
            let _ = writeln!(out, "launchers = {}", array(&self.launchers));
        }
        for (name, fields) in &self.profiles {
            let _ = write!(out, "\n[profile.{}]\n", quote(name));
            for (k, v) in fields {
                let _ = writeln!(out, "{k} = {v}");
            }
        }
        for (profile, conditions) in &self.rules {
            out.push_str("\n[[rule]]\n");
            let _ = writeln!(out, "profile = {}", quote(profile));
            for (k, patterns) in conditions {
                let _ = writeln!(out, "{k} = {}", array(patterns));
            }
        }
        out
    }

    /// Checks the output loads alongside the given config files.
    pub fn validate(&self, with: &[PathBuf]) -> Result<Config, String> {
        let mut sources = Vec::new();
        for path in with {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            sources.push((path.clone(), text));
        }
        sources.push((PathBuf::from("<import>"), self.to_toml()));
        Config::from_sources(sources).map_err(|e| e.to_string())
    }
}

pub fn quote(s: &str) -> Value {
    toml::Value::String(s.to_owned()).to_string()
}

fn array(items: &[String]) -> String {
    if items.len() <= 4 {
        let quoted: Vec<String> = items.iter().map(|s| quote(s)).collect();
        return format!("[{}]", quoted.join(", "));
    }
    let mut s = String::from("[\n");
    for item in items {
        let _ = writeln!(s, "    {},", quote(item));
    }
    s.push(']');
    s
}

/// Escapes glob metacharacters so a literal process name matches only itself.
pub fn glob_literal(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '*' | '?' | '[' | ']' | '{' | '}' => {
                out.push('[');
                out.push(c);
                out.push(']');
            }
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out
}

/// A stand-in process with this name, to test it against existing rules.
pub fn synthetic(name: &str) -> ProcInfo {
    ProcInfo {
        pid: 0,
        ppid: 0,
        start_time: 0,
        comm: name.chars().take(15).collect(),
        exe: Some(format!("/usr/bin/{name}")),
        cmdline: vec![format!("/usr/bin/{name}")],
        cgroup: String::new(),
        uid: u32::MAX,
        kthread: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_glob_metacharacters() {
        assert_eq!(glob_literal("a*b?[c]{d}"), "a[*]b[?][[]c[]][{]d[}]");
        let mut imp = Import::new("test");
        imp.profile("p", vec![("nice".into(), "1".into())]);
        imp.rule("p", vec![("name".into(), vec![glob_literal("weird[1].exe")])]);
        let c = imp.validate(&[]).unwrap();
        assert!(c.rules.find(&synthetic("weird[1].exe")).is_some());
        assert!(c.rules.find(&synthetic("weird1.exe")).is_none());
    }
}
