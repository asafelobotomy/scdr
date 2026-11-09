//! Shared plumbing for the compositor IPC watchers (Hyprland, niri).

use super::Shared;
use std::future::Future;
use std::time::{Duration, Instant};

/// Delay after `failures` consecutive short-lived sessions: 1s doubling, max 30s.
pub fn backoff(failures: u32) -> Duration {
    Duration::from_secs(1u64 << failures.saturating_sub(1).min(5)).min(Duration::from_secs(30))
}

/// A session that lasted this long counts as healthy: the backoff restarts.
const HEALTHY: Duration = Duration::from_secs(60);

/// Runs `session` forever, reconnecting with backoff whenever it ends (EOF,
/// error, compositor restart) so the loop never spins.
pub async fn retry<F, Fut>(name: &'static str, mut session: F) -> anyhow::Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let mut failures = 0u32;
    loop {
        let started = Instant::now();
        match session().await {
            Ok(()) => tracing::info!("{name} connection closed"),
            Err(e) if failures == 0 => tracing::warn!("{name} focus tracking interrupted: {e:#}"),
            Err(e) => tracing::debug!("{name}: {e:#}"),
        }
        failures = if started.elapsed() >= HEALTHY {
            1
        } else {
            failures.saturating_add(1)
        };
        tokio::time::sleep(backoff(failures)).await;
    }
}

/// Reports a window pid as the foreground; `None` (unknown window) is ignored.
pub fn report(shared: &Shared, pid: Option<u32>) {
    shared.set_foreground(pid);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        let secs: Vec<u64> = (1..=8).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 16, 30, 30, 30]);
        assert_eq!(backoff(0), Duration::from_secs(1));
        assert_eq!(backoff(u32::MAX), Duration::from_secs(30));
    }
}
