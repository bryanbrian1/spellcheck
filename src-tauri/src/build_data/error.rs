//! Errors shared by every provider.
//!
//! "No build data for this champion-role yet" is deliberately *not* an error —
//! it is the expected answer while our crawl is still filling in, so it travels
//! as [`BuildLookup::NoData`](super::schema::BuildLookup). Errors here mean
//! something actually went wrong.

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("unknown role: {0:?}")]
    UnknownRole(String),

    /// Champion keys become path segments, so anything that is not a plain
    /// Data Dragon key (`Ahri`, `MonkeyKing`, `Kaisa`) is rejected outright.
    #[error("invalid champion key: {0:?}")]
    InvalidChampionKey(String),

    #[error("{provider} is not reachable: {detail}")]
    Transport { provider: &'static str, detail: String },

    #[error("{provider} returned an unexpected response: {detail}")]
    Protocol { provider: &'static str, detail: String },

    #[error("{provider} reported an error: {detail}")]
    Upstream { provider: &'static str, detail: String },

    /// The payload arrived intact but did not contain the fields we map from.
    #[error("could not read a build for {champion} {role} out of the {provider} response: {detail}")]
    Mapping {
        provider: &'static str,
        champion: String,
        role: &'static str,
        detail: String,
    },

    #[error("could not read {path}: {detail}")]
    Io { path: String, detail: String },

    #[error("{path} is not valid build data: {detail}")]
    MalformedFile { path: String, detail: String },

    #[error("provider configuration is invalid: {0}")]
    Config(String),
}

impl ProviderError {
    pub fn transport(provider: &'static str, detail: impl std::fmt::Display) -> Self {
        ProviderError::Transport {
            provider,
            detail: detail.to_string(),
        }
    }

    pub fn protocol(provider: &'static str, detail: impl std::fmt::Display) -> Self {
        ProviderError::Protocol {
            provider,
            detail: detail.to_string(),
        }
    }
}

/// Tauri commands serialize their error type, and the UI only ever shows the
/// message, so a plain string keeps the wire shape identical for every variant.
impl Serialize for ProviderError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
