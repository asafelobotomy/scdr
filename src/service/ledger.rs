//! Crash recovery. Everything scdr changes is undone on a clean stop, but a
//! crash or SIGKILL used to leave it all in place, and the next start then
//! read scdr's own values as the system's: unit weights it set looked like
//! the unit's own, boosted processes looked like their originals.
//!
//! So what scdr has changed is kept in a ledger in its RuntimeDirectory
//! (/run/scdr, cleared at boot along with every setting it records). The
//! main loop rewrites it when it changes; a clean stop removes it; a start
//! that finds one first puts back what it lists.

use super::{Service, State};
use crate::apply::{self, Snapshot};
use crate::procfs::ProcInfo;
use crate::system;
use crate::units::{self, UnitId, Weights};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    boot_id: String,
    procs: Vec<LedgerProc>,
    units: Vec<LedgerUnit>,
    autogroup_original: Option<bool>,
    iocost_enabled: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct LedgerProc {
    pid: u32,
    start_time: u64,
    original: Snapshot,
    target: Snapshot,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct LedgerUnit {
    id: UnitId,
    original: Weights,
    applied: Weights,
}

impl Ledger {
    fn is_empty(&self) -> bool {
        self.procs.is_empty() && self.units.is_empty() && self.autogroup_original.is_none() && self.iocost_enabled.is_empty()
    }
}

fn boot_id() -> String {
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

impl Service {
    fn ledger(&self) -> Ledger {
        let mut procs: Vec<LedgerProc> = self
            .procs
            .iter()
            .filter_map(|(&pid, e)| match &e.state {
                State::Managed(m) if m.target != m.original => Some(LedgerProc {
                    pid,
                    start_time: e.start_time,
                    original: m.original,
                    target: m.target,
                }),
                _ => None,
            })
            .collect();
        procs.sort_by_key(|p| p.pid);
        Ledger {
            boot_id: String::new(),
            procs,
            units: self
                .units
                .iter()
                .filter(|(_, s)| s.applied != s.original)
                .map(|(id, s)| LedgerUnit {
                    id: id.clone(),
                    original: s.original,
                    applied: s.applied,
                })
                .collect(),
            autogroup_original: self.autogroup_original,
            iocost_enabled: self.iocost_enabled.clone(),
        }
    }

    /// Writes the ledger if it changed since the last write; removes it
    /// when scdr has nothing changed.
    pub fn save_ledger(&mut self) {
        let Some(path) = self.ledger_path.clone() else {
            return;
        };
        if self.dry_run {
            return;
        }
        let ledger = self.ledger();
        if ledger.is_empty() {
            if self.ledger_written.take().is_some() || path.exists() {
                let _ = fs::remove_file(&path);
            }
            return;
        }
        let Ok(json) = serde_json::to_string(&Ledger { boot_id: boot_id(), ..ledger }) else {
            return;
        };
        if self.ledger_written.as_deref() == Some(json.as_str()) {
            return;
        }
        match write_atomic(&path, &json) {
            Ok(()) => self.ledger_written = Some(json),
            Err(e) => tracing::warn!("can't write {}: {e}", path.display()),
        }
    }

    /// At startup: puts back what a previous run changed and didn't restore.
    pub(super) fn recover_ledger(&mut self) {
        let Some(path) = self.ledger_path.clone() else {
            return;
        };
        let Ok(text) = fs::read_to_string(&path) else {
            return;
        };
        if self.dry_run {
            tracing::info!("would recover changes from an unclean stop ({})", path.display());
            return;
        }
        let ledger: Ledger = match serde_json::from_str(&text) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("ignoring unreadable {}: {e}", path.display());
                let _ = fs::remove_file(&path);
                return;
            }
        };
        if ledger.boot_id != boot_id() {
            let _ = fs::remove_file(&path);
            return;
        }
        tracing::warn!(
            "previous run didn't stop cleanly: restoring {} processes, {} units",
            ledger.procs.len(),
            ledger.units.len()
        );
        for p in &ledger.procs {
            // Same pid and start time: the same process, not a reused pid.
            if ProcInfo::read(p.pid).is_ok_and(|i| i.start_time == p.start_time) {
                apply::apply(p.pid, &p.original, &[p.target], false);
            }
        }
        for u in &ledger.units {
            match units::get(&u.id) {
                Ok(now) if now == u.applied => {
                    if let Err(e) = units::set(&u.id, &u.original, &now) {
                        tracing::warn!("{}: can't restore {}: {e}", u.id, u.original);
                    }
                }
                _ => {} // gone, or changed by something else since
            }
        }
        if let Some(orig) = ledger.autogroup_original {
            let _ = system::set_autogroup(orig);
        }
        for dev in &ledger.iocost_enabled {
            let _ = system::set_iocost(dev, false);
        }
        let _ = fs::remove_file(&path);
    }
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn ledger_round_trips_and_clears() {
        let dir = std::env::temp_dir().join(format!("scdr-ledger-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ledger.json");
        let mut svc = Service::new(Config::from_sources(Vec::new()).unwrap(), false);
        svc.ledger_path = Some(path.clone());

        svc.autogroup_original = Some(true);
        svc.save_ledger();
        let saved: Ledger = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.autogroup_original, Some(true));
        assert_eq!(saved.boot_id, boot_id());

        // Nothing changed any more: the ledger goes away.
        svc.autogroup_original = None;
        svc.save_ledger();
        assert!(!path.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stale_boot_is_ignored() {
        let dir = std::env::temp_dir().join(format!("scdr-ledger-boot-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ledger.json");
        let old = Ledger { boot_id: "not-this-boot".into(), autogroup_original: Some(true), ..Default::default() };
        fs::write(&path, serde_json::to_string(&old).unwrap()).unwrap();
        let mut svc = Service::new(Config::from_sources(Vec::new()).unwrap(), false);
        svc.ledger_path = Some(path.clone());
        svc.recover_ledger(); // must not touch autogroup
        assert!(!path.exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
