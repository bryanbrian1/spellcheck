//! leaguechecker core.
//!
//! The build data layer, the two layers that watch the League client and the
//! game it launches, and the Tauri shell that hosts them. They meet at
//! [`BuildService::build_for`], which three routes now call: the search box
//! when the user types a champion, [`spawn_champ_select`] when the client
//! says one was locked, and [`spawn_live_game`] when a game turns out to be
//! running that we never saw the champ select for. None of the three knows
//! about the others; a [`BuildClaim`] is all that keeps the last two from
//! asking for the same build twice.

pub mod build_data;
pub mod commands;
pub mod ddragon;
pub mod lcu;
pub mod live;
pub mod recommend;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

use build_data::config::CONFIG_FILE_NAME;
use ddragon::DataDragonState;
use lcu::session::Comp;
use lcu::{ChampSelectEvent, WatcherConfig};
use live::{GameEvent, GameSnapshot, LiveWatcherConfig};
use recommend::{
    enemy_threat, game_state, standing, team_gaps, Standing, Suggestion, Tags, TeamView,
};

pub use build_data::{
    BuildDataProvider, BuildLookup, BuildRequest, ChampionBuild, ProviderConfig, ProviderError,
    ProviderKind, Role,
};

/// The single entry point the UI layer talks to.
///
/// It holds whichever provider the config selected and exposes exactly one
/// operation. Commands call this; they never name a provider, and neither does
/// the frontend — swapping OP.GG for our own crawled data is a config change.
pub struct BuildService {
    provider: Arc<dyn BuildDataProvider>,
}

impl BuildService {
    pub fn from_config(config: &ProviderConfig) -> Result<BuildService, ProviderError> {
        Ok(BuildService {
            provider: config.active_provider()?,
        })
    }

    pub fn new(provider: Arc<dyn BuildDataProvider>) -> BuildService {
        BuildService { provider }
    }

    /// Attribution string for the active source. The UI renders this; it must
    /// not branch on the value.
    pub fn source_label(&self) -> &str {
        self.provider.label()
    }

    pub async fn build(&self, request: &BuildRequest) -> Result<BuildLookup, ProviderError> {
        self.provider.fetch_build(request).await
    }

    /// Champ-select shaped entry point: the LCU hands us a champion key and an
    /// `assignedPosition` string.
    pub async fn build_for(
        &self,
        champion_key: &str,
        assigned_position: &str,
        champion_id: Option<u32>,
    ) -> Result<BuildLookup, ProviderError> {
        let role = Role::parse(assigned_position)?;
        let mut request = BuildRequest::new(champion_key, role);
        request.champion_id = champion_id;
        self.build(&request).await
    }
}

/// Where the client's champ select state reaches the UI: `clientOffline`,
/// `clientConnected`, `entered`, `locked`, `left`.
pub const CHAMP_SELECT_STATUS_EVENT: &str = "lcu:status";

/// Where the build for the champion we are on reaches the UI.
///
/// Deliberately not champ-select specific. The same event carries a build
/// fetched because the client said we locked in and one fetched because a
/// game turned out to be running, because the screen that draws them is one
/// screen. Which route found it is not something the UI can act on, so it is
/// not something the UI is told.
pub const LIVE_BUILD_EVENT: &str = "live:build";

/// Where the rule-based checks reach the UI.
///
/// Separate from the build event because the two have different lifetimes: a
/// build is fetched once when you lock in, while the suggestions change every
/// time one of the other nine players picks. Sending them together would mean
/// either re-fetching the build nine times or holding the suggestions back
/// until they were stale.
pub const CHAMP_SELECT_SUGGESTIONS_EVENT: &str = "lcu:suggestions";

/// Where the state of a game in progress reaches the UI.
///
/// A separate stream from the champ select ones, fed by a separate watcher,
/// and the two are never live at the same time — which is exactly why they
/// draw to the same screen. Champ select hands over to this the moment the
/// game loads, and this hands back when it ends.
pub const GAME_STATE_EVENT: &str = "game:state";

/// A build looked up because League said so rather than because the user
/// typed something.
///
/// It names the pair it was asked for and nothing else. Champ select knows a
/// champion id and the game does not; the game knows a game clock and champ
/// select does not; neither belongs here, because the screen matches a build
/// to what it is showing on the champion key alone.
///
/// `lookup` and `error` are exclusive, and "this source has nothing for that
/// pair" lives inside `lookup` as [`BuildLookup::NoData`] — not here.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveBuild {
    /// The Data Dragon key, which is what art is filed under and what the UI
    /// matches against the champion it is currently showing.
    pub champion_key: String,
    /// The position we asked for, in whichever vocabulary the source used.
    pub position: String,
    pub lookup: Option<BuildLookup>,
    pub error: Option<String>,
}

/// What the two champ-select checks made of the composition.
///
/// Kept apart so the UI can say which check spoke. Every suggestion in here
/// is a rule and wears the amber rail; none of them carries a number, and the
/// engine has a test that holds them to it.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampSelectSuggestions {
    /// Check one: what the enemy composition forces.
    pub threat: Vec<Suggestion>,
    /// Check two: what your own team leaves uncovered.
    pub gaps: Vec<Suggestion>,
    /// Item id to display name, so the UI can label a tile without a second
    /// lookup. It sits beside the suggestions rather than inside one because
    /// the shape of a suggestion is fixed at
    /// `{ itemId, priority, reason, source }`.
    pub item_names: HashMap<u32, String>,
}

impl ChampSelectSuggestions {
    pub fn is_empty(&self) -> bool {
        self.threat.is_empty() && self.gaps.is_empty()
    }
}

/// Run both champ-select checks over one composition.
///
/// `None` when we cannot say anything worth sending: the champion the user
/// locked has no tags, so there is no damage type to filter items by and no
/// honest way to guess one.
fn suggestions_for(champion_id: u32, comp: &Comp) -> Option<ChampSelectSuggestions> {
    let tags = Tags::get();
    let ours = tags.champion_by_id(champion_id)?.damage_type;

    let threat = enemy_threat(
        tags,
        &TeamView::from_ids(tags, comp.enemy.iter().copied()),
        ours,
    );
    let gaps = team_gaps(
        tags,
        &TeamView::from_ids(tags, comp.ally.iter().copied()),
        ours,
    );

    let item_names = threat
        .iter()
        .chain(&gaps)
        .filter_map(|suggestion| {
            let item = tags.item(suggestion.item_id)?;
            Some((suggestion.item_id, item.name.clone()))
        })
        .collect();

    Some(ChampSelectSuggestions {
        threat,
        gaps,
        item_names,
    })
}

/// What the third check made of a game in progress.
///
/// `standing` is measured and carries its numbers; the two suggestion lists
/// are rules and carry none. They travel in one payload because the screen
/// draws them together, not because they are the same kind of claim.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InGameState {
    /// Our champion, by display name — the only name the live API knows.
    pub champion: Option<String>,
    /// The same champion's Data Dragon key, which is what icon art is filed
    /// under. `None` when the champion has no tags to look it up through.
    pub champion_key: Option<String>,
    /// Our lane. `None` in a mode that assigns none, or while spectating.
    ///
    /// The one thing champ select and the game both name, which is what makes
    /// it possible to ask for the same build from either side.
    pub role: Option<Role>,
    pub level: u32,
    /// Seconds since the game started.
    pub game_time: f64,
    /// `None` in a mode that assigns no lanes, or while spectating.
    pub standing: Option<Standing>,
    /// Check one, re-run against an enemy team that is now fully visible.
    pub threat: Vec<Suggestion>,
    /// Check three.
    pub state: Vec<Suggestion>,
    pub item_names: HashMap<u32, String>,
}

/// What the live watcher's news looks like by the time it reaches the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum InGameUpdate {
    /// No game is running. The ordinary state, and the one the screen opens on.
    NoGame,
    Playing(Box<InGameState>),
}

/// Run the checks that a game in progress makes answerable.
///
/// Check one runs again here, and it is worth saying why: in champ select the
/// enemy team may be hidden entirely, so the damage split often could not be
/// called. In game every champion is visible, so the same check finally has
/// the whole picture. Check two is not re-run — your own team's shape was
/// settled at champ select and no item changes it.
fn in_game_state(snapshot: &GameSnapshot) -> InGameState {
    let tags = Tags::get();

    let us = snapshot.local_player();
    let ours = us.and_then(|player| tags.champion_by_name(&player.champion_name));

    let threat = match ours {
        Some(ours) => enemy_threat(
            tags,
            &TeamView::new(
                snapshot
                    .enemies()
                    .iter()
                    .map(|player| tags.champion_by_name(&player.champion_name)),
            ),
            ours.damage_type,
        ),
        // No tags for our own champion means no damage type to filter items
        // by, and nothing honest to say.
        None => Vec::new(),
    };

    let state = game_state(tags, snapshot);

    let item_names = threat
        .iter()
        .chain(&state)
        .filter_map(|suggestion| {
            let item = tags.item(suggestion.item_id)?;
            Some((suggestion.item_id, item.name.clone()))
        })
        .collect();

    InGameState {
        champion: us.map(|player| player.champion_name.clone()),
        champion_key: us
            .and_then(|player| tags.champion_key_by_name(&player.champion_name))
            .map(str::to_string),
        role: us.and_then(|player| player.position),
        level: us.map(|player| player.level).unwrap_or(0),
        game_time: snapshot.game_time,
        standing: standing(snapshot),
        threat,
        state,
        item_names,
    }
}

/// The champion-role pair the live screen already holds a build for.
///
/// Two routes can now ask for a build — champ select when you lock in, and
/// the game itself when the app was opened after champ select had ended — and
/// in the ordinary run they would both ask for the same one. This is the
/// whole of the coordination between them: whoever gets there first takes the
/// slot, and the other sees the pair is already answered and stays quiet.
///
/// A failed lookup hands the slot back rather than keeping it. That is what
/// turns the in-game poll into a retry for champ select, and it is why a
/// queue that assigns no position still ends up with a build: champ select
/// cannot name a lane there, but the game always can.
#[derive(Clone, Default)]
struct BuildClaim(Arc<tokio::sync::Mutex<Option<(String, String)>>>);

impl BuildClaim {
    /// Take the slot for this pair. `false` when it already holds it, which
    /// is the caller's cue to say nothing.
    async fn take(&self, champion_key: &str, position: &str) -> bool {
        let mut held = self.0.lock().await;
        let pair = (champion_key.to_string(), position.to_string());
        if held.as_ref() == Some(&pair) {
            return false;
        }
        *held = Some(pair);
        true
    }

    /// Hand the slot back, so the next route to look asks again.
    async fn release(&self) {
        *self.0.lock().await = None;
    }
}

/// Boot the desktop app.
///
/// One window, one piece of managed state. The provider is chosen from config
/// at startup and never re-examined by anything downstream.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let config = resolve_config(app.handle());
            let service = Arc::new(build_service(&config));
            app.manage(Arc::clone(&service));
            // Fetched lazily on the first render that wants an icon, then
            // kept. Nothing is requested if the window is never opened on a
            // build.
            app.manage(Arc::new(DataDragonState::new()));
            spawn_champ_select(app.handle(), service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::source_label,
            commands::fetch_build,
            commands::data_dragon,
        ])
        .run(tauri::generate_context!())
        .expect("leaguechecker failed to start");
}

/// A broken config must not be fatal. The user still gets a working window on
/// the default provider, and the reason lands in the log.
fn build_service(config: &ProviderConfig) -> BuildService {
    match BuildService::from_config(config) {
        Ok(service) => service,
        Err(error) => {
            eprintln!("leaguechecker: provider setup failed ({error}); using defaults");
            BuildService::from_config(&ProviderConfig::default())
                .expect("the default provider is always constructible")
        }
    }
}

/// Watch the League client, and look a build up whenever it reports a lock.
///
/// This is the whole seam between the two halves of the app. The watcher
/// knows nothing about builds and the service knows nothing about the client;
/// they meet here, in one call. Both tasks run for the life of the app, and
/// both are cheap while nothing is happening — the watcher is asleep on a
/// socket and this one is asleep on a channel.
fn spawn_champ_select(handle: &AppHandle, service: Arc<BuildService>) {
    // Champ select produces a handful of events per game. A small buffer is
    // plenty, and a full one would mean something is very wrong.
    let (sender, mut receiver) = mpsc::channel(16);
    let handle = handle.clone();

    tauri::async_runtime::spawn(lcu::watch(WatcherConfig::default(), sender));

    // The live watcher is gated on this. While it holds false that task does
    // no work whatsoever — it is asleep on the channel, not polling slowly —
    // which is what makes an in-game poll loop affordable at all. The LCU
    // layer already knows whether League is open, so nothing else has to look.
    let (launcher_running, launcher_gate) = tokio::sync::watch::channel(false);
    let claim = BuildClaim::default();
    spawn_live_game(&handle, Arc::clone(&service), claim.clone(), launcher_gate);

    tauri::async_runtime::spawn(async move {
        // The two halves of a suggestion arrive separately and in either
        // order: you can lock in before the enemy team is visible, or after.
        // Both are held until champ select ends so a late enemy pick can be
        // reasoned about against the champion you already locked.
        let mut locked: Option<u32> = None;
        let mut comp: Option<Comp> = None;

        while let Some(event) = receiver.recv().await {
            // The UI shows "League isn't running" from this, so every state
            // goes out, not just the interesting one.
            let _ = handle.emit(CHAMP_SELECT_STATUS_EVENT, &event);

            // Every state the watcher reports also answers "is League open?",
            // which is the gate the live task waits on. Read before the match
            // below consumes the event. A send failure means that task is
            // gone, which is not fatal to this one.
            let _ = launcher_running.send(!matches!(&event, ChampSelectEvent::ClientOffline));

            let recheck = match event {
                ChampSelectEvent::Locked(champion) => {
                    locked = Some(champion.champion_id);

                    // Taking the slot unconditionally: the watcher already
                    // drops every repeat, so a `Locked` reaching this line is
                    // always news, and it must win over whatever the game
                    // route may have claimed for the game before this one.
                    claim
                        .take(&champion.champion_key, &champion.assigned_position)
                        .await;

                    let build = build_for(
                        &service,
                        &champion.champion_key,
                        &champion.assigned_position,
                        Some(champion.champion_id),
                    )
                    .await;
                    // Nothing to hold the slot for. Letting it go means the
                    // game route will try the pair again once the match
                    // starts, which is the only retry this app has.
                    if build.error.is_some() {
                        claim.release().await;
                    }
                    let _ = handle.emit(LIVE_BUILD_EVENT, build);
                    true
                }
                ChampSelectEvent::CompChanged(changed) => {
                    comp = Some(changed);
                    true
                }
                // Champ select ended, or the client did. Whatever we were
                // reasoning about is gone, and holding it would let the next
                // champ select open against the last one's enemy team.
                ChampSelectEvent::Left => {
                    locked = None;
                    comp = None;
                    false
                }
                // The client went away, so the build on screen belongs to
                // nothing we can still see. `Left` deliberately does not do
                // this: champ select ending is how a game *starts*, and the
                // pair we just looked up is the pair about to be played.
                ChampSelectEvent::ClientOffline => {
                    locked = None;
                    comp = None;
                    claim.release().await;
                    false
                }
                // A fresh champ select. Whatever was claimed belongs to the
                // last game.
                ChampSelectEvent::Entered => {
                    claim.release().await;
                    false
                }
                _ => false,
            };

            if !recheck {
                continue;
            }

            // Both checks are map lookups over a handful of champions. This
            // runs during champ select, where the game is idle, and never
            // once it starts.
            if let (Some(champion_id), Some(comp)) = (locked, comp.as_ref()) {
                if let Some(suggestions) = suggestions_for(champion_id, comp) {
                    let _ = handle.emit(CHAMP_SELECT_SUGGESTIONS_EVENT, suggestions);
                }
            }
        }
    });
}

/// Watch the game itself, and run the third check over what it says.
///
/// Gated on `launcher_running`: this task does nothing at all until League is
/// open. See [`live::watcher`] for why that gate is the whole reason an
/// in-game poll loop is affordable.
fn spawn_live_game(
    handle: &AppHandle,
    service: Arc<BuildService>,
    claim: BuildClaim,
    launcher_running: tokio::sync::watch::Receiver<bool>,
) {
    // A reading every half-minute at most. A buffer this size is already
    // generous; a full one would mean the UI thread had stopped entirely.
    let (sender, mut receiver) = mpsc::channel(8);
    let handle = handle.clone();

    tauri::async_runtime::spawn(live::watch(
        LiveWatcherConfig::default(),
        launcher_running,
        sender,
    ));

    tauri::async_runtime::spawn(async move {
        // Every reading goes out. Deduplicating here would not work: the game
        // clock moves on every poll, so no two payloads are ever equal, and
        // stripping the clock out to compare would mean showing a time that
        // stopped updating.
        //
        // The two halves of this payload change at completely different
        // rates — the clock every half-minute, the advice a handful of times
        // a game — so they are deduplicated where they are drawn rather than
        // here. The screen updates its header every time and rewrites the
        // suggestion blocks only when they actually differ.
        while let Some(event) = receiver.recv().await {
            let update = match event {
                GameEvent::NoGame => InGameUpdate::NoGame,
                GameEvent::Snapshot(snapshot) => {
                    let state = in_game_state(&snapshot);

                    // Opening the app after champ select is over is an
                    // ordinary way to use it, and until now it meant playing
                    // the whole game with no build on screen. The game names
                    // the same two things champ select does — which champion,
                    // which lane — so it can ask for exactly the same build.
                    //
                    // The claim is what keeps this quiet in the usual case,
                    // where champ select already fetched it: this looks every
                    // half-minute and speaks at most once a game.
                    if let (Some(key), Some(role)) = (state.champion_key.as_deref(), state.role) {
                        if claim.take(key, role.as_str()).await {
                            let build = build_for(&service, key, role.as_str(), None).await;
                            if build.error.is_some() {
                                claim.release().await;
                            }
                            let _ = handle.emit(LIVE_BUILD_EVENT, build);
                        }
                    }

                    InGameUpdate::Playing(Box::new(state))
                }
            };

            let _ = handle.emit(GAME_STATE_EVENT, &update);
        }
    });
}

/// The handoff itself, kept out of the Tauri tasks so it can be tested
/// without an app handle.
///
/// Both routes into the live screen come through here, which is what makes
/// them indistinguishable downstream: the payload records the pair that was
/// asked for and says nothing about who asked.
///
/// `champion_id` is passed through to the provider when the route has one —
/// champ select does, the live game does not — and only ever enriches the
/// answer. No source needs it to find a build.
async fn build_for(
    service: &BuildService,
    champion_key: &str,
    position: &str,
    champion_id: Option<u32>,
) -> LiveBuild {
    let (lookup, error) = match service.build_for(champion_key, position, champion_id).await {
        Ok(lookup) => (Some(lookup), None),
        Err(error) => (None, Some(error.to_string())),
    };

    LiveBuild {
        champion_key: champion_key.to_string(),
        position: position.to_string(),
        lookup,
        error,
    }
}

/// `providers.json` in the OS app-config directory, falling back to the
/// working directory when the platform will not name one. A missing file is
/// the normal first-run case and yields defaults.
fn resolve_config(handle: &tauri::AppHandle) -> ProviderConfig {
    let path = handle
        .path()
        .app_config_dir()
        .map(|dir| dir.join(CONFIG_FILE_NAME))
        .unwrap_or_else(|_| PathBuf::from(CONFIG_FILE_NAME));

    ProviderConfig::load(&path).unwrap_or_else(|error| {
        eprintln!("leaguechecker: {}: {error}; using defaults", path.display());
        ProviderConfig::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct Stub;

    #[async_trait]
    impl BuildDataProvider for Stub {
        fn label(&self) -> &str {
            "stub"
        }

        async fn fetch_build(&self, request: &BuildRequest) -> Result<BuildLookup, ProviderError> {
            Ok(BuildLookup::no_data(request, "stub"))
        }
    }

    #[tokio::test]
    async fn routes_lcu_shaped_input_to_the_active_provider() {
        let service = BuildService::new(Arc::new(Stub));
        assert_eq!(service.source_label(), "stub");

        let lookup = service.build_for("Ahri", "middle", Some(103)).await.unwrap();
        match lookup {
            BuildLookup::NoData(no_data) => {
                assert_eq!(no_data.role, Role::Middle);
                assert_eq!(no_data.champion_key, "Ahri");
            }
            BuildLookup::Found(_) => panic!("expected no data"),
        }
    }

    #[tokio::test]
    async fn a_locked_champion_goes_straight_to_the_active_provider() {
        let service = BuildService::new(Arc::new(Stub));
        let build = build_for(&service, "Ahri", "middle", Some(103)).await;

        assert!(build.error.is_none());
        assert!(matches!(build.lookup, Some(BuildLookup::NoData(_))));
        assert_eq!(build.champion_key, "Ahri");
    }

    /// The live screen is fed by two routes and must not be able to tell them
    /// apart. A build the client asked for and a build the running game asked
    /// for are byte-for-byte the same payload.
    #[tokio::test]
    async fn both_routes_produce_the_same_payload() {
        let service = BuildService::new(Arc::new(Stub));

        let from_champ_select = build_for(&service, "Ahri", "middle", Some(103)).await;
        let from_the_game = build_for(&service, "Ahri", "middle", None).await;

        assert_eq!(
            serde_json::to_value(&from_champ_select).unwrap(),
            serde_json::to_value(&from_the_game).unwrap(),
        );
    }

    /// Matches `LiveBuild` in main.ts. `lookup` and `error` are both present
    /// as keys and exactly one of them is null, because the UI branches on
    /// which; `championKey` is what it matches against the champion on screen.
    #[tokio::test]
    async fn the_build_payload_the_ui_reads_is_fixed() {
        let service = BuildService::new(Arc::new(Stub));
        let build = build_for(&service, "Ahri", "middle", Some(103)).await;

        let json = serde_json::to_value(&build).unwrap();
        assert_eq!(json["championKey"], "Ahri");
        assert_eq!(json["position"], "middle");
        assert_eq!(json["lookup"]["status"], "noData");
        assert_eq!(json["error"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn a_failed_lookup_reaches_the_ui_as_a_message() {
        let service = BuildService::new(Arc::new(Stub));
        // Champ select in a queue that assigns no position.
        let build = build_for(&service, "Ahri", "", Some(103)).await;

        assert!(build.lookup.is_none());
        assert!(build.error.unwrap().contains("unknown role"));
    }

    /// The claim is the only thing standing between the two routes and a
    /// duplicate lookup every game.
    #[tokio::test]
    async fn the_second_route_to_ask_for_a_pair_is_told_to_stay_quiet() {
        let claim = BuildClaim::default();

        assert!(claim.take("Ahri", "middle").await, "nobody had asked yet");
        assert!(
            !claim.take("Ahri", "middle").await,
            "the game re-fetched a build champ select already had"
        );
        // A different pair is a different question.
        assert!(claim.take("Ahri", "top").await);
    }

    /// A queue that assigns no position cannot be looked up in champ select,
    /// and the game can. Releasing the slot after a failure is what lets the
    /// second route try.
    #[tokio::test]
    async fn a_released_claim_lets_the_other_route_try() {
        let claim = BuildClaim::default();

        assert!(claim.take("Ahri", "").await);
        claim.release().await;
        assert!(
            claim.take("Ahri", "middle").await,
            "champ select's failure locked the game out of asking"
        );
    }

    #[tokio::test]
    async fn an_unassigned_position_is_a_clear_error() {
        let service = BuildService::new(Arc::new(Stub));
        let error = service.build_for("Ahri", "", None).await.unwrap_err();
        assert!(matches!(error, ProviderError::UnknownRole(_)));
    }
}

#[cfg(test)]
mod suggestion_tests {
    use super::*;
    use recommend::{DamageType, SuggestionSource};

    /// Locked Ahri into Darius, Zed, Caitlyn, Talon and Draven — five
    /// physical-damage enemies, two of whom dive.
    fn against_physical() -> Comp {
        Comp {
            ally: vec![103, 64, 22, 412, 86],
            enemy: vec![122, 238, 51, 91, 119],
        }
    }

    #[test]
    fn a_full_champ_select_produces_advice_from_both_sides() {
        let found = suggestions_for(103, &against_physical()).expect("Ahri is tagged");
        assert!(!found.threat.is_empty(), "five enemies and nothing to say");
        assert!(!found.is_empty());
    }

    #[test]
    fn every_suggestion_can_be_labelled() {
        let found = suggestions_for(103, &against_physical()).unwrap();
        for suggestion in found.threat.iter().chain(&found.gaps) {
            assert!(
                found.item_names.contains_key(&suggestion.item_id),
                "item {} reaches the UI with no name to render",
                suggestion.item_id
            );
        }
    }

    #[test]
    fn a_champion_we_have_no_tags_for_says_nothing_rather_than_guessing() {
        assert!(
            suggestions_for(999_999, &against_physical()).is_none(),
            "an untagged champion produced advice from a damage type we invented"
        );
    }

    #[test]
    fn a_hidden_enemy_team_is_not_reasoned_about() {
        // Blind pick: we locked in, and their side is five empty seats.
        let blind = Comp {
            ally: vec![103, 64, 22, 412, 86],
            enemy: vec![0, 0, 0, 0, 0],
        };
        let found = suggestions_for(103, &blind).unwrap();
        assert!(
            found.threat.is_empty(),
            "the enemy check spoke about a team it cannot see"
        );
    }

    #[test]
    fn the_items_suit_the_champion_that_locked_in() {
        // Garen is physical damage and must never be handed an ability power
        // item, however well it answers the threat.
        let found = suggestions_for(86, &against_physical()).unwrap();
        for suggestion in found.threat.iter().chain(&found.gaps) {
            let item = Tags::get().item(suggestion.item_id).unwrap();
            assert_ne!(item.damage, Some(DamageType::Ap), "{}", item.name);
            assert_eq!(suggestion.source, SuggestionSource::Rule);
        }
    }
}

#[cfg(test)]
mod in_game_tests {
    use super::*;
    use recommend::{Footing, SuggestionSource};
    use serde_json::json;

    /// Ahri mid against a fully visible, entirely physical enemy team — the
    /// situation champ select could not read in a blind pick queue.
    fn game(our_gold: u32, their_gold: u32) -> GameSnapshot {
        let player = |name: &str, team: &str, position: &str, gold: u32| {
            json!({
                "championName": name, "team": team, "position": position,
                "riotId": format!("{name}#EUW"), "level": 11, "isDead": false,
                "items": [{ "itemID": 1, "price": gold, "count": 1 }],
                "scores": { "kills": 2, "deaths": 4, "assists": 1, "creepScore": 90 },
            })
        };
        GameSnapshot::from_json(&json!({
            "activePlayer": { "riotId": "Ahri#EUW", "currentGold": 1340.0 },
            "allPlayers": [
                player("Ahri", "ORDER", "MIDDLE", our_gold),
                player("Leona", "ORDER", "UTILITY", 2000),
                player("Zed", "CHAOS", "MIDDLE", their_gold),
                player("Darius", "CHAOS", "TOP", 5000),
                player("Draven", "CHAOS", "BOTTOM", 5000),
            ],
            "gameData": { "gameTime": 1104.5, "gameMode": "CLASSIC" },
        }))
        .unwrap()
    }

    #[test]
    fn the_enemy_check_finally_sees_the_whole_team() {
        let state = in_game_state(&game(3000, 8000));

        // In champ select this team may have been five hidden seats. In game
        // it never is, so the damage split can actually be called.
        assert!(
            !state.threat.is_empty(),
            "three physical-damage enemies in plain sight and nothing to say"
        );
        assert_eq!(state.champion.as_deref(), Some("Ahri"));
        assert_eq!(state.level, 11);
    }

    /// The lane is what makes the game a second route to a build. Without it
    /// there is a champion and no question to ask about it.
    #[test]
    fn the_game_names_the_pair_a_build_is_filed_under() {
        let state = in_game_state(&game(3000, 8000));
        assert_eq!(state.champion_key.as_deref(), Some("Ahri"));
        assert_eq!(state.role, Some(Role::Middle));
    }

    #[test]
    fn being_behind_is_measured_and_the_advice_beside_it_is_not() {
        let state = in_game_state(&game(3000, 8000));

        let standing = state.standing.expect("Zed is in our lane");
        assert_eq!(standing.footing, Footing::Behind);
        assert_eq!(standing.opponent, "Zed");
        assert_eq!(standing.gold_delta, -5000);
        assert_eq!(standing.gold_in_hand, 1340);

        assert!(!state.state.is_empty(), "five thousand down and no advice");
        for suggestion in state.threat.iter().chain(&state.state) {
            assert_eq!(suggestion.source, SuggestionSource::Rule);
            assert!(
                !suggestion.reason.chars().any(|c| c.is_ascii_digit()),
                "the standing carries the number, not the sentence: {}",
                suggestion.reason
            );
        }
    }

    #[test]
    fn an_even_game_still_answers_the_enemy_team() {
        let state = in_game_state(&game(5000, 5200));

        assert_eq!(state.standing.unwrap().footing, Footing::Even);
        assert!(state.state.is_empty(), "an even game changed what to buy");
        assert!(
            !state.threat.is_empty(),
            "the enemy composition is worth answering however the game is going"
        );
    }

    #[test]
    fn every_suggestion_can_be_labelled() {
        let state = in_game_state(&game(3000, 8000));
        for suggestion in state.threat.iter().chain(&state.state) {
            assert!(
                state.item_names.contains_key(&suggestion.item_id),
                "item {} reaches the UI with no name to render",
                suggestion.item_id
            );
        }
    }

    #[test]
    fn spectating_produces_a_payload_with_nothing_claimed_in_it() {
        let spectated = GameSnapshot::from_json(&json!({
            "allPlayers": [{
                "championName": "Ahri", "team": "ORDER", "position": "MIDDLE",
                "items": [], "scores": {},
            }],
            "gameData": { "gameTime": 300.0 },
        }))
        .unwrap();

        let state = in_game_state(&spectated);
        assert!(state.champion.is_none());
        // Nothing to look a build up by either, so the game route stays quiet.
        assert!(state.role.is_none());
        assert!(state.standing.is_none());
        assert!(state.threat.is_empty());
        assert!(state.state.is_empty());
    }
}
