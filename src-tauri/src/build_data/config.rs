//! Which provider is live, and how each one is set up.
//!
//! The active provider is data, not a compile-time choice: flip
//! `provider` in the config file (or set `SPELLCHECK_PROVIDER`) to move the
//! whole app from OP.GG to our own crawl without touching the UI.
//!
//! `providers.json` is also the app's only config file, so the one setting
//! that is not about a provider — where the League client is — lives in it
//! too rather than in a second file the player would have to find.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::error::ProviderError;
use super::opgg::{OpggConfig, OpggProvider};
use super::riot::{RiotConfig, RiotProvider};
use super::BuildDataProvider;

/// Environment override, mainly for development and for CI smoke tests.
pub const PROVIDER_ENV_VAR: &str = "SPELLCHECK_PROVIDER";

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
    /// Where the League client's lockfile is, for an install the app cannot
    /// find on its own. Absent on almost every machine: the watcher looks in
    /// the installer's own records and the platform default first, and this
    /// is for the install those miss. Tried before them when set, after the
    /// `SPELLCHECK_LOCKFILE` environment override. Written by hand today; a
    /// settings screen would write the same field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lockfile: Option<PathBuf>,
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

    /// `SPELLCHECK_PROVIDER` wins over the file when set.
    pub fn apply_env_overrides(&mut self) -> Result<(), ProviderError> {
        if let Ok(raw) = std::env::var(PROVIDER_ENV_VAR) {
            if !raw.trim().is_empty() {
                self.provider = ProviderKind::parse(&raw)?;
            }
        }
        Ok(())
    }

    /// Anchor relative provider paths to `base`.
    ///
    /// A relative `data_root` means "the data the app ships with", and that
    /// only resolves against a repo checkout. A bundled app has no such
    /// working directory — launched from Finder it is `/` — so the path has
    /// to be re-hung on something the app carries with it, which is the
    /// caller's job because it is the only layer that knows where that is.
    ///
    /// Two things are deliberately left alone. An absolute path was written
    /// by somebody on purpose and is not ours to move. And an anchored path
    /// that does not exist is discarded rather than kept, so running from a
    /// checkout still finds `data/builds` in the working directory the way it
    /// always has.
    pub fn anchor_relative_paths(&mut self, base: &Path) {
        if self.riot.data_root.is_relative() {
            let anchored = base.join(&self.riot.data_root);
            if anchored.is_dir() {
                self.riot.data_root = anchored;
            }
        }
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
///
/// Relative on purpose: the ingest tooling runs from the checkout and means
/// exactly this path. The app cannot use it as-is, which is what
/// [`ProviderConfig::anchor_relative_paths`] exists to correct at startup.
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
        assert_eq!(config.lockfile, None);
    }

    #[test]
    fn the_lockfile_setting_is_read_as_written() {
        // What a Windows tester with League on D: types into providers.json.
        // Backslashes are doubled because it is JSON, and nothing here should
        // try to be clever about that.
        let config: ProviderConfig =
            serde_json::from_str(r#"{ "lockfile": "D:\\Games\\League of Legends\\lockfile" }"#)
                .unwrap();
        assert_eq!(
            config.lockfile.as_deref(),
            Some(Path::new(r"D:\Games\League of Legends\lockfile"))
        );
        assert_eq!(config.provider, ProviderKind::Opgg, "the rest still defaults");
    }

    #[test]
    fn an_absent_lockfile_setting_is_not_written_out() {
        // A settings screen that saves the file must not leave `"lockfile":
        // null` behind for the player to wonder about.
        let text = serde_json::to_string(&ProviderConfig::default()).unwrap();
        assert!(!text.contains("lockfile"), "{text}");
    }

    #[test]
    fn a_relative_data_root_is_anchored_to_the_bundle() {
        let base = std::env::temp_dir().join("spellcheck-anchor-test");
        let builds = base.join("data/builds");
        std::fs::create_dir_all(&builds).unwrap();

        let mut config = ProviderConfig::default();
        assert!(config.riot.data_root.is_relative());
        config.anchor_relative_paths(&base);
        assert_eq!(config.riot.data_root, builds);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn an_absolute_data_root_is_left_where_it_was_put() {
        // Somebody wrote this into providers.json deliberately. Re-hanging it
        // on the bundle would silently ignore what they asked for.
        let mut config = ProviderConfig::default();
        config.riot.data_root = PathBuf::from("/opt/spellcheck/builds");
        config.anchor_relative_paths(Path::new("/Applications/x.app/Contents/Resources"));
        assert_eq!(config.riot.data_root, PathBuf::from("/opt/spellcheck/builds"));
    }

    #[test]
    fn anchoring_somewhere_the_data_is_not_changes_nothing() {
        // The dev case: running from a checkout, where the resource directory
        // is a build folder with no data in it. Falling back to the relative
        // path keeps `cargo run` from the repo root working.
        let mut config = ProviderConfig::default();
        let before = config.riot.data_root.clone();
        config.anchor_relative_paths(Path::new("/nonexistent/bundle/Resources"));
        assert_eq!(config.riot.data_root, before);
    }

    #[test]
    fn provider_is_a_config_value() {
        let config: ProviderConfig = serde_json::from_str(r#"{ "provider": "riot" }"#).unwrap();
        assert_eq!(config.provider, ProviderKind::Riot);
        assert_eq!(config.active_provider().unwrap().label(), "spellcheck");
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
