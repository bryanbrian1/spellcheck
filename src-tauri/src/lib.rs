//! leaguechecker core.
//!
//! Today this is the build data layer. The Tauri shell (lockfile parsing, LCU
//! client, champ select WebSocket) will sit on top of it and expose
//! [`BuildService`] as managed state.

pub mod build_data;

use std::sync::Arc;

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
