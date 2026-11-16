//! The preview document every integrated app prints for `--dry-run --json`
//! (uQ's confirmation sheet reads it: `docs/settings-pages.md`, "Progress and
//! previews"). One document, so the window and the command line say the same.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub group: String,
    pub label: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    /// `Some(true)`: it can be put back; `Some(false)`: it can't; `None`: unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reversible: Option<bool>,
}

impl Change {
    pub fn new(group: &str, label: impl Into<String>, detail: impl Into<String>, reversible: Option<bool>) -> Self {
        Change { group: group.into(), label: label.into(), detail: detail.into(), bytes: None, reversible }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Plan {
    pub summary: String,
    pub changes: Vec<Change>,
    pub would_refuse: bool,
    pub reason: String,
}

impl Plan {
    pub fn new(summary: impl Into<String>, changes: Vec<Change>) -> Self {
        Plan { summary: summary.into(), changes, would_refuse: false, reason: String::new() }
    }

    /// A plan for something scdr would turn down, and why.
    pub fn refused(summary: impl Into<String>, reason: impl Into<String>) -> Self {
        Plan { summary: summary.into(), changes: Vec::new(), would_refuse: true, reason: reason.into() }
    }

    pub fn print(&self) {
        println!("{}", serde_json::to_string(self).expect("a plan is JSON"));
    }

    /// The plan for people: the summary, then each change under its group.
    pub fn print_text(&self) {
        println!("{}", self.summary);
        if self.would_refuse {
            println!("would refuse: {}", self.reason);
        }
        let mut group = "";
        for c in &self.changes {
            if c.group != group {
                println!("{}:", c.group);
                group = &c.group;
            }
            let how = match c.reversible {
                Some(true) => "  (can be undone)",
                Some(false) => "  (can't be undone)",
                None => "",
            };
            if c.detail.is_empty() {
                println!("  {}{how}", c.label);
            } else {
                println!("  {}: {}{how}", c.label, c.detail);
            }
        }
    }
}
