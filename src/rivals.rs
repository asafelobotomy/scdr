//! Other priority daemons. Two of them renice the same processes and undo
//! each other's work, so scdrd refuses to start while one runs (DESIGN.md §6).
//! scdrd.service also lists their units in `Conflicts=`; this check catches
//! one started some other way. Others overlap only partly (falcond's game
//! boosting and scheduler switching): scdr warns about those and runs.

use std::path::Path;

/// `comm` (at most 15 bytes) of each rival, and what to tell the user.
const RIVALS: &[(&str, &str)] = &[
    ("ananicy-cpp", "ananicy-cpp"),
    // "system76-scheduler", cut to 15 bytes by the kernel.
    ("system76-schedu", "system76-scheduler"),
];

/// Daemons that overlap scdr's game role and `[sched_ext]`: warned about.
const OVERLAPPING: &[(&str, &str)] = &[("falcond", "falcond")];

/// Names of the partly overlapping daemons running now.
pub fn overlapping(proc: &Path) -> Vec<&'static str> {
    scan(proc, OVERLAPPING)
}

/// Names of the rival daemons running now, read from `proc` (normally
/// `/proc`), each once.
pub fn running(proc: &Path) -> Vec<&'static str> {
    scan(proc, RIVALS)
}

fn scan(proc: &Path, list: &[(&str, &'static str)]) -> Vec<&'static str> {
    let Ok(dir) = std::fs::read_dir(proc) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in dir.flatten() {
        let name = entry.file_name();
        if !name
            .to_str()
            .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) else {
            continue;
        };
        let comm = comm.trim_end();
        if let Some((_, rival)) = list.iter().find(|(c, _)| *c == comm)
            && !found.contains(rival)
        {
            found.push(*rival);
        }
    }
    found.sort_unstable();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_rivals_by_comm() {
        let dir = std::env::temp_dir().join(format!("scdr-rivals-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (pid, comm) in [
            ("1", "systemd"),
            ("40", "ananicy-cpp"),
            ("41", "ananicy-cpp"),
            ("77", "system76-schedu"),
            ("90", "falcond"),
        ] {
            std::fs::create_dir_all(dir.join(pid)).unwrap();
            std::fs::write(dir.join(pid).join("comm"), format!("{comm}\n")).unwrap();
        }
        std::fs::create_dir_all(dir.join("self")).unwrap();
        assert_eq!(running(&dir), ["ananicy-cpp", "system76-scheduler"]);
        assert_eq!(overlapping(&dir), ["falcond"]);
        std::fs::remove_dir_all(dir.join("40")).unwrap();
        std::fs::remove_dir_all(dir.join("41")).unwrap();
        std::fs::remove_dir_all(dir.join("77")).unwrap();
        assert!(running(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(running(&dir).is_empty());
    }
}
