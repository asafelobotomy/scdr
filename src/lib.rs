//! SCDR: a process scheduling and priority daemon for EEVDF and sched_ext kernels.

pub mod agent;
pub mod apply;
pub mod bench;
pub mod config;
pub mod dbus;
pub mod explain;
pub mod import;
pub mod plan;
pub mod policy;
pub mod procfs;
pub mod rivals;
pub mod rules;
pub mod schedulers;
pub mod sched;
pub mod service;
pub mod system;
pub mod units;
// uQ's log format and crash records (uQ's native/uq-diag; don't edit, copy a new release over).
#[rustfmt::skip]
pub mod uq_diag;
pub mod watcher;
