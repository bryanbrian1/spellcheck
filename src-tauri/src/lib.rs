//! leaguechecker core.
//!
//! The build data layer, the LCU layer that watches the League client, and
//! the Tauri shell that hosts both. The two meet in exactly one place:
//! [`BuildService::build_for`], which the search box calls when the user
//! types a champion and [`spawn_champ_select`] calls when the client says one
//! was locked. Neither route knows about the other.

pub mod build_data;
pub mod commands;
pub mod lcu;

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

use build_data::config::CONFIG_FILE_NAME;
use lcu::{ChampSelectEvent, LockedChampion, WatcherConfig};

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

/// Where the build for a locked champion reaches the UI.
pub const CHAMP_SELECT_BUILD_EVENT: &str = "lcu:build";

/// A build looked up because the client said so rather than because the user
/// typed something. `lookup` and `error` are exclusive, and "this source has
/// nothing for that pair" lives inside `lookup` as
/// [`BuildLookup::NoData`] — not here.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampSelectBuild {
    pub champion: LockedChampion,
    pub lookup: Option<BuildLookup>,
    pub error: Option<String>,
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
            spawn_champ_select(app.handle(), service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::source_label,
            commands::fetch_build,
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
    tauri::async_runtime::spawn(async move {
        while let Some(event) = receiver.recv().await {
            // The UI shows "League isn't running" from this, so every state
            // goes out, not just the interesting one.
            let _ = handle.emit(CHAMP_SELECT_STATUS_EVENT, &event);

            if let ChampSelectEvent::Locked(locked) = event {
                let build = build_for_locked(&service, locked).await;
                let _ = handle.emit(CHAMP_SELECT_BUILD_EVENT, build);
            }
        }
    });
}

/// The handoff itself, kept out of the Tauri task so it can be tested without
/// an app handle.
async fn build_for_locked(service: &BuildService, champion: LockedChampion) -> ChampSelectBuild {
    match service
        .build_for(
            &champion.champion_key,
            &champion.assigned_position,
            Some(champion.champion_id),
        )
        .await
    {
        Ok(lookup) => ChampSelectBuild {
            champion,
            lookup: Some(lookup),
            error: None,
        },
        Err(error) => ChampSelectBuild {
            champion,
            lookup: None,
            error: Some(error.to_string()),
        },
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
        let build = build_for_locked(
            &service,
            LockedChampion {
                champion_id: 103,
                champion_key: "Ahri".to_string(),
                assigned_position: "middle".to_string(),
            },
        )
        .await;

        assert!(build.error.is_none());
        assert!(matches!(build.lookup, Some(BuildLookup::NoData(_))));
        assert_eq!(build.champion.champion_key, "Ahri");
    }

    /// Matches `ChampSelectBuild` in main.ts. `lookup` and `error` are both
    /// present as keys and exactly one of them is null, because the UI branches
    /// on which.
    #[tokio::test]
    async fn the_build_payload_the_ui_reads_is_fixed() {
        let service = BuildService::new(Arc::new(Stub));
        let build = build_for_locked(
            &service,
            LockedChampion {
                champion_id: 103,
                champion_key: "Ahri".to_string(),
                assigned_position: "middle".to_string(),
            },
        )
        .await;

        let json = serde_json::to_value(&build).unwrap();
        assert_eq!(json["champion"]["championKey"], "Ahri");
        assert_eq!(json["champion"]["assignedPosition"], "middle");
        assert_eq!(json["lookup"]["status"], "noData");
        assert_eq!(json["error"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn a_failed_lookup_reaches_the_ui_as_a_message() {
        let service = BuildService::new(Arc::new(Stub));
        let build = build_for_locked(
            &service,
            LockedChampion {
                champion_id: 103,
                champion_key: "Ahri".to_string(),
                // Champ select in a queue that assigns no position.
                assigned_position: String::new(),
            },
        )
        .await;

        assert!(build.lookup.is_none());
        assert!(build.error.unwrap().contains("unknown role"));
    }

    #[tokio::test]
    async fn an_unassigned_position_is_a_clear_error() {
        let service = BuildService::new(Arc::new(Stub));
        let error = service.build_for("Ahri", "", None).await.unwrap_err();
        assert!(matches!(error, ProviderError::UnknownRole(_)));
    }
}
