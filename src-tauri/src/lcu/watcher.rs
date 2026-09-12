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

use std::time::Duration;

use serde::Serialize;
use tokio::sync::mpsc;

use super::client::LcuClient;
use super::error::LcuError;
use super::lockfile::{Lockfile, LockfileSearch};
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
    /// Where to look for the client. Re-asked on every check, so a client
    /// installed while the app is open is found without a relaunch.
    pub lockfile: LockfileSearch,
    pub client_check_interval: Duration,
    pub reconnect_delay: Duration,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        WatcherConfig {
            lockfile: LockfileSearch::default(),
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
    ///
    /// Carries where the app looked, because "offline" is also what a client
    /// installed somewhere this app does not know about looks like, and the
    /// only way the player can tell the two apart is to be shown the paths.
    ClientOffline { searched: Vec<String> },
    /// A lockfile exists but cannot be read — a permissions problem, most
    /// likely. The client is running; this app cannot reach it. Its own
    /// state because on screen it would otherwise be identical to offline,
    /// and unlike offline it is something the player can fix.
    LockfileUnreadable { path: String, detail: String },
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
    /// Champ select is running and the payload no longer makes sense.
    ///
    /// Its own state rather than a variant of "nothing locked yet", because
    /// the two are indistinguishable from the outside and only one of them
    /// is the user's fault. The LCU is an internal client API with no
    /// versioning promise, so this is what a patch that renames a field
    /// looks like from in here.
    Unreadable { reason: &'static str },
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
    // What was last said while no client was found, so it is said once. The
    // paths are part of it: a config change that adds one is worth repeating.
    let mut reported: Option<ChampSelectEvent> = None;

    loop {
        let candidates = config.lockfile.candidates();
        match Lockfile::read_first(&candidates) {
            Ok(Some(lockfile)) => {
                reported = None;
                if let Err(error) = follow_client(&lockfile, &events).await {
                    if !events.is_closed() {
                        eprintln!("spellcheck: {error}");
                    }
                }
                if events.is_closed() {
                    return;
                }
                tokio::time::sleep(config.reconnect_delay).await;
            }
            Ok(None) => {
                // The ordinary case. Say so once, then stay quiet.
                let event = ChampSelectEvent::ClientOffline {
                    searched: candidates.iter().map(|p| p.display().to_string()).collect(),
                };
                if !say_once(&mut reported, event, &events).await {
                    return;
                }
                tokio::time::sleep(config.client_check_interval).await;
            }
            Err(error) => {
                // A half-written lockfile is retryable and the next check
                // reads it whole. Anything else is a file that exists and
                // cannot be used, which the player needs to be told about.
                match &error {
                    LcuError::LockfileUnreadable { path, detail } if !error.is_retryable() => {
                        let event = ChampSelectEvent::LockfileUnreadable {
                            path: path.clone(),
                            detail: detail.clone(),
                        };
                        if !say_once(&mut reported, event, &events).await {
                            return;
                        }
                    }
                    _ if !error.is_retryable() => eprintln!("spellcheck: {error}"),
                    _ => {}
                }
                tokio::time::sleep(config.client_check_interval).await;
            }
        }

        if events.is_closed() {
            return;
        }
    }
}

/// Send `event` unless it is what was last sent. Returns false when the
/// receiver is gone, which is the watcher's signal to stop.
async fn say_once(
    last: &mut Option<ChampSelectEvent>,
    event: ChampSelectEvent,
    events: &mpsc::Sender<ChampSelectEvent>,
) -> bool {
    if last.as_ref() == Some(&event) {
        return true;
    }
    let sent = events.send(event.clone()).await.is_ok();
    *last = Some(event);
    sent
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
            Err(error) => eprintln!("spellcheck: {error}"),
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
            Change::Unreadable(reason) => {
                // Logged as well as sent: the screen tells the user their app
                // is confused, and the terminal tells whoever is debugging
                // which of the two shapes broke.
                eprintln!("spellcheck: unreadable champ select — {reason}");
                ChampSelectEvent::Unreadable { reason }
            }
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
                            "spellcheck: the client has no champion {}",
                            selection.champion_id
                        );
                        continue;
                    }
                    Err(error) => {
                        state.forget();
                        eprintln!("spellcheck: {error}");
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
    Unreadable(&'static str),
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
    /// Reported once per champ select. The client resends the session on
    /// every hover and every tick, and a broken shape stays broken.
    unreadable: bool,
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

        // Before reading anything out of it: a payload that contradicts
        // itself cannot be trusted to be merely empty.
        match session.unreadable() {
            Some(reason) => {
                if !self.unreadable {
                    self.unreadable = true;
                    changes.push(Change::Unreadable(reason));
                }
                return changes;
            }
            // A session that reads again after a bad one is worth hearing
            // about, so the next break is reported too.
            None => self.unreadable = false,
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
    use std::path::PathBuf;
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
            serde_json::to_value(ChampSelectEvent::ClientOffline {
                searched: vec!["/x/lockfile".to_string()]
            })
            .unwrap(),
            json!({ "event": "clientOffline", "searched": ["/x/lockfile"] })
        );
        assert_eq!(
            serde_json::to_value(ChampSelectEvent::LockfileUnreadable {
                path: "/x/lockfile".to_string(),
                detail: "permission denied".to_string(),
            })
            .unwrap(),
            json!({
                "event": "lockfileUnreadable",
                "path": "/x/lockfile",
                "detail": "permission denied",
            })
        );
        assert_eq!(
            serde_json::to_value(ChampSelectEvent::Entered).unwrap(),
            json!({ "event": "entered" })
        );
        // A struct variant rather than a newtype one, because an internally
        // tagged enum cannot serialise a variant holding a bare string — it
        // returns an error instead, which would have made the one event that
        // exists to break a silence fail silently itself.
        assert_eq!(
            serde_json::to_value(ChampSelectEvent::Unreadable { reason: "no team" }).unwrap(),
            json!({ "event": "unreadable", "reason": "no team" })
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
        let missing = "/nonexistent/League of Legends.app/lockfile";
        let config = WatcherConfig {
            lockfile: LockfileSearch::Only(vec![PathBuf::from(missing)]),
            client_check_interval: Duration::from_millis(5),
            reconnect_delay: Duration::from_millis(5),
        };

        let watcher = tokio::spawn(watch(config, sender));

        // The report names where it looked, so an install this app does not
        // know about can be told apart from a closed client.
        assert_eq!(
            receiver.recv().await,
            Some(ChampSelectEvent::ClientOffline {
                searched: vec![missing.to_string()]
            })
        );

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
