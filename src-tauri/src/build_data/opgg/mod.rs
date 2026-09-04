//! Live build data from the OP.GG MCP endpoint.
//!
//! **Per-request only.** OP.GG's dataset is theirs; nothing fetched here is
//! written to disk, committed, or redistributed. There is deliberately no
//! cache in this module — a lookup either hits the endpoint or it does not
//! happen. The distributable path is
//! [`RiotProvider`](crate::build_data::riot::RiotProvider).

pub mod map;
pub mod mcp;
pub mod wire;

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::error::ProviderError;
use super::schema::{BuildLookup, BuildRequest};
use super::{validate_champion_key, BuildDataProvider};

use mcp::McpClient;

pub const PROVIDER_LABEL: &str = "OP.GG";
pub const DEFAULT_ENDPOINT: &str = "https://mcp-api.op.gg/mcp";
pub const DEFAULT_TOOL: &str = "lol_get_champion_analysis";
const DEFAULT_TIMEOUT_SECS: u64 = 10;

/// Short name used in error messages.
const PROVIDER: &str = "OP.GG";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OpggConfig {
    pub endpoint: String,
    /// MCP tool to call. Configurable so a renamed tool is a config change,
    /// not a release.
    pub tool: String,
    /// Optional query narrowing, passed through only when set.
    pub region: Option<String>,
    pub tier: Option<String>,
    /// Champ select is short. A lookup that has not answered by now is not
    /// going to be useful.
    pub timeout_secs: u64,
    /// Attribution shown in the UI.
    pub label: String,
}

impl Default for OpggConfig {
    fn default() -> Self {
        OpggConfig {
            endpoint: DEFAULT_ENDPOINT.to_string(),
            tool: DEFAULT_TOOL.to_string(),
            region: None,
            tier: None,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            label: PROVIDER_LABEL.to_string(),
        }
    }
}

#[derive(Debug)]
pub struct OpggProvider {
    config: OpggConfig,
    client: McpClient,
}

impl OpggProvider {
    pub fn new(config: OpggConfig) -> Result<OpggProvider, ProviderError> {
        if config.endpoint.trim().is_empty() {
            return Err(ProviderError::Config("opgg.endpoint is empty".to_string()));
        }

        let client = McpClient::new(
            config.endpoint.clone(),
            Duration::from_secs(config.timeout_secs.max(1)),
            PROVIDER,
        )?;

        Ok(OpggProvider { config, client })
    }

    /// Arguments for the analysis tool.
    ///
    /// Kept in one place: the tool's exact parameter names are the endpoint's
    /// to define, and [`Self::tool_catalogue`] reports them from the live
    /// server if they ever change.
    fn arguments(&self, request: &BuildRequest) -> Value {
        let mut arguments = json!({
            "champion": request.champion_key,
            "position": request.role.opgg_position(),
        });

        let object = arguments.as_object_mut().expect("json! built an object");
        if let Some(region) = &self.config.region {
            object.insert("region".to_string(), json!(region));
        }
        if let Some(tier) = &self.config.tier {
            object.insert("tier".to_string(), json!(tier));
        }

        arguments
    }

    /// The endpoint's tool list with input schemas. Not used in the lookup
    /// path — it exists so the argument names above can be checked against
    /// the live server.
    pub async fn tool_catalogue(&self) -> Result<Value, ProviderError> {
        self.client.list_tools().await
    }
}

#[async_trait]
impl BuildDataProvider for OpggProvider {
    fn label(&self) -> &str {
        &self.config.label
    }

    async fn fetch_build(&self, request: &BuildRequest) -> Result<BuildLookup, ProviderError> {
        // Validated even though this is not a filesystem path: the key goes
        // into a request we make on the user's behalf.
        validate_champion_key(&request.champion_key)?;

        let payload = self
            .client
            .call_tool(&self.config.tool, self.arguments(request))
            .await?;

        // Mapped and returned; never persisted.
        Ok(map::build_from_payload(&payload, request, self.label()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_data::role::Role;

    fn provider() -> OpggProvider {
        OpggProvider::new(OpggConfig::default()).unwrap()
    }

    #[test]
    fn defaults_point_at_the_documented_endpoint() {
        let provider = provider();
        assert_eq!(provider.client.endpoint(), DEFAULT_ENDPOINT);
        assert_eq!(provider.config.tool, DEFAULT_TOOL);
        assert_eq!(provider.label(), PROVIDER_LABEL);
    }

    #[test]
    fn translates_roles_to_opgg_positions() {
        let provider = provider();
        let arguments = provider.arguments(&BuildRequest::new("Ahri", Role::Middle));
        assert_eq!(arguments["champion"], "Ahri");
        assert_eq!(arguments["position"], "MID");

        let support = provider.arguments(&BuildRequest::new("Thresh", Role::Utility));
        assert_eq!(support["position"], "SUPPORT");
    }

    #[test]
    fn optional_narrowing_is_omitted_when_unset() {
        let arguments = provider().arguments(&BuildRequest::new("Ahri", Role::Middle));
        assert!(arguments.get("region").is_none());
        assert!(arguments.get("tier").is_none());

        let narrowed = OpggProvider::new(OpggConfig {
            region: Some("kr".to_string()),
            tier: Some("diamond_plus".to_string()),
            ..OpggConfig::default()
        })
        .unwrap();
        let arguments = narrowed.arguments(&BuildRequest::new("Ahri", Role::Middle));
        assert_eq!(arguments["region"], "kr");
        assert_eq!(arguments["tier"], "diamond_plus");
    }

    #[test]
    fn rejects_an_empty_endpoint() {
        let error = OpggProvider::new(OpggConfig {
            endpoint: "  ".to_string(),
            ..OpggConfig::default()
        })
        .unwrap_err();
        assert!(matches!(error, ProviderError::Config(_)));
    }

    #[tokio::test]
    async fn rejects_a_bad_champion_key_before_any_request() {
        let error = provider()
            .fetch_build(&BuildRequest::new("../etc", Role::Middle))
            .await
            .unwrap_err();
        assert!(matches!(error, ProviderError::InvalidChampionKey(_)));
    }
}
