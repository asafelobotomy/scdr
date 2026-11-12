//! scx_devbox as one of scdr's scheduler choices. scx_loader doesn't know
//! it, so scdr starts and stops its systemd unit (`scx_devbox.service`)
//! instead, stopping scx_loader's scheduler first: only one sched_ext
//! scheduler runs at a time.

use crate::config::SchedChoice;
use std::time::Duration;

/// The name scdr's config and the widget use, as `SchedChoice` stores it.
pub const NAME: &str = "scx_devbox";
const UNIT: &str = "scx_devbox.service";
/// What the kernel reports in `/sys/kernel/sched_ext/root/ops`.
const OPS: &str = "devbox";

pub fn is(choice: &SchedChoice) -> bool {
    matches!(choice, SchedChoice::Ext { name, .. } if name == NAME)
}

pub fn choice() -> SchedChoice {
    SchedChoice::Ext { name: NAME.into(), mode: 0 }
}

pub fn installed() -> bool {
    crate::schedulers::on_path(NAME)
}

/// Whether the kernel runs scx_devbox now.
pub fn running() -> bool {
    crate::system::sched_ext().is_some_and(|s| is_devbox(&s))
}

fn is_devbox(s: &crate::system::SchedExt) -> bool {
    s.is_active() && s.ops.as_deref().is_some_and(|o| o == OPS || o.starts_with("devbox_"))
}

pub async fn start(bus: &zbus::Connection) -> zbus::Result<()> {
    unit(bus, "StartUnit").await
}

/// Stops the unit and waits (up to 5 s) for the kernel to let go of it, so
/// scx_loader can load the next scheduler.
pub async fn stop(bus: &zbus::Connection) -> zbus::Result<()> {
    unit(bus, "StopUnit").await?;
    for _ in 0..50 {
        if crate::system::sched_ext().is_none_or(|s| s.state == "disabled") {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(zbus::Error::Failure("scx_devbox didn't stop within 5 s".into()))
}

async fn unit(bus: &zbus::Connection, method: &str) -> zbus::Result<()> {
    bus.call_method(
        Some("org.freedesktop.systemd1"),
        "/org/freedesktop/systemd1",
        Some("org.freedesktop.systemd1.Manager"),
        method,
        &(UNIT, "replace"),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::system::SchedExt;

    #[test]
    fn recognises_devbox() {
        let ext = |state: &str, ops: &str| SchedExt { state: state.into(), ops: Some(ops.into()) };
        assert!(super::is_devbox(&ext("enabled", "devbox")));
        assert!(!super::is_devbox(&ext("enabled", "bpfland_1.1.4_x86_64_unknown_linux_gnu")));
        assert!(!super::is_devbox(&ext("disabled", "devbox")));
        assert!(super::is(&"devbox".parse().unwrap()));
        assert!(!super::is(&"bpfland".parse().unwrap()));
    }
}
