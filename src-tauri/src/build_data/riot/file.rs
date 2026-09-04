//! The on-disk format of `data/builds/{Champion}/{role}.json`.
//!
//! These structs are deliberately separate from the internal schema. The files
//! are committed artefacts produced by CI, so their shape is a contract with
//! the crawler and is versioned; the internal schema stays free to change.
//! `schemaVersion` is what lets an older app refuse a newer file cleanly
//! instead of silently mis-reading it.

use serde::{Deserialize, Serialize};

use crate::build_data::role::Role;
use crate::build_data::schema::{
    BuildRequest, BuildStats, ChampionBuild, ChampionRef, ItemGroup, ItemPlan, ItemRef, RunePage,
    Skill, SkillPlan, SourceInfo, SummonerSet, SummonerSpell,
};

/// Bumped whenever the crawler changes the file shape incompatibly.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BuildFile {
    pub schema_version: u32,
    pub champion: Option<FileChampion>,
    pub role: Option<Role>,
    pub patch: Option<String>,
    pub region: Option<String>,
    pub tier: Option<String>,
    pub updated_at: Option<String>,
    pub stats: Option<FileStats>,
    pub items: FileItems,
    pub runes: Vec<FileRunePage>,
    pub summoners: Vec<FileSummonerSet>,
    pub skills: FileSkills,
}

impl Default for BuildFile {
    fn default() -> Self {
        BuildFile {
            schema_version: SCHEMA_VERSION,
            champion: None,
            role: None,
            patch: None,
            region: None,
            tier: None,
            updated_at: None,
            stats: None,
            items: FileItems::default(),
            runes: Vec::new(),
            summoners: Vec::new(),
            skills: FileSkills::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FileChampion {
    pub key: Option<String>,
    pub name: Option<String>,
    pub id: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FileStats {
    pub games: Option<u64>,
    pub win_rate: Option<f64>,
    pub pick_rate: Option<f64>,
    pub ban_rate: Option<f64>,
}

impl From<FileStats> for Option<BuildStats> {
    fn from(stats: FileStats) -> Self {
        BuildStats {
            games: stats.games,
            win_rate: stats.win_rate,
            pick_rate: stats.pick_rate,
            ban_rate: stats.ban_rate,
        }
        .non_empty()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FileItems {
    pub starters: Vec<FileItemGroup>,
    pub boots: Vec<FileItemGroup>,
    pub core: Vec<FileItemGroup>,
    pub situational: Vec<FileItemGroup>,
}

/// Ids only — names come from Data Dragon at render time, so the committed
/// files stay small and never go stale on a rename.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FileItemGroup {
    pub items: Vec<u32>,
    pub label: Option<String>,
    #[serde(flatten)]
    pub stats: FileStats,
}

impl From<FileItemGroup> for ItemGroup {
    fn from(group: FileItemGroup) -> Self {
        ItemGroup {
            items: group.items.into_iter().map(ItemRef::new).collect(),
            stats: group.stats.into(),
            label: group.label,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FileRunePage {
    pub primary_style: Option<u32>,
    pub secondary_style: Option<u32>,
    pub primary: Vec<u32>,
    pub secondary: Vec<u32>,
    pub shards: Vec<u32>,
    pub label: Option<String>,
    #[serde(flatten)]
    pub stats: FileStats,
}

impl From<FileRunePage> for RunePage {
    fn from(page: FileRunePage) -> Self {
        RunePage {
            primary_style: page.primary_style,
            secondary_style: page.secondary_style,
            primary: page.primary,
            secondary: page.secondary,
            shards: page.shards,
            stats: page.stats.into(),
            label: page.label,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FileSummonerSet {
    pub spells: Vec<u32>,
    #[serde(flatten)]
    pub stats: FileStats,
}

impl From<FileSummonerSet> for SummonerSet {
    fn from(set: FileSummonerSet) -> Self {
        SummonerSet {
            spells: set
                .spells
                .into_iter()
                .map(|id| SummonerSpell { id, name: None })
                .collect(),
            stats: set.stats.into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FileSkills {
    pub priority: Vec<Skill>,
    pub order: Vec<Skill>,
}

impl From<FileSkills> for SkillPlan {
    fn from(skills: FileSkills) -> Self {
        SkillPlan {
            priority: skills.priority,
            order: skills.order,
        }
    }
}

impl BuildFile {
    /// Convert into the internal schema.
    ///
    /// `provider_label` is passed in so the file itself never has to carry
    /// attribution — the provider owns that.
    pub fn into_build(self, request: &BuildRequest, provider_label: &str) -> ChampionBuild {
        let champion = self.champion.unwrap_or_default();

        ChampionBuild {
            champion: ChampionRef {
                key: champion.key.unwrap_or_else(|| request.champion_key.clone()),
                name: champion
                    .name
                    .unwrap_or_else(|| request.display_name().to_string()),
                id: champion.id.or(request.champion_id),
            },
            role: self.role.unwrap_or(request.role),
            source: SourceInfo {
                provider_label: provider_label.to_string(),
                patch: self.patch,
                region: self.region,
                tier: self.tier,
                updated_at: self.updated_at,
            },
            stats: self.stats.and_then(|stats| stats.into()),
            items: ItemPlan {
                starters: self.items.starters.into_iter().map(Into::into).collect(),
                boots: self.items.boots.into_iter().map(Into::into).collect(),
                core: self.items.core.into_iter().map(Into::into).collect(),
                situational: self.items.situational.into_iter().map(Into::into).collect(),
            },
            runes: self.runes.into_iter().map(Into::into).collect(),
            summoners: self.summoners.into_iter().map(Into::into).collect(),
            skills: self.skills.into(),
        }
    }
}
