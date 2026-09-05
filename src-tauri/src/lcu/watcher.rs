//! The loop that turns a running client into "you locked Ahri mid".
//!
//! It owns the one honest compromise in this layer. Champ select itself is
//! never polled — that arrives on the socket — but *something* has to notice
//! that the client has started, and the client does not announce itself to a
//! process that is not connected to it yet. So while nothing is running we
//! stat one file every few seconds, and the moment it exists we stop doing
//! even that and let the socket do the work. Statting a path is not the loop
//! the rule is about: it costs no wakeup of the game, no TLS handshake, and
//! it happens only while no game exists.
//!
//! Everything the watcher reports is a state, including the one it reports
//! most: the client is closed.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::mpsc;

use super::client::LcuClient;
use super::error::LcuError;
use super::lockfile::{default_lockfile_path, Lockfile};
use super::session::{ChampSelectSession, Comp, Selection, CHAMP_SELECT_SESSION_URI};
use super::ws::LcuEventStream;

/// How often we look for the lockfile while the client is closed. Slow on
/// purpose: nobody starts League and expects a result in the same second, and
/// this is the cost the app pays for the twenty-three hours a day it is
/// waiting.
const DEFAULT_CLIENT_CHECK_SECS: u64 = 5;

/// How long to wait before reconnecting after the socket drops. The usual
/// cause is the client quitting, in which case the next lockfile read finds
/// nothing and we go back to waiting anyway.
const DEFAULT_RECONNECT_SECS: u64 = 2;

#[derive(Debug, Clone)]
pub struct WatcherConfig {
    pub lockfile_path: PathBuf,
    pub client_check_interval: Duration,
    pub reconnect_delay: Duration,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        WatcherConfig {
            lockfile_path: default_lockfile_path(),
            client_check_interval: Duration::from_secs(DEFAULT_CLIENT_CHECK_SECS),
            reconnect_delay: Duration::from_secs(DEFAULT_RECONNECT_SECS),
        }
    }
}

/// A champion locked in a role, with the id already resolved to the key a
/// build provider looks up by. This is the layer's output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedChampion {
    pub champion_id: u32,
    pub champion_key: String,
    /// The client's own `assignedPosition`, passed through untranslated.
    pub assigned_position: String,
}

/// What the watcher tells the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum ChampSelectEvent {
    /// No client. The default state of the machine, and the first thing the
    /// app hears on almost every launch.
    ClientOffline,
    /// Connected and listening, but not in champ select.
    ClientConnected,
    /// Champ select opened. Nothing is locked yet.
    Entered,
    /// A champion is locked in a role we can look a build up for.
    Locked(LockedChampion),
    /// The champions visible on both sides changed.
    ///
    /// Deliberately separate from `Locked`. Nine other people lock in during
    /// a champ select, and folding their picks into the lock event would make
    /// every one of them look like a new champion for us and fire a fresh
    /// build lookup. This carries no build: it is what the recommendation
    /// engine reads, and reasoning over it costs nothing but a few map
    /// lookups.
    CompChanged(Comp),
    /// Champ select ended — dodged, declined, or the game started.
    Left,
}

/// Watch the client until the receiver goes away.
///
/// Returns when the event channel closes, which is how the app shuts this
/// down: drop the receiver. Every error inside is logged and retried, because
/// the failures available here — client quit, socket dropped, lockfile caught
/// mid-write — are all cured by waiting.
pub async fn watch(config: WatcherConfig, events: mpsc::Sender<ChampSelectEvent>) {
    let mut reported_offline = false;

    loop {
        match Lockfile::read(&config.lockfile_path) {
            Ok(Some(lockfile)) => {
                reported_offline = false;
                if let Err(error) = follow_client(&lockfile, &events).await {
                    if !events.is_closed() {
                        eprintln!("leaguechecker: {error}");
                    }
                }
                if events.is_closed() {
                    return;
                }
                tokio::time::sleep(config.reconnect_delay).await;
            }
            Ok(None) => {
                // The ordinary case. Say so once, then stay quiet.
                if !reported_offline {
                    reported_offline = true;
                    if events.send(ChampSelectEvent::ClientOffline).await.is_err() {
                        return;
                    }
                }
                tokio::time::sleep(config.client_check_interval).await;
            }
            Err(error) => {
                if !error.is_retryable() {
                    eprintln!("leaguechecker: {error}");
                }
                tokio::time::sleep(config.client_check_interval).await;
            }
        }

        if events.is_closed() {
            return;
        }
    }
}

/// One connection's lifetime: connect, catch up, then listen until the client
/// goes away.
async fn follow_client(
    lockfile: &Lockfile,
    events: &mpsc::Sender<ChampSelectEvent>,
) -> Result<(), LcuError> {
    let client = LcuClient::new(lockfile)?;
    let mut stream = LcuEventStream::connect(lockfile).await?;

    if events.send(ChampSelectEvent::ClientConnected).await.is_err() {
        stream.close().await;
        return Ok(());
    }

    let mut state = ChampSelectState::default();

    // The socket carries changes, not the present. Without this read, joining
    // a champ select already in progress would show nothing until the next
    // update — and the update after a lock may never come.
    if let Some(session) = client.champ_select_session().await? {
        if !emit(&client, &mut state, &session, events).await {
            stream.close().await;
            return Ok(());
        }
    }

    while let Some(event) = stream.next_event().await? {
        if event.uri != CHAMP_SELECT_SESSION_URI {
            continue;
        }

        if event.is_delete() {
            if let Some(left) = state.ended() {
                if events.send(left).await.is_err() {
                    break;
                }
            }
            continue;
        }

        match ChampSelectSession::from_json(&event.data) {
            // A payload we cannot read is worth a line in the log, but it is
            // not worth dropping the connection: the next update usually
            // parses, and reconnecting would lose champ select entirely.
            Err(error) => eprintln!("leaguechecker: {error}"),
            Ok(session) => {
                if !emit(&client, &mut state, &session, events).await {
                    break;
                }
            }
        }
    }

    Ok(())
}

/// Send everything one session update implies. Returns false once the
/// receiver is gone.
async fn emit(
    client: &LcuClient,
    state: &mut ChampSelectState,
    session: &ChampSelectSession,
    events: &mpsc::Sender<ChampSelectEvent>,
) -> bool {
    for change in state.observe(session) {
        let event = match change {
            Change::Entered => ChampSelectEvent::Entered,
            // No id to resolve and no lookup to make: the ids go out as they
            // arrived and the engine reads them against its own tag file.
            Change::CompChanged(comp) => ChampSelectEvent::CompChanged(comp),
            Change::Locked(selection) => {
                match client.champion_key(selection.champion_id).await {
                    Ok(Some(champion_key)) => ChampSelectEvent::Locked(LockedChampion {
                        champion_id: selection.champion_id,
                        champion_key,
                        assigned_position: selection.assigned_position,
                    }),
                    // The client knows every champion it can put in champ
                    // select, so both of these mean the client went away
                    // mid-lookup. Forget the lock so a reconnect re-reports
                    // it rather than deduplicating it into silence.
                    Ok(None) => {
                        state.forget();
                        eprintln!(
                            "leaguechecker: the client has no champion {}",
                            selection.champion_id
                        );
                        continue;
                    }
                    Err(error) => {
                        state.forget();
                        eprintln!("leaguechecker: {error}");
                        continue;
                    }
                }
            }
        };

        if events.send(event).await.is_err() {
            return false;
        }
    }
    true
}

/// What changed between two session payloads.
#[derive(Debug, PartialEq, Eq)]
enum Change {
    Entered,
    Locked(Selection),
    CompChanged(Comp),
}

/// Champ select sends an update for everything: every ban, every hover, every
/// tick of the timer. This remembers just enough to tell a real change from
/// the same news arriving again — without it, one champ select would trigger
/// dozens of identical build lookups.
#[derive(Debug, Default)]
struct ChampSelectState {
    entered: bool,
    locked: Option<Selection>,
    comp: Option<Comp>,
}

impl ChampSelectState {
    fn observe(&mut self, session: &ChampSelectSession) -> Vec<Change> {
        let mut changes = Vec::new();

        if !self.entered {
            self.entered = true;
            changes.push(Change::Entered);
        }

        if let Some(selection) = session.selection() {
            if self.locked.as_ref() != Some(&selection) {
                self.locked = Some(selection.clone());
                changes.push(Change::Locked(selection));
            }
        }

        // Champ select sends an update for every hover, every timer tick and
        // every trade offer. Only a change in who is actually locked in is
        // worth telling anyone about.
        let comp = session.comp();
        if !comp.is_empty() && self.comp.as_ref() != Some(&comp) {
            self.comp = Some(comp.clone());
            changes.push(Change::CompChanged(comp));
        }

        changes
    }

    /// Champ select closed. Reports `Left` only if we ever reported being in
    /// it, so a Delete arriving for a session we never saw stays quiet.
    fn ended(&mut self) -> Option<ChampSelectEvent> {
        let was_in = std::mem::take(self);
        was_in.entered.then_some(ChampSelectEvent::Left)
    }

    /// Drop the remembered lock so the next identical update is treated as
    /// new. Used when a lookup failed and the news still needs delivering.
    fn forget(&mut self) {
        self.locked = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session(champion_id: u32, position: &str) -> ChampSelectSession {
        ChampSelectSession::from_json(&json!({
            "localPlayerCellId": 0,
            "myTeam": [
                { "cellId": 0, "championId": champion_id, "assignedPosition": position },
            ],
        }))
        .unwrap()
    }

    #[test]
    fn reports_entering_once_and_then_the_lock() {
        let mut state = ChampSelectState::default();

        // Champ select opens; nothing picked yet.
        assert_eq!(state.observe(&session(0, "middle")), vec![Change::Entered]);
        // Still nothing picked, several updates later.
        assert!(state.observe(&session(0, "middle")).is_empty());

        // The lock, and the composition it changed. Both go out, and they
        // are separate changes on purpose — only the first costs a lookup.
        let changes = state.observe(&session(103, "middle"));
        assert_eq!(
            changes,
            vec![
                Change::Locked(Selection {
                    champion_id: 103,
                    assigned_position: "middle".to_string(),
                }),
                Change::CompChanged(Comp {
                    ally: vec![103],
                    enemy: vec![],
                }),
            ]
        );
    }

    #[test]
    fn the_same_lock_arriving_again_is_not_a_new_lookup() {
        let mut state = ChampSelectState::default();
        state.observe(&session(103, "middle"));
        assert!(state.observe(&session(103, "middle")).is_empty());
        assert!(state.observe(&session(103, "middle")).is_empty());
    }

    #[test]
    fn a_swap_after_locking_is_a_new_lookup() {
        let mut state = ChampSelectState::default();
        state.observe(&session(103, "middle"));

        let changes = state.observe(&session(64, "jungle"));
        assert_eq!(
            changes,
            vec![
                Change::Locked(Selection {
                    champion_id: 64,
                    assigned_position: "jungle".to_string(),
                }),
                Change::CompChanged(Comp {
                    ally: vec![64],
                    enemy: vec![],
                }),
            ]
        );
    }

    /// The reason `CompChanged` exists at all: the other nine seats fill in
    /// one at a time, and none of them may cost a build lookup.
    #[test]
    fn an_enemy_locking_in_moves_the_composition_and_nothing_else() {
        let mut state = ChampSelectState::default();

        let with_enemies = |enemy: Vec<u32>| {
            ChampSelectSession::from_json(&json!({
                "localPlayerCellId": 0,
                "myTeam": [{ "cellId": 0, "championId": 103, "assignedPosition": "middle" }],
                "theirTeam": enemy
                    .iter()
                    .map(|id| json!({ "championId": id }))
                    .collect::<Vec<_>>(),
            }))
            .unwrap()
        };

        state.observe(&with_enemies(vec![0, 0]));

        // One enemy locks. Our champion did not change, so no lookup may fire.
        let changes = state.observe(&with_enemies(vec![266, 0]));
        assert_eq!(
            changes,
            vec![Change::CompChanged(Comp {
                ally: vec![103],
                enemy: vec![266, 0],
            })]
        );
        assert!(
            !changes.iter().any(|c| matches!(c, Change::Locked(_))),
            "an enemy pick triggered a build lookup for our own champion"
        );

        // The same session again is silence, as everything else here is.
        assert!(state.observe(&with_enemies(vec![266, 0])).is_empty());
    }

    #[test]
    fn a_failed_lookup_is_retried_rather_than_deduplicated_away() {
        let mut state = ChampSelectState::default();
        state.observe(&session(103, "middle"));
        state.forget();

        assert_eq!(
            state.observe(&session(103, "middle")),
            vec![Change::Locked(Selection {
                champion_id: 103,
                assigned_position: "middle".to_string(),
            })]
        );
    }

    #[test]
    fn leaving_resets_everything() {
        let mut state = ChampSelectState::default();
        state.observe(&session(103, "middle"));

        assert_eq!(state.ended(), Some(ChampSelectEvent::Left));
        // A second Delete, or one for a session we never saw, says nothing.
        assert_eq!(state.ended(), None);

        // The next champ select is reported from scratch.
        assert_eq!(state.observe(&session(0, "top")), vec![Change::Entered]);
    }

    /// The UI's `LcuStatus` type in main.ts is hand-written against this
    /// shape. A rename on either side is silent at compile time and shows up
    /// as a champ select screen that never updates, so the contract is
    /// asserted here rather than discovered in a game.
    #[test]
    fn the_wire_shape_the_ui_reads_is_fixed() {
        assert_eq!(
            serde_json::to_value(ChampSelectEvent::ClientOffline).unwrap(),
            json!({ "event": "clientOffline" })
        );
        assert_eq!(
            serde_json::to_value(ChampSelectEvent::Entered).unwrap(),
            json!({ "event": "entered" })
        );
        assert_eq!(
            serde_json::to_value(ChampSelectEvent::Locked(LockedChampion {
                champion_id: 62,
                champion_key: "MonkeyKing".to_string(),
                assigned_position: "jungle".to_string(),
            }))
            .unwrap(),
            json!({
                "event": "locked",
                "championId": 62,
                "championKey": "MonkeyKing",
                "assignedPosition": "jungle",
            })
        );
    }

    #[tokio::test]
    async fn a_closed_client_is_reported_once_and_is_not_an_error() {
        let (sender, mut receiver) = mpsc::channel(4);
        let config = WatcherConfig {
            lockfile_path: PathBuf::from("/nonexistent/League of Legends.app/lockfile"),
            client_check_interval: Duration::from_millis(5),
            reconnect_delay: Duration::from_millis(5),
        };

        let watcher = tokio::spawn(watch(config, sender));

        assert_eq!(receiver.recv().await, Some(ChampSelectEvent::ClientOffline));

        // Several check intervals later, still nothing further to say.
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(receiver.try_recv().is_err());

        // Dropping the receiver is how the app stops the watcher.
        drop(receiver);
        tokio::time::timeout(Duration::from_secs(2), watcher)
            .await
            .expect("the watcher stops when nobody is listening")
            .unwrap();
    }
}
