//! System-wide settings: autogroup and iocost. Both are restored on shutdown.

use super::Service;
use crate::system;

impl Service {
    pub(super) fn apply_system(&mut self) {
        self.apply_autogroup();
        self.apply_iocost();
    }

    pub(super) fn restore_system(&mut self) {
        if let Some(orig) = self.autogroup_original.take() {
            if !self.dry_run {
                let _ = system::set_autogroup(orig);
            }
        }
        for dev in std::mem::take(&mut self.iocost_enabled) {
            match system::set_iocost(&dev, false) {
                Ok(()) => tracing::info!("disabled iocost on {dev} again"),
                Err(e) => tracing::warn!("could not disable iocost on {dev}: {e}"),
            }
        }
    }

    fn apply_autogroup(&mut self) {
        let Some(want) = self.config.system.autogroup else {
            return;
        };
        let current = system::autogroup();
        if current == Some(want) {
            return;
        }
        if self.autogroup_original.is_none() {
            self.autogroup_original = current;
        }
        if self.dry_run {
            tracing::info!("would set sched_autogroup_enabled={}", want as u8);
        } else if let Err(e) = system::set_autogroup(want) {
            tracing::warn!("could not set sched_autogroup_enabled: {e}");
        } else {
            tracing::info!("set sched_autogroup_enabled={}", want as u8);
        }
    }

    /// With `iocost = true`, enables the iocost controller on disks that
    /// otherwise ignore `IOWeight=` (anything not using bfq).
    fn apply_iocost(&mut self) {
        if !self.config.system.iocost {
            // Turned off by a reload: undo only what we enabled.
            for dev in std::mem::take(&mut self.iocost_enabled) {
                let _ = system::set_iocost(&dev, false);
            }
            return;
        }
        for disk in system::disks() {
            if disk.honours_io_weight() {
                continue;
            }
            if self.dry_run {
                tracing::info!("would enable iocost on {} ({})", disk.name, disk.dev);
                continue;
            }
            match system::set_iocost(&disk.dev, true) {
                Ok(()) => {
                    tracing::info!("enabled iocost on {} ({})", disk.name, disk.dev);
                    self.iocost_enabled.push(disk.dev);
                }
                Err(e) => tracing::warn!("could not enable iocost on {}: {e}", disk.name),
            }
        }
    }
}
