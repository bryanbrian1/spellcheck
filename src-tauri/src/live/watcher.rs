//! The poll loop, and the reason it is affordable.
//!
//! This is the one place in the app that polls something on a timer, and the
//! blueprint is blunt about why that is dangerous: a loop here wakes the CPU
//! while the game is drawing frames, and a helper that stutters the game it is
//! helping has failed. Low-end machines feel it first.
//!
//! Three decisions keep the cost near zero.
//!
//! **It does not run while the launcher is closed.** Not slowly — at all. The
//! loop waits on a channel fed by the champ select watcher, so for the
//! twenty-three hours a day nobody is playing, this task is asleep on a
//! channel and costs exactly nothing. That is the single biggest saving
//! available, and it is free because the LCU layer already knows.
//!
//! **It looks rarely once a game is underway.** Half a minute, not a second.
//! Nothing this check reads changes faster than that: item purchases happen
//! on a trip to the shop, and levels arrive a few times a game.
//!
//! **It looks more often while the player is dead.** That is the one window
//! where the answer both changes and matters — you are about to respawn and
//! spend — and it is also the one window where the player is looking at a
//! grey screen rather than at the game. Being busier exactly then is the
//! opposite of the mistake this module is built to avoid.

use std::time::Duration;

use tokio::sync::{mpsc, watch};

use super::client::LiveClient;
use super::game::GameSnapshot;

/// While the launcher is open but no game is running. A refused connection on
/// loopback is cheap — no TLS handshake, no wakeup of anything outside this
/// process — but it is not free, so this is deliberately unhurried.
const DEFAULT_IDLE_SECS: u64 = 10;

/// While a game is in progress. See the module docs: nothing read here moves
/// faster than a trip to the shop.
const DEFAULT_IN_GAME_SECS: u64 = 30;

/// While the player is dead, and therefore both about to spend gold and not
/// currently watching the game render.
const DEFAULT_DEAD_SECS: u64 = 5;

#[derive(Debug, Clone)]
pub struct LiveWatcherConfig {
    pub base_url: String,
    pub idle_interval: Duration,
    pub in_game_interval: Duration,
    pub dead_interval: Duration,
}

impl Default for LiveWatcherConfig {
    fn default() -> Self {
        LiveWatcherConfig {
            base_url: format!("https://127.0.0.1:{}", super::LIVE_CLIENT_PORT),
            idle_interval: Duration::from_secs(DEFAULT_IDLE_SECS),
            in_game_interval: Duration::from_secs(DEFAULT_IN_GAME_SECS),
            dead_interval: Duration::from_secs(DEFAULT_DEAD_SECS),
        }
    }
}

/// What the live watcher reports.
#[derive(Debug, Clone, PartialEq)]
pub enum GameEvent {
    /// No game is running. Sent once when that becomes true, not on every
    /// look — otherwise the ordinary state would be the noisiest one.
    NoGame,
    /// One reading of a game in progress. Sent on every poll; deciding
    /// whether anything in it actually changed is the consumer's job, because
    /// only the consumer knows which fields it renders.
    Snapshot(Box<GameSnapshot>),
}

/// What the loop last told anyone, so the quiet state stays quiet.
#[derive(Debug, Default, PartialEq, Clone, Copy)]
enum Reported {
    #[default]
    Nothing,
    InGame,
    NoGame,
}

#[derive(Debug, Default)]
struct WatchState {
    reported: Reported,
}

impl WatchState {
    fn observe(&mut self, found: Option<GameSnapshot>) -> Option<GameEvent> {
        match found {
            Some(snapshot) => {
                self.reported = Reported::InGame;
                Some(GameEvent::Snapshot(Box::new(snapshot)))
            }
            // "No game" is worth saying when it becomes true and worth saying
            // once at startup, so the screen can render something. Saying it
            // every ten seconds forever would be the same news on a timer.
            None => match self.reported {
                Reported::NoGame => None,
                _ => {
                    self.reported = Reported::NoGame;
                    Some(GameEvent::NoGame)
                }
            },
        }
    }

    /// The gap before the next look.
    fn interval(&self, config: &LiveWatcherConfig, found: Option<&GameSnapshot>) -> Duration {
        match found {
            Some(snapshot) if snapshot.local_player().is_some_and(|player| player.is_dead) => {
                config.dead_interval
            }
            Some(_) => config.in_game_interval,
            None => config.idle_interval,
        }
    }
}

/// Watch for a game, and report what is in it.
///
/// `launcher_running` is the gate: while it holds `false` this task performs
/// no work at all and waits for it to change. The champ select watcher is
/// what flips it, which is why the two layers are wired together in
/// [`crate::spawn_champ_select`] rather than each polling for itself.
///
/// Returns when either channel closes, which is how the app shuts it down.
pub async fn watch(
    config: LiveWatcherConfig,
    mut launcher_running: watch::Receiver<bool>,
    events: mpsc::Sender<GameEvent>,
) {
    let client = LiveClient::at(&config.base_url);
    let mut state = WatchState::default();

    loop {
        // Asleep on a channel, not a timer. This is the state the app is in
        // almost all of the time it is open.
        while !*launcher_running.borrow_and_update() {
            if state.reported == Reported::InGame {
                state.reported = Reported::NoGame;
                if events.send(GameEvent::NoGame).await.is_err() {
                    return;
                }
            }
            if launcher_running.changed().await.is_err() {
                return;
            }
        }

        let found = match client.game().await {
            Ok(found) => found,
            // A shape we cannot read is worth a line, because unlike a closed
            // port it means something we believed about the payload is no
            // longer true. It is not worth stopping over: the next look
            // usually succeeds, and the game is still running either way.
            Err(error) => {
                eprintln!("leaguechecker: {error}");
                None
            }
        };

        let interval = state.interval(&config, found.as_ref());
        if let Some(event) = state.observe(found) {
            if events.send(event).await.is_err() {
                return;
            }
        }

        // Wake early if the launcher closes, so quitting League stops the
        // polling immediately rather than up to half a minute later.
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            changed = launcher_running.changed() => {
                if changed.is_err() {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snapshot(dead: bool) -> GameSnapshot {
        GameSnapshot::from_json(&json!({
            "activePlayer": { "riotId": "Ahri#EUW" },
            "allPlayers": [{
                "championName": "Ahri", "team": "ORDER", "position": "MIDDLE",
                "riotId": "Ahri#EUW", "isDead": dead, "items": [], "scores": {},
            }],
            "gameData": { "gameTime": 600.0 },
        }))
        .unwrap()
    }

    #[test]
    fn no_game_is_said_once_rather_than_every_ten_seconds() {
        let mut state = WatchState::default();

        // First look of the app's life: the screen needs to be told.
        assert_eq!(state.observe(None), Some(GameEvent::NoGame));
        // And then nothing, for as long as that stays true.
        assert_eq!(state.observe(None), None);
        assert_eq!(state.observe(None), None);
    }

    #[test]
    fn the_end_of_a_game_is_reported_even_after_the_quiet_state_was() {
        let mut state = WatchState::default();
        state.observe(None);

        assert!(matches!(
            state.observe(Some(snapshot(false))),
            Some(GameEvent::Snapshot(_))
        ));
        assert_eq!(
            state.observe(None),
            Some(GameEvent::NoGame),
            "the game ending is news even though we had said 'no game' before it started"
        );
    }

    #[test]
    fn every_look_at_a_running_game_is_reported() {
        let mut state = WatchState::default();
        // Deciding whether anything changed belongs to the consumer, which is
        // the only part that knows which fields it draws.
        for _ in 0..3 {
            assert!(matches!(
                state.observe(Some(snapshot(false))),
                Some(GameEvent::Snapshot(_))
            ));
        }
    }

    #[test]
    fn the_loop_slows_right_down_once_a_game_is_running() {
        let config = LiveWatcherConfig::default();
        let state = WatchState::default();

        let alive = snapshot(false);
        assert_eq!(
            state.interval(&config, Some(&alive)),
            Duration::from_secs(30),
            "polling a running game faster than this is the mistake the \
             blueprint names as the most expensive one available"
        );
        assert!(state.interval(&config, Some(&alive)) >= config.idle_interval);
    }

    #[test]
    fn a_dead_player_is_checked_more_often_than_a_living_one() {
        let config = LiveWatcherConfig::default();
        let state = WatchState::default();

        let dead = snapshot(true);
        let alive = snapshot(false);
        assert!(
            state.interval(&config, Some(&dead)) < state.interval(&config, Some(&alive)),
            "being dead is the one window where the answer both changes and \
             the player is not watching the game render"
        );
    }

    #[test]
    fn with_no_game_the_loop_waits_the_idle_gap() {
        let config = LiveWatcherConfig::default();
        let state = WatchState::default();
        assert_eq!(state.interval(&config, None), config.idle_interval);
    }

    #[tokio::test]
    async fn a_closed_launcher_produces_no_traffic_and_no_events() {
        let (_running_tx, running_rx) = watch::channel(false);
        let (events_tx, mut events_rx) = mpsc::channel(8);

        // Port 1 has nothing on it. If the gate leaked, the loop would poll
        // it and report "no game"; instead it must never look at all.
        let config = LiveWatcherConfig {
            base_url: "https://127.0.0.1:1".to_string(),
            idle_interval: Duration::from_millis(1),
            ..LiveWatcherConfig::default()
        };

        let task = tokio::spawn(watch(config, running_rx, events_tx));
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert!(
            events_rx.try_recv().is_err(),
            "the watcher worked while the launcher was closed"
        );
        task.abort();
    }

    #[tokio::test]
    async fn opening_the_launcher_wakes_it_and_it_reports_no_game() {
        let (running_tx, running_rx) = watch::channel(false);
        let (events_tx, mut events_rx) = mpsc::channel(8);

        let config = LiveWatcherConfig {
            base_url: "https://127.0.0.1:1".to_string(),
            idle_interval: Duration::from_millis(5),
            ..LiveWatcherConfig::default()
        };

        let task = tokio::spawn(watch(config, running_rx, events_tx));
        running_tx.send(true).expect("the receiver is alive");

        let event = tokio::time::timeout(Duration::from_secs(2), events_rx.recv())
            .await
            .expect("the watcher woke up");
        assert_eq!(event, Some(GameEvent::NoGame));
        task.abort();
    }
}
