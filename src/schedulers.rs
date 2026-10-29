//! The sched_ext schedulers the widget's Schedulers page lists: what
//! scx_loader supports, whether each binary is installed, and a short
//! description for the well-known ones.

use crate::config::policy::MODES;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct SchedulerInfo {
    /// Short name as the config takes it, e.g. `bpfland`.
    pub name: String,
    pub installed: bool,
    /// Modes scx_loader has configured for it, e.g. `["auto", "gaming"]`.
    pub modes: Vec<&'static str>,
    pub description: Option<&'static str>,
}

/// `supported` comes from scx_loader: full names and configured mode numbers.
pub fn inventory(supported: Vec<(String, Vec<u32>)>) -> Vec<SchedulerInfo> {
    supported
        .into_iter()
        .map(|(full, modes)| {
            let name = full.strip_prefix("scx_").unwrap_or(&full).to_owned();
            let mut modes: Vec<&'static str> = modes.iter().filter_map(|m| MODES.get(*m as usize).copied()).collect();
            if modes.is_empty() {
                modes.push(MODES[0]);
            }
            SchedulerInfo {
                installed: on_path(&full),
                description: describe(&name),
                modes,
                name,
            }
        })
        .collect()
}

fn on_path(bin: &str) -> bool {
    let path = std::env::var_os("PATH").unwrap_or_else(|| "/usr/local/bin:/usr/bin".into());
    std::env::split_paths(&path).any(|dir| dir.join(bin).is_file()) || Path::new("/usr/bin").join(bin).is_file()
}

fn describe(name: &str) -> Option<&'static str> {
    Some(match name {
        "bpfland" => "Favours interactive tasks that sleep and wake often. A good general choice for desktops and games.",
        "lavd" => "Latency-criticality aware virtual deadlines, built for gaming; has power-saving and performance modes.",
        "rusty" => "General-purpose, with load balancing across CPU domains decided in user space.",
        "rustland" => "Makes its scheduling decisions in a user-space Rust program; mostly a showcase.",
        "flash" => "Earliest-deadline-first, aiming for fair and predictable latency.",
        "cosmos" => "Lightweight and general-purpose, keeping tasks close to their caches.",
        "p2dq" => "Pick-two load balancing over per-domain queues; versatile.",
        "tickless" => "Cuts timer ticks on busy CPUs; meant for server and HPC work.",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_and_modes() {
        let inv = super::inventory(vec![("scx_bpfland".into(), vec![0, 1]), ("scx_new".into(), vec![])]);
        assert_eq!(inv[0].name, "bpfland");
        assert_eq!(inv[0].modes, vec!["auto", "gaming"]);
        assert!(inv[0].description.is_some());
        assert_eq!((inv[1].modes.clone(), inv[1].description), (vec!["auto"], None));
    }
}
