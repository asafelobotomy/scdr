//! Bookkeeping for what has been delivered to scdrd, so a failed D-Bus call
//! is retried instead of being assumed delivered.

use super::State;
use std::collections::BTreeSet;

/// What scdrd is known to have received. `None` means unknown, so it is sent.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Sent {
    pub foreground: Option<u32>,
    pub audio: Option<BTreeSet<u32>>,
    pub games: Option<BTreeSet<u32>>,
}

/// The calls still owed to scdrd.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Plan {
    pub foreground: Option<u32>,
    pub audio: Option<Vec<u32>>,
    pub games: Option<Vec<u32>>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        *self == Plan::default()
    }
}

/// Fields of `now` that differ from what `sent` records. While the screen is
/// locked the foreground is 0: no app is in front, so none is boosted.
pub fn plan(now: &State, sent: &Sent) -> Plan {
    let foreground = if now.locked { Some(0) } else { now.foreground };
    Plan {
        foreground: foreground.filter(|_| sent.foreground != foreground),
        audio: (sent.audio.as_ref() != Some(&now.audio))
            .then(|| now.audio.iter().copied().collect()),
        games: (sent.games.as_ref() != Some(&now.games))
            .then(|| now.games.iter().copied().collect()),
    }
}

impl Sent {
    pub fn mark_foreground(&mut self, pid: u32) {
        self.foreground = Some(pid);
    }
    pub fn mark_audio(&mut self, pids: &[u32]) {
        self.audio = Some(pids.iter().copied().collect());
    }
    pub fn mark_games(&mut self, pids: &[u32]) {
        self.games = Some(pids.iter().copied().collect());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(fg: Option<u32>, audio: &[u32], games: &[u32]) -> State {
        State {
            foreground: fg,
            audio: audio.iter().copied().collect(),
            games: games.iter().copied().collect(),
            locked: false,
        }
    }

    #[test]
    fn locking_clears_the_foreground_and_unlocking_restores_it() {
        let mut now = state(Some(5), &[], &[]);
        let mut sent = Sent::default();
        sent.mark_foreground(5);
        sent.mark_audio(&[]);
        sent.mark_games(&[]);
        now.locked = true;
        assert_eq!(plan(&now, &sent).foreground, Some(0));
        sent.mark_foreground(0);
        assert!(plan(&now, &sent).is_empty());
        now.locked = false;
        assert_eq!(plan(&now, &sent).foreground, Some(5));
    }

    #[test]
    fn fresh_sent_wants_everything() {
        let p = plan(&state(Some(5), &[], &[]), &Sent::default());
        assert_eq!(p.foreground, Some(5));
        assert_eq!(p.audio, Some(vec![]));
        assert_eq!(p.games, Some(vec![]));
    }

    #[test]
    fn no_foreground_is_never_sent() {
        let p = plan(&state(None, &[], &[]), &Sent::default());
        assert_eq!(p.foreground, None);
    }

    #[test]
    fn marking_only_what_succeeded_keeps_the_rest_pending() {
        let now = state(Some(5), &[1, 2], &[9]);
        let mut sent = Sent::default();
        let p = plan(&now, &sent);
        // Only the foreground call succeeded.
        sent.mark_foreground(p.foreground.unwrap());
        let retry = plan(&now, &sent);
        assert_eq!(retry.foreground, None);
        assert_eq!(retry.audio, Some(vec![1, 2]));
        assert_eq!(retry.games, Some(vec![9]));
        sent.mark_audio(&[1, 2]);
        sent.mark_games(&[9]);
        assert!(plan(&now, &sent).is_empty());
    }

    #[test]
    fn changes_after_success_are_detected() {
        let mut sent = Sent::default();
        sent.mark_foreground(5);
        sent.mark_audio(&[1]);
        sent.mark_games(&[]);
        let p = plan(&state(Some(6), &[1], &[3]), &sent);
        assert_eq!(p.foreground, Some(6));
        assert_eq!(p.audio, None);
        assert_eq!(p.games, Some(vec![3]));
    }
}
