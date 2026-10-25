//! niri: the `EventStream` request on $NIRI_SOCKET. Window events carry
//! `id` and `pid`; `WindowFocusChanged` names the focused id.

use super::Shared;
use serde_json::Value;
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

pub fn available() -> bool {
    std::env::var_os("NIRI_SOCKET").is_some()
}

pub async fn watch(shared: Shared) -> anyhow::Result<()> {
    let path = std::env::var_os("NIRI_SOCKET").ok_or_else(|| anyhow::anyhow!("NIRI_SOCKET not set"))?;
    let mut stream = UnixStream::connect(path).await?;
    stream.write_all(b"\"EventStream\"\n").await?;
    let mut lines = BufReader::new(stream).lines();
    let mut state = Windows::default();
    while let Some(line) = lines.next_line().await? {
        if let Ok(event) = serde_json::from_str::<Value>(&line) {
            if let Some(pid) = state.apply(&event) {
                shared.update(|s| s.foreground = Some(pid));
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct Windows {
    pids: HashMap<u64, u32>,
    focused: Option<u64>,
}

impl Windows {
    /// Applies one event; returns the focused window's pid when it changes.
    fn apply(&mut self, event: &Value) -> Option<u32> {
        let before = self.focused_pid();
        if let Some(e) = event.get("WindowsChanged") {
            self.pids.clear();
            for w in e.get("windows").and_then(Value::as_array).into_iter().flatten() {
                self.window(w);
            }
        } else if let Some(w) = event.get("WindowOpenedOrChanged").and_then(|e| e.get("window")) {
            self.window(w);
        } else if let Some(id) = event.get("WindowClosed").and_then(|e| e.get("id")).and_then(Value::as_u64) {
            self.pids.remove(&id);
        } else if let Some(e) = event.get("WindowFocusChanged") {
            self.focused = e.get("id").and_then(Value::as_u64);
        }
        let after = self.focused_pid();
        (after != before).then_some(after).flatten()
    }

    fn window(&mut self, w: &Value) {
        let Some(id) = w.get("id").and_then(Value::as_u64) else { return };
        if let Some(pid) = w.get("pid").and_then(Value::as_u64) {
            self.pids.insert(id, pid as u32);
        }
        if w.get("is_focused").and_then(Value::as_bool) == Some(true) {
            self.focused = Some(id);
        }
    }

    fn focused_pid(&self) -> Option<u32> {
        self.pids.get(&self.focused?).copied()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn follows_focus_across_events() {
        let mut w = super::Windows::default();
        let initial = json!({"WindowsChanged": {"windows": [
            {"id": 1, "pid": 100, "is_focused": false},
            {"id": 2, "pid": 200, "is_focused": true}
        ]}});
        assert_eq!(w.apply(&json!({"Ok": "Handled"})), None);
        assert_eq!(w.apply(&initial), Some(200));
        assert_eq!(w.apply(&json!({"WindowFocusChanged": {"id": 1}})), Some(100));
        assert_eq!(w.apply(&json!({"WindowFocusChanged": {"id": 1}})), None, "no change");
        assert_eq!(
            w.apply(&json!({"WindowOpenedOrChanged": {"window": {"id": 3, "pid": 300, "is_focused": true}}})),
            Some(300)
        );
        assert_eq!(w.apply(&json!({"WindowFocusChanged": {"id": null}})), None);
        assert_eq!(w.apply(&json!({"WindowClosed": {"id": 3}})), None);
    }
}
