//! Processes with a running PipeWire audio stream, from `pw-dump --monitor`.
//!
//! A stream node carries `client.id`; the client object carries
//! `application.process.id` (for PulseAudio apps too, via pipewire-pulse).
//! Only `Stream/{Output,Input}/Audio` nodes in state `running` count, so an
//! app that merely has PipeWire open isn't boosted.

use super::Shared;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::io::BufReader;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, Default)]
pub struct Graph {
    /// node id -> (client id, is an audio stream, running)
    nodes: HashMap<u64, (Option<u64>, bool, bool)>,
    /// client id -> pid
    clients: HashMap<u64, u32>,
}

impl Graph {
    /// Applies one object from pw-dump's output.
    pub fn update(&mut self, obj: &Value) {
        let Some(id) = obj.get("id").and_then(Value::as_u64) else {
            return;
        };
        let info = obj.get("info");
        if info.is_none_or(Value::is_null) {
            self.nodes.remove(&id);
            self.clients.remove(&id);
            return;
        }
        let info = info.unwrap();
        let props = info.get("props");
        let prop = |k: &str| props.and_then(|p| p.get(k));
        match obj.get("type").and_then(Value::as_str) {
            Some("PipeWire:Interface:Node") => {
                let class = prop("media.class").and_then(Value::as_str).unwrap_or("");
                let audio = class == "Stream/Output/Audio" || class == "Stream/Input/Audio";
                let entry = self.nodes.entry(id).or_insert((None, false, false));
                if props.is_some() {
                    entry.0 = prop("client.id").and_then(as_u64);
                    entry.1 = audio;
                }
                if let Some(state) = info.get("state").and_then(Value::as_str) {
                    entry.2 = state == "running";
                }
            }
            Some("PipeWire:Interface:Client") => {
                if let Some(pid) = prop("application.process.id").and_then(as_u64) {
                    self.clients.insert(id, pid as u32);
                }
            }
            _ => {}
        }
    }

    pub fn audio_pids(&self) -> BTreeSet<u32> {
        self.nodes
            .values()
            .filter(|(_, audio, running)| *audio && *running)
            .filter_map(|(client, _, _)| self.clients.get(&(*client)?).copied())
            .collect()
    }
}

/// pw-dump prints numbers, but some properties arrive as strings.
fn as_u64(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str()?.parse().ok())
}

/// Runs pw-dump in a thread, restarting it if PipeWire restarts.
pub fn spawn(shared: Shared) {
    std::thread::Builder::new()
        .name("scdr-pipewire".into())
        .spawn(move || loop {
            if let Err(e) = follow(&shared) {
                tracing::debug!("pw-dump: {e}");
            }
            shared.update(|s| s.audio.clear());
            std::thread::sleep(Duration::from_secs(5));
        })
        .expect("spawn pipewire thread");
}

fn follow(shared: &Shared) -> std::io::Result<()> {
    let mut child = Command::new("pw-dump")
        .args(["--monitor", "--no-colors"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let mut graph = Graph::default();
    let mut last = BTreeSet::new();
    let stream = serde_json::Deserializer::from_reader(BufReader::new(stdout)).into_iter::<Vec<Value>>();
    for batch in stream {
        let Ok(batch) = batch else { break };
        for obj in &batch {
            graph.update(obj);
        }
        let now = graph.audio_pids();
        if now != last {
            tracing::debug!("audio clients: {now:?}");
            last = now.clone();
            shared.update(|s| s.audio = now);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn running_audio_streams_map_to_client_pids() {
        let mut g = Graph::default();
        for obj in [
            json!({"id": 70, "type": "PipeWire:Interface:Client", "info": {"props": {"application.process.id": 4242}}}),
            json!({"id": 71, "type": "PipeWire:Interface:Client", "info": {"props": {"application.process.id": "777"}}}),
            json!({"id": 80, "type": "PipeWire:Interface:Node", "info": {"state": "running",
                "props": {"media.class": "Stream/Output/Audio", "client.id": 70}}}),
            json!({"id": 81, "type": "PipeWire:Interface:Node", "info": {"state": "suspended",
                "props": {"media.class": "Stream/Input/Audio", "client.id": 71}}}),
            json!({"id": 82, "type": "PipeWire:Interface:Node", "info": {"state": "running",
                "props": {"media.class": "Stream/Output/Video", "client.id": 71}}}),
        ] {
            g.update(&obj);
        }
        assert_eq!(g.audio_pids(), BTreeSet::from([4242]));

        // The microphone stream starts; a state-only update keeps its props.
        g.update(&json!({"id": 81, "type": "PipeWire:Interface:Node", "info": {"state": "running"}}));
        assert_eq!(g.audio_pids(), BTreeSet::from([777, 4242]));

        // Removal is {"id": N, "info": null}.
        g.update(&json!({"id": 80, "info": null}));
        assert_eq!(g.audio_pids(), BTreeSet::from([777]));
    }
}
