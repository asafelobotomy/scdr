//! Hyprland: `activewindowv2>>ADDRESS` events on the event socket (socket2),
//! then `j/activewindow` on the request socket for the window's pid.

use super::Shared;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

fn socket_dir() -> Option<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    [runtime.map(|r| r.join("hypr").join(&sig)), Some(PathBuf::from("/tmp/hypr").join(&sig))]
        .into_iter()
        .flatten()
        .find(|d| d.join(".socket2.sock").exists())
}

pub fn available() -> bool {
    socket_dir().is_some()
}

pub async fn watch(shared: Shared) -> anyhow::Result<()> {
    let dir = socket_dir().ok_or_else(|| anyhow::anyhow!("Hyprland sockets not found"))?;
    let report = |pid: Option<u32>| {
        if let Some(pid) = pid {
            shared.update(|s| s.foreground = Some(pid));
        }
    };
    report(active_pid(&dir).await);
    let mut events = BufReader::new(UnixStream::connect(dir.join(".socket2.sock")).await?).lines();
    while let Some(line) = events.next_line().await? {
        if is_focus_event(&line) {
            report(active_pid(&dir).await);
        }
    }
    Ok(())
}

fn is_focus_event(line: &str) -> bool {
    line.split_once(">>")
        .is_some_and(|(event, addr)| event == "activewindowv2" && !addr.is_empty() && addr != ",")
}

async fn active_pid(dir: &std::path::Path) -> Option<u32> {
    let mut s = UnixStream::connect(dir.join(".socket.sock")).await.ok()?;
    s.write_all(b"j/activewindow").await.ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).await.ok()?;
    parse_active_window(&out)
}

fn parse_active_window(json: &str) -> Option<u32> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let pid = v.get("pid")?.as_i64()?;
    u32::try_from(pid).ok().filter(|p| *p > 0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_events_and_active_window() {
        assert!(super::is_focus_event("activewindowv2>>5612a3e9c4f0"));
        assert!(!super::is_focus_event("activewindowv2>>"));
        assert!(!super::is_focus_event("activewindow>>kitty,~"));
        assert!(!super::is_focus_event("workspace>>2"));
        let json = r#"{"address": "0x5612a3e9c4f0", "class": "kitty", "pid": 4242, "title": "~"}"#;
        assert_eq!(super::parse_active_window(json), Some(4242));
        assert_eq!(super::parse_active_window("{}"), None);
        assert_eq!(super::parse_active_window(r#"{"pid": -1}"#), None);
    }
}
