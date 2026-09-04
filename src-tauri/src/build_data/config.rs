//! Which provider is live, and how each one is set up.
//!
//! The active provider is data, not a compile-time choice: flip
//! `provider` in the config file (or set `LEAGUECHECKER_PROVIDER`) to move the
//! whole app from OP.GG to our own crawl without touching the UI.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::error::ProviderError;
use super::opgg::{OpggConfig, OpggProvider};
use super::riot::{RiotConfig, RiotProvider};
use super::BuildDataProvider;

/// Environment override, mainly for development and for CI smoke tests.
pub const PROVIDER_ENV_VAR: &str = "LEAGUECHECKER_PROVIDER";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// Live per-request queries to the OP.GG MCP endpoint.
    #[default]
    Opgg,
    /// Our own crawled data under `data/builds/`.
    Riot,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 2] = [ProviderKind::Opgg, ProviderKind::Riot];

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::Opgg => "opgg",
            ProviderKind::Riot => "riot",
        }
    }

    pub fn parse(raw: &str) -> Result<ProviderKind, ProviderError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "opgg" | "op.gg" | "op_gg" => Ok(ProviderKind::Opgg),
            "riot" | "local" | "crawl" => Ok(ProviderKind::Riot),
            other => Err(ProviderError::Config(format!(
                "unknown provider {other:?}; expected one of: {}",
                ProviderKind::ALL
                    .iter()
                    .map(|kind| kind.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }

    /// The one place in the codebase that names concrete provider types.
    /// A third provider is a new variant plus a new arm here.
    fn instantiate(self, config: &ProviderConfig) -> Result<Arc<dyn BuildDataProvider>, ProviderError> {
        Ok(match self {
            ProviderKind::Opgg => Arc::new(OpggProvider::new(config.opgg.clone())?),
            ProviderKind::Riot => Arc::new(RiotProvider::new(config.riot.clone())),
        })
    }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderConfig {
    /// Which provider serves build lookups.
    pub provider: ProviderKind,
    pub opgg: OpggConfig,
    pub riot: RiotConfig,
}

impl ProviderConfig {
    /// Read config from `path`.
    ///
    /// A missing file is not an error — first run has no config yet and the
    /// defaults are the shipping configuration.
    pub fn load(path: &Path) -> Result<ProviderConfig, ProviderError> {
        let mut config = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|error| {
                ProviderError::Config(format!("{}: {error}", path.display()))
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => ProviderConfig::default(),
            Err(error) => {
                return Err(ProviderError::Config(format!(
                    "{}: {error}",
                    path.display()
                )))
            }
        };
        config.apply_env_overrides()?;
        Ok(config)
    }

    /// `LEAGUECHECKER_PROVIDER` wins over the file when set.
    pub fn apply_env_overrides(&mut self) -> Result<(), ProviderError> {
        if let Ok(raw) = std::env::var(PROVIDER_ENV_VAR) {
            if !raw.trim().is_empty() {
                self.provider = ProviderKind::parse(&raw)?;
            }
        }
        Ok(())
    }

    /// Build the active provider. Callers hold the returned `Arc` as app state
    /// and never learn which implementation is behind it.
    pub fn active_provider(&self) -> Result<Arc<dyn BuildDataProvider>, ProviderError> {
        self.provider.instantiate(self)
    }
}

/// Default on-disk location for the config file, relative to the app's data
/// directory. Tauri resolves the absolute path at startup.
pub const CONFIG_FILE_NAME: &str = "providers.json";

/// Convenience for tests and for the CLI ingest tooling.
pub fn provider_from_kind(
    kind: ProviderKind,
    config: &ProviderConfig,
) -> Result<Arc<dyn BuildDataProvider>, ProviderError> {
    kind.instantiate(config)
}

/// Where `data/builds` lives relative to a repo checkout, used by the ingest
/// tooling and by [`RiotConfig::default`].
pub fn default_build_data_root() -> PathBuf {
    PathBuf::from("data/builds")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_opgg() {
        let config = ProviderConfig::default();
        assert_eq!(config.provider, ProviderKind::Opgg);
        assert_eq!(config.active_provider().unwrap().label(), "OP.GG");
    }

    #[test]
    fn missing_config_file_is_not_an_error() {
        let config = ProviderConfig::load(Path::new("/nonexistent/providers.json")).unwrap();
        assert_eq!(config.provider, ProviderKind::Opgg);
    }

    #[test]
    fn provider_is_a_config_value() {
        let config: ProviderConfig = serde_json::from_str(r#"{ "provider": "riot" }"#).unwrap();
        assert_eq!(config.provider, ProviderKind::Riot);
        assert_eq!(config.active_provider().unwrap().label(), "leaguechecker");
    }

    #[test]
    fn rejects_an_unknown_provider_name() {
        let error = ProviderKind::parse("mobafire").unwrap_err();
        assert!(error.to_string().contains("unknown provider"));
    }

    #[test]
    fn partial_config_keeps_defaults() {
        let config: ProviderConfig =
            serde_json::from_str(r#"{ "opgg": { "tier": "diamond_plus" } }"#).unwrap();
        assert_eq!(config.provider, ProviderKind::Opgg);
        assert_eq!(config.opgg.tier.as_deref(), Some("diamond_plus"));
        assert_eq!(config.opgg.endpoint, OpggConfig::default().endpoint);
    }
}
