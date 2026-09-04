//! Reads build data we crawled ourselves, from
//! `data/builds/{Champion}/{role}.json`.
//!
//! This is the distributable path: everything here derives from Riot's own
//! API and Data Dragon, so it ships with the app. Files are read one champion
//! at a time and never held after the lookup returns — the full dataset must
//! not sit in memory.

pub mod file;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::config::default_build_data_root;
use super::error::ProviderError;
use super::schema::{BuildLookup, BuildRequest};
use super::{validate_champion_key, BuildDataProvider};

use file::{BuildFile, SCHEMA_VERSION};

pub const PROVIDER_LABEL: &str = "leaguechecker";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RiotConfig {
    /// Directory holding `{Champion}/{role}.json`.
    pub data_root: PathBuf,
    /// Attribution shown in the UI.
    pub label: String,
}

impl Default for RiotConfig {
    fn default() -> Self {
        RiotConfig {
            data_root: default_build_data_root(),
            label: PROVIDER_LABEL.to_string(),
        }
    }
}

pub struct RiotProvider {
    config: RiotConfig,
}

impl RiotProvider {
    pub fn new(config: RiotConfig) -> Self {
        RiotProvider { config }
    }

    /// Point the provider at a specific data directory.
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        RiotProvider::new(RiotConfig {
            data_root: root.into(),
            ..RiotConfig::default()
        })
    }

    pub fn data_root(&self) -> &Path {
        &self.config.data_root
    }

    /// `{data_root}/{Champion}/{role}.json`.
    ///
    /// The champion key is validated first: it arrives from the LCU and ends
    /// up as a path segment, so `..` and separators must never get through.
    pub fn build_path(&self, request: &BuildRequest) -> Result<PathBuf, ProviderError> {
        let champion = validate_champion_key(&request.champion_key)?;
        Ok(self
            .config
            .data_root
            .join(champion)
            .join(format!("{}.json", request.role.file_stem())))
    }
}

#[async_trait]
impl BuildDataProvider for RiotProvider {
    fn label(&self) -> &str {
        &self.config.label
    }

    async fn fetch_build(&self, request: &BuildRequest) -> Result<BuildLookup, ProviderError> {
        let path = self.build_path(request)?;

        let text = match tokio::fs::read_to_string(&path).await {
            Ok(text) => text,
            // Neither a missing file nor a missing champion directory is a
            // failure: our crawl is incremental, so "not yet" is the normal
            // answer for most pairs early on.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BuildLookup::no_data(
                    request,
                    format!(
                        "no build data for {} {} yet",
                        request.display_name(),
                        request.role
                    ),
                ));
            }
            Err(error) => {
                return Err(ProviderError::Io {
                    path: path.display().to_string(),
                    detail: error.to_string(),
                })
            }
        };

        // An empty or whitespace-only file is what a half-finished CI write
        // leaves behind. Treat it as absent rather than as corruption.
        if text.trim().is_empty() {
            return Ok(BuildLookup::no_data(
                request,
                format!(
                    "no build data for {} {} yet",
                    request.display_name(),
                    request.role
                ),
            ));
        }

        let parsed: BuildFile =
            serde_json::from_str(&text).map_err(|error| ProviderError::MalformedFile {
                path: path.display().to_string(),
                detail: error.to_string(),
            })?;

        if parsed.schema_version > SCHEMA_VERSION {
            return Err(ProviderError::MalformedFile {
                path: path.display().to_string(),
                detail: format!(
                    "schema version {} is newer than this app understands \
                     (it reads up to {SCHEMA_VERSION}); update leaguechecker",
                    parsed.schema_version
                ),
            });
        }

        if let Some(role) = parsed.role {
            if role != request.role {
                return Err(ProviderError::MalformedFile {
                    path: path.display().to_string(),
                    detail: format!("file declares role {role} but is filed under {}", request.role),
                });
            }
        }

        let build = parsed.into_build(request, self.label());

        // A file that parses but carries no items is not a usable build.
        if build.items.is_empty() {
            return Ok(BuildLookup::no_data(
                request,
                format!(
                    "build data for {} {} has no items yet",
                    request.display_name(),
                    request.role
                ),
            ));
        }

        Ok(BuildLookup::found(build))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_data::role::Role;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Minimal scratch directory; avoids pulling in a tempdir dependency.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let unique = format!(
                "leaguechecker-test-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let path = std::env::temp_dir().join(unique);
            std::fs::create_dir_all(&path).unwrap();
            Scratch(path)
        }

        fn write(&self, champion: &str, role: Role, contents: &str) {
            let dir = self.0.join(champion);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{}.json", role.file_stem())), contents).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const AHRI_MIDDLE: &str = r#"{
        "schemaVersion": 1,
        "champion": { "key": "Ahri", "name": "Ahri", "id": 103 },
        "role": "middle",
        "patch": "14.18",
        "updatedAt": "2026-09-01T00:00:00Z",
        "stats": { "games": 120000, "winRate": 0.518 },
        "items": {
            "starters": [{ "items": [1056, 2003], "games": 90000, "winRate": 0.52 }],
            "boots": [{ "items": [3020] }],
            "core": [{ "items": [6655, 3020, 3089], "games": 40000, "winRate": 0.535 }]
        },
        "runes": [{
            "primaryStyle": 8100,
            "secondaryStyle": 8200,
            "primary": [8112, 8139, 8138, 8106],
            "secondary": [8226, 8210],
            "shards": [5008, 5008, 5001]
        }],
        "summoners": [{ "spells": [4, 12], "winRate": 0.53 }],
        "skills": { "priority": ["Q", "E", "W"], "order": ["Q", "E", "W", "Q"] }
    }"#;

    #[tokio::test]
    async fn reads_a_committed_build() {
        let scratch = Scratch::new();
        scratch.write("Ahri", Role::Middle, AHRI_MIDDLE);
        let provider = RiotProvider::with_root(&scratch.0);

        let lookup = provider
            .fetch_build(&BuildRequest::new("Ahri", Role::Middle))
            .await
            .unwrap();

        let build = lookup.build().expect("expected a build");
        assert_eq!(build.champion.id, Some(103));
        assert_eq!(build.role, Role::Middle);
        assert_eq!(build.source.provider_label, PROVIDER_LABEL);
        assert_eq!(build.source.patch.as_deref(), Some("14.18"));
        assert_eq!(build.stats.unwrap().win_rate, Some(0.518));
        assert_eq!(build.items.core[0].items.len(), 3);
        assert_eq!(build.items.core[0].stats.unwrap().games, Some(40000));
        assert_eq!(build.runes[0].shards, vec![5008, 5008, 5001]);
        assert_eq!(build.summoners[0].spells[0].id, 4);
        assert_eq!(build.skills.priority.len(), 3);
    }

    #[tokio::test]
    async fn missing_file_is_no_data_not_an_error() {
        let scratch = Scratch::new();
        let provider = RiotProvider::with_root(&scratch.0);

        let lookup = provider
            .fetch_build(&BuildRequest::new("Zed", Role::Middle))
            .await
            .unwrap();

        match lookup {
            BuildLookup::NoData(no_data) => {
                assert_eq!(no_data.champion_key, "Zed");
                assert_eq!(no_data.role, Role::Middle);
                assert!(no_data.detail.contains("no build data"));
            }
            BuildLookup::Found(_) => panic!("expected no data"),
        }
    }

    #[tokio::test]
    async fn missing_champion_directory_is_no_data() {
        let scratch = Scratch::new();
        scratch.write("Ahri", Role::Middle, AHRI_MIDDLE);
        let provider = RiotProvider::with_root(&scratch.0);

        // Champion exists, role does not.
        let lookup = provider
            .fetch_build(&BuildRequest::new("Ahri", Role::Top))
            .await
            .unwrap();
        assert!(!lookup.is_found());

        // Data root itself does not exist.
        let provider = RiotProvider::with_root(scratch.0.join("nope"));
        let lookup = provider
            .fetch_build(&BuildRequest::new("Ahri", Role::Middle))
            .await
            .unwrap();
        assert!(!lookup.is_found());
    }

    #[tokio::test]
    async fn empty_file_is_no_data() {
        let scratch = Scratch::new();
        scratch.write("Ahri", Role::Middle, "   \n");
        let provider = RiotProvider::with_root(&scratch.0);

        let lookup = provider
            .fetch_build(&BuildRequest::new("Ahri", Role::Middle))
            .await
            .unwrap();
        assert!(!lookup.is_found());
    }

    #[tokio::test]
    async fn corrupt_file_is_an_error() {
        let scratch = Scratch::new();
        scratch.write("Ahri", Role::Middle, "{ not json");
        let provider = RiotProvider::with_root(&scratch.0);

        let error = provider
            .fetch_build(&BuildRequest::new("Ahri", Role::Middle))
            .await
            .unwrap_err();
        assert!(matches!(error, ProviderError::MalformedFile { .. }));
    }

    #[tokio::test]
    async fn refuses_a_newer_schema_version() {
        let scratch = Scratch::new();
        scratch.write(
            "Ahri",
            Role::Middle,
            r#"{ "schemaVersion": 99, "items": { "core": [{ "items": [1] }] } }"#,
        );
        let provider = RiotProvider::with_root(&scratch.0);

        let error = provider
            .fetch_build(&BuildRequest::new("Ahri", Role::Middle))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("newer than this app"));
    }

    #[tokio::test]
    async fn rejects_a_champion_key_that_escapes_the_data_root() {
        let scratch = Scratch::new();
        let provider = RiotProvider::with_root(&scratch.0);

        let error = provider
            .fetch_build(&BuildRequest::new("../../etc/passwd", Role::Middle))
            .await
            .unwrap_err();
        assert!(matches!(error, ProviderError::InvalidChampionKey(_)));
    }

    #[tokio::test]
    async fn a_build_with_no_items_is_no_data() {
        let scratch = Scratch::new();
        scratch.write("Ahri", Role::Middle, r#"{ "schemaVersion": 1 }"#);
        let provider = RiotProvider::with_root(&scratch.0);

        let lookup = provider
            .fetch_build(&BuildRequest::new("Ahri", Role::Middle))
            .await
            .unwrap();
        assert!(!lookup.is_found());
    }
}
