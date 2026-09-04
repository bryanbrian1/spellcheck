//! leaguechecker core.
//!
//! The build data layer, plus the Tauri shell that hosts it. Lockfile
//! parsing, the LCU client and the champ select WebSocket are still to come;
//! they will feed [`BuildService::build_for`] the same way the search box
//! does today.

pub mod build_data;
pub mod commands;
pub mod lcu;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::Manager;

use build_data::config::CONFIG_FILE_NAME;

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

/// Boot the desktop app.
///
/// One window, one piece of managed state. The provider is chosen from config
/// at startup and never re-examined by anything downstream.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let config = resolve_config(app.handle());
            app.manage(build_service(&config));
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
    async fn an_unassigned_position_is_a_clear_error() {
        let service = BuildService::new(Arc::new(Stub));
        let error = service.build_for("Ahri", "", None).await.unwrap_err();
        assert!(matches!(error, ProviderError::UnknownRole(_)));
    }
}
