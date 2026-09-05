//! Build data providers.
//!
//! One trait, [`BuildDataProvider`], and one result shape,
//! [`BuildLookup`](schema::BuildLookup). Which implementation is live is a
//! config value ([`config::ProviderConfig`]); the UI receives identical JSON
//! either way and must never branch on the source.
//!
//! # Adding a provider
//!
//! 1. Add a module here with a `map.rs` that builds a
//!    [`ChampionBuild`](schema::ChampionBuild) using the [`mapping`] helpers.
//! 2. Implement [`BuildDataProvider`] for it.
//! 3. Add a variant to [`config::ProviderKind`] and an arm to its
//!    `instantiate` match — the only place that names concrete providers.
//!
//! No schema type and no UI code changes.

pub mod config;
pub mod error;
pub mod mapping;
pub mod opgg;
pub mod riot;
pub mod role;
pub mod schema;

pub use config::{ProviderConfig, ProviderKind};
pub use error::ProviderError;
pub use opgg::OpggProvider;
pub use riot::RiotProvider;
pub use role::Role;
pub use schema::{
    BuildLookup, BuildRequest, BuildStats, ChampionBuild, ChampionRef, ItemGroup, ItemPlan,
    ItemRef, LaneAdvantage, MatchupInfo, NoData, RunePage, Skill, SkillPlan, SourceInfo,
    SummonerSet, SummonerSpell,
};

use async_trait::async_trait;

/// A source of champion-role build data.
///
/// Implementations must be cheap to clone-share (`Arc<dyn BuildDataProvider>`)
/// and safe to call concurrently: champ select can ask for a build while the
/// user is still hovering.
#[async_trait]
pub trait BuildDataProvider: Send + Sync {
    /// Attribution string surfaced as
    /// [`SourceInfo::provider_label`](schema::SourceInfo::provider_label).
    fn label(&self) -> &str;

    /// Look up the build for one champion-role pair.
    ///
    /// Returns [`BuildLookup::NoData`](schema::BuildLookup::NoData) when the
    /// source simply has nothing for this pair — that is an expected answer,
    /// not a failure. `Err` is reserved for things that actually went wrong.
    ///
    /// # Answering a matchup request
    ///
    /// [`BuildRequest::opponent_key`](schema::BuildRequest::opponent_key) may
    /// name a lane opponent. A source that cannot filter by opponent must
    /// answer with the ordinary build and leave
    /// [`ChampionBuild::matchup`](schema::ChampionBuild::matchup) empty — not
    /// fail, and not fill the field in from the request. The field is the
    /// only thing that distinguishes a build that really is about this
    /// matchup from one that merely was asked about it, and the UI labels the
    /// build on that field alone.
    async fn fetch_build(&self, request: &BuildRequest) -> Result<BuildLookup, ProviderError>;

    /// The lane this champion is actually played in most.
    ///
    /// Asked only when nothing else named one. Practice Tool, customs, ARAM
    /// and blind pick before the assignment lands all leave the lane blank,
    /// and a build is filed by champion *and* lane, so without this the app
    /// has to say nothing at all — which is the least useful thing it can do
    /// when it already knows the champion you are playing.
    ///
    /// `None` means the source cannot say, and the caller shows nothing
    /// rather than inventing a lane. The default is `None` so a new provider
    /// is never silently guessing on our behalf.
    async fn primary_role(&self, champion_key: &str) -> Result<Option<Role>, ProviderError> {
        let _ = champion_key;
        Ok(None)
    }
}

/// Champion keys become path segments and query values, so they are validated
/// before they reach a filesystem or a URL. Data Dragon keys are ASCII
/// alphanumeric (`Ahri`, `MonkeyKing`, `Kaisa`, `Fiddlesticks`) with no
/// separators, dots, or whitespace.
pub fn validate_champion_key(key: &str) -> Result<&str, ProviderError> {
    let invalid = key.is_empty()
        || key.len() > 32
        || !key.chars().all(|c| c.is_ascii_alphanumeric());

    if invalid {
        Err(ProviderError::InvalidChampionKey(key.to_string()))
    } else {
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_real_champion_keys() {
        for key in ["Ahri", "MonkeyKing", "Kaisa", "Fiddlesticks", "Nunu"] {
            assert!(validate_champion_key(key).is_ok(), "rejected {key}");
        }
    }

    #[test]
    fn rejects_path_traversal_and_junk() {
        for key in ["", "..", "../../etc/passwd", "Ahri/../Zed", "Kai'Sa", "Dr. Mundo", "a b"] {
            assert!(validate_champion_key(key).is_err(), "accepted {key:?}");
        }
        assert!(validate_champion_key(&"A".repeat(33)).is_err());
    }
}
