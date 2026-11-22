//! scdrd keeps answering on a busy bus while its loop waits on a slow peer, and still stops on
//! SIGTERM.
//!
//! 2026-11-22: at boot, scdrd's loop waited on scx_loader while services started and the
//! NameOwnerChanged signals it subscribes to piled up. zbus stops reading a connection once one of
//! its signal queues is full, so the answer the loop waited for never arrived: scdrd answered
//! nothing (uQ's Scheduling page: "no answer within 2 s") and ignored SIGTERM.
//!
//! This runs a private dbus-daemon, a slow scx_loader and a systemd stub, starts scdrd on it in
//! dry-run mode, connects 150 short-lived clients during scdrd's first scheduler check, and reads
//! `State`. Skipped when dbus-daemon isn't installed, unless `SCDR_TEST_DBUS=1` (CI sets it).

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct SlowLoader;

#[zbus::interface(name = "org.scx.Loader")]
impl SlowLoader {
    #[zbus(property)]
    async fn current_scheduler(&self) -> String {
        tokio::time::sleep(Duration::from_secs(4)).await;
        "unknown".into()
    }
    #[zbus(property)]
    fn scheduler_mode(&self) -> u32 {
        0
    }
    #[zbus(property)]
    fn supported_schedulers(&self) -> Vec<String> {
        vec![]
    }
    fn switch_scheduler(&self, _name: &str, _mode: u32) {}
    fn stop_scheduler(&self) {}
}

struct Systemd;

#[zbus::interface(name = "org.freedesktop.systemd1.Manager")]
impl Systemd {
    fn start_unit(&self, _name: &str, _mode: &str) -> zbus::OwnedObjectPath {
        zbus::OwnedObjectPath::try_from("/job/1").unwrap()
    }
    fn stop_unit(&self, _name: &str, _mode: &str) -> zbus::OwnedObjectPath {
        zbus::OwnedObjectPath::try_from("/job/1").unwrap()
    }
}

async fn state(addr: &str) -> Result<String, String> {
    let conn = zbus::connection::Builder::address(addr)
        .build()
        .await
        .map_err(|e| e.to_string())?;
    let proxy = zbus::fdo::PropertiesProxy::builder(&conn)
        .destination("org.scdr.Daemon1")
        .path("/org/scdr/Daemon1")
        .build()
        .await
        .map_err(|e| e.to_string())?;
    let iface = zbus::names::InterfaceName::try_from("org.scdr.Daemon1").unwrap();
    match tokio::time::timeout(Duration::from_secs(2), proxy.get(iface, "State")).await {
        Ok(Ok(v)) => String::try_from(v).map_err(|e| e.to_string()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("no answer within 2 s".into()),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_while_waiting_on_a_slow_peer() {
    if Command::new("dbus-daemon")
        .arg("--version")
        .output()
        .is_err()
    {
        assert!(
            std::env::var_os("SCDR_TEST_DBUS").is_none(),
            "SCDR_TEST_DBUS is set but dbus-daemon isn't installed"
        );
        eprintln!("dbus-daemon not installed; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("scdr-busy-bus-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for d in ["etc", "share", "state", "run"] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
    let socket = dir.join("bus");
    let addr = format!("unix:path={}", socket.display());
    std::fs::write(
        dir.join("bus.conf"),
        format!(
            r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>system</type><listen>{addr}</listen><auth>EXTERNAL</auth>
<policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/><allow user="*"/></policy>
</busconfig>"#
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("etc/scdr.toml"),
        "[sched_ext]\nenable = true\ndefault = \"scx_devbox\"\n",
    )
    .unwrap();
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/data/scdr.toml"),
        dir.join("share/scdr.toml"),
    )
    .unwrap();

    let _bus = Kill(
        Command::new("dbus-daemon")
            .arg(format!("--config-file={}", dir.join("bus.conf").display()))
            .args(["--nofork", "--nopidfile"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    while !socket.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "dbus-daemon didn't start"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let _peers = zbus::connection::Builder::address(addr.as_str())
        .name("org.scx.Loader")
        .serve_at("/org/scx/Loader", SlowLoader)
        .name("org.freedesktop.systemd1")
        .serve_at("/org/freedesktop/systemd1", Systemd)
        .build()
        .await
        .unwrap();

    let scdrd = Kill(
        Command::new(env!("CARGO_BIN_EXE_scdrd"))
            .arg("--dry-run")
            .arg("--etc-dir")
            .arg(dir.join("etc"))
            .arg("--share-dir")
            .arg(dir.join("share"))
            .arg("--state-dir")
            .arg(dir.join("state"))
            .env("DBUS_SYSTEM_BUS_ADDRESS", &addr)
            .env("RUNTIME_DIRECTORY", dir.join("run"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    while state(&addr).await.is_err() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "scdrd didn't come up"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Each connection that comes and goes is two NameOwnerChanged signals.
    for _ in 0..150 {
        let c = zbus::connection::Builder::address(addr.as_str())
            .build()
            .await
            .unwrap();
        drop(c);
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let read = state(&addr).await;
    assert!(
        read.is_ok(),
        "scdrd stopped answering on a busy bus: {read:?}"
    );

    let pid = scdrd.0.id() as libc::pid_t;
    // SAFETY: plain kill(2) on our own child.
    assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
    let mut scdrd = scdrd;
    let start = Instant::now();
    loop {
        if scdrd.0.try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "scdrd ignored SIGTERM"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let _ = std::fs::remove_dir_all(&dir);
}
