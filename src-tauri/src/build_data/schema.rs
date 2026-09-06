//! The internal build schema.
//!
//! This is the only build shape the UI ever sees. Providers translate *into*
//! it; nothing in here may name or hint at a particular source. The one
//! source-flavoured field is [`SourceInfo::provider_label`], which exists so we
//! can render the attribution a data source requires — the UI renders that
//! string and must never branch on its value.

use serde::{Deserialize, Serialize};

use super::role::Role;

/// What the caller wants a build for. Built once in champ select from the LCU
/// session and handed to whichever provider is active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRequest {
    /// Data Dragon key, e.g. `Ahri`, `MonkeyKing`, `Kaisa`.
    pub champion_key: String,
    /// Display name, e.g. `Wukong`. Optional: not every caller knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub champion_name: Option<String>,
    /// Riot numeric champion id from `myTeam[].championId`, when we have it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub champion_id: Option<u32>,
    pub role: Role,
    /// The lane opponent to build *against*, as a Data Dragon key.
    ///
    /// `None` asks the ordinary question — the build for this champion in
    /// this lane against the field — and is what the search box sends unless
    /// the user names somebody. `Some` asks a narrower one, and a provider
    /// that cannot answer it says so by leaving
    /// [`ChampionBuild::matchup`] empty rather than by failing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opponent_key: Option<String>,
}

impl BuildRequest {
    pub fn new(champion_key: impl Into<String>, role: Role) -> Self {
        BuildRequest {
            champion_key: champion_key.into(),
            champion_name: None,
            champion_id: None,
            role,
            opponent_key: None,
        }
    }

    pub fn with_champion_id(mut self, id: u32) -> Self {
        self.champion_id = Some(id);
        self
    }

    pub fn with_champion_name(mut self, name: impl Into<String>) -> Self {
        self.champion_name = Some(name.into());
        self
    }

    /// Ask against one opponent. An empty or whitespace-only key is the same
    /// as naming nobody — the UI sends a blank field rather than omitting it.
    pub fn with_opponent(mut self, opponent_key: impl Into<String>) -> Self {
        let key = opponent_key.into();
        self.opponent_key = (!key.trim().is_empty()).then_some(key);
        self
    }

    /// The opponent this request asked about, if any.
    pub fn opponent(&self) -> Option<&str> {
        self.opponent_key.as_deref()
    }

    /// Name for display, falling back to the key.
    pub fn display_name(&self) -> &str {
        self.champion_name.as_deref().unwrap_or(&self.champion_key)
    }
}

/// The result of a lookup. "We have nothing for this pair yet" is a normal
/// answer, not an error, so the UI renders one enum with two arms.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum BuildLookup {
    Found(Box<ChampionBuild>),
    NoData(NoData),
}

impl BuildLookup {
    pub fn found(build: ChampionBuild) -> Self {
        BuildLookup::Found(Box::new(build))
    }

    pub fn no_data(request: &BuildRequest, detail: impl Into<String>) -> Self {
        BuildLookup::NoData(NoData {
            champion_key: request.champion_key.clone(),
            champion_name: request.display_name().to_string(),
            role: request.role,
            detail: detail.into(),
        })
    }

    pub fn build(&self) -> Option<&ChampionBuild> {
        match self {
            BuildLookup::Found(build) => Some(build),
            BuildLookup::NoData(_) => None,
        }
    }

    pub fn is_found(&self) -> bool {
        matches!(self, BuildLookup::Found(_))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoData {
    pub champion_key: String,
    pub champion_name: String,
    pub role: Role,
    /// Human-readable reason, e.g. "no crawled data for Ahri middle yet".
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampionBuild {
    pub champion: ChampionRef,
    pub role: Role,
    pub source: SourceInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<BuildStats>,
    /// Set only when this build really is filtered to one opponent.
    ///
    /// The load-bearing rule of this field is that its absence is
    /// meaningful. A provider with no matchup data answers a matchup request
    /// with the ordinary build and leaves this `None`, and the UI then says
    /// it is showing the general build. Filling it in from a request rather
    /// than from an answer would let a general build be labelled "vs Zed",
    /// which is the same class of mistake as putting a rule on the teal rail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matchup: Option<MatchupInfo>,
    pub items: ItemPlan,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runes: Vec<RunePage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summoners: Vec<SummonerSet>,
    #[serde(default)]
    pub skills: SkillPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampionRef {
    pub key: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u32>,
}

impl ChampionRef {
    pub fn from_request(request: &BuildRequest) -> Self {
        ChampionRef {
            key: request.champion_key.clone(),
            name: request.display_name().to_string(),
            id: request.champion_id,
        }
    }
}

/// Who has the upper hand in a lane, as the source reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LaneAdvantage {
    /// The champion the build is for.
    Ours,
    /// The opponent.
    Theirs,
    /// Neither, or the source named a champion in neither seat.
    Even,
}

/// What makes a build a *matchup* build.
///
/// Present only on a build a source genuinely filtered to one opponent — see
/// [`ChampionBuild::matchup`]. Every field but `opponent` is optional,
/// because a source that can filter a build is not thereby promising to have
/// an opinion about the lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchupInfo {
    /// Who the build is against.
    pub opponent: ChampionRef,
    /// The source's own prose about playing this lane, rendered verbatim and
    /// attributed. It is neither a statistic nor one of our rules, so it is
    /// drawn as neither — see the colour law in `docs/blueprint.html`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane_advantage: Option<LaneAdvantage>,
    /// How the source suggests playing the lane — `even`, `aggressive`,
    /// `defensive`. Kept as the source's own word rather than an enum: it is
    /// displayed, never branched on, and a vocabulary we do not control is
    /// not one to bake into a type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub play_style: Option<String>,
}

/// Provenance for the build, for display and for the "is this a statistic or a
/// rule?" distinction the recommendation engine has to make.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    /// Attribution string to render as-is. Never branch on this.
    pub provider_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    /// ISO-8601 timestamp of when the underlying sample was taken, when the
    /// source tells us. Kept as a string: no date crate for one display field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Rates are fractions in `0.0..=1.0`, never percentages — normalise at the
/// provider boundary with [`super::mapping::as_rate`].
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStats {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub games: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub win_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pick_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ban_rate: Option<f64>,
}

impl BuildStats {
    pub fn is_empty(&self) -> bool {
        self.games.is_none()
            && self.win_rate.is_none()
            && self.pick_rate.is_none()
            && self.ban_rate.is_none()
    }

    /// `None` rather than a struct full of `None`s, so the UI can tell
    /// "no sample" from "a sample with nothing in it".
    pub fn non_empty(self) -> Option<BuildStats> {
        if self.is_empty() {
            None
        } else {
            Some(self)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemPlan {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub starters: Vec<ItemGroup>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boots: Vec<ItemGroup>,
    /// Ordered core paths, best first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub core: Vec<ItemGroup>,
    /// Late / situational options the recommendation engine can draw from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub situational: Vec<ItemGroup>,
}

impl ItemPlan {
    pub fn is_empty(&self) -> bool {
        self.starters.is_empty()
            && self.boots.is_empty()
            && self.core.is_empty()
            && self.situational.is_empty()
    }
}

/// One buildable option: a single item, or an ordered path like
/// `[Eclipse, Ionian Boots, Serylda's]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemGroup {
    pub items: Vec<ItemRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<BuildStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl ItemGroup {
    pub fn new(items: Vec<ItemRef>) -> Self {
        ItemGroup {
            items,
            stats: None,
            label: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemRef {
    pub id: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ItemRef {
    pub fn new(id: u32) -> Self {
        ItemRef { id, name: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunePage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_style: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_style: Option<u32>,
    /// Perk ids in slot order, keystone first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub primary: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secondary: Vec<u32>,
    /// Offense / flex / defense stat shards.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shards: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<BuildStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Default for RunePage {
    fn default() -> Self {
        RunePage {
            primary_style: None,
            secondary_style: None,
            primary: Vec::new(),
            secondary: Vec::new(),
            shards: Vec::new(),
            stats: None,
            label: None,
        }
    }
}

impl RunePage {
    pub fn is_empty(&self) -> bool {
        self.primary.is_empty() && self.secondary.is_empty() && self.shards.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummonerSet {
    pub spells: Vec<SummonerSpell>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<BuildStats>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummonerSpell {
    pub id: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Skill {
    Q,
    W,
    E,
    R,
}

impl Skill {
    pub fn parse(raw: &str) -> Option<Skill> {
        match raw.trim().to_ascii_uppercase().as_str() {
            "Q" | "1" => Some(Skill::Q),
            "W" | "2" => Some(Skill::W),
            "E" | "3" => Some(Skill::E),
            "R" | "4" => Some(Skill::R),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPlan {
    /// Max order, e.g. `[Q, E, W]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub priority: Vec<Skill>,
    /// Level-by-level order, index 0 = level 1. Up to 18 entries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<Skill>,
}

impl SkillPlan {
    pub fn is_empty(&self) -> bool {
        self.priority.is_empty() && self.order.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_build() -> ChampionBuild {
        ChampionBuild {
            champion: ChampionRef {
                key: "Ahri".into(),
                name: "Ahri".into(),
                id: Some(103),
            },
            role: Role::Middle,
            source: SourceInfo {
                provider_label: "test".into(),
                ..SourceInfo::default()
            },
            stats: BuildStats {
                games: Some(1000),
                win_rate: Some(0.52),
                ..BuildStats::default()
            }
            .non_empty(),
            matchup: None,
            items: ItemPlan {
                core: vec![ItemGroup::new(vec![ItemRef::new(6655)])],
                ..ItemPlan::default()
            },
            runes: vec![],
            summoners: vec![],
            skills: SkillPlan {
                priority: vec![Skill::Q, Skill::E, Skill::W],
                order: vec![],
            },
        }
    }

    #[test]
    fn lookup_is_tagged_for_the_ui() {
        let found = BuildLookup::found(sample_build());
        let json = serde_json::to_value(&found).unwrap();
        assert_eq!(json["status"], "found");
        assert_eq!(json["champion"]["key"], "Ahri");
        assert_eq!(json["role"], "middle");

        let request = BuildRequest::new("Ahri", Role::Middle);
        let missing = BuildLookup::no_data(&request, "nothing yet");
        let json = serde_json::to_value(&missing).unwrap();
        assert_eq!(json["status"], "noData");
        assert_eq!(json["detail"], "nothing yet");
    }

    #[test]
    fn lookup_round_trips() {
        let found = BuildLookup::found(sample_build());
        let encoded = serde_json::to_string(&found).unwrap();
        let decoded: BuildLookup = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, found);
    }

    /// The search box sends whatever is in the field, and an empty field is
    /// not a request to build against nobody — it is the ordinary question.
    #[test]
    fn a_blank_opponent_is_no_opponent() {
        let request = BuildRequest::new("Ahri", Role::Middle);
        assert_eq!(request.opponent(), None);

        assert_eq!(request.clone().with_opponent("Zed").opponent(), Some("Zed"));
        assert_eq!(request.clone().with_opponent("").opponent(), None);
        assert_eq!(request.clone().with_opponent("   ").opponent(), None);
    }

    /// A matchup build says so in the JSON the page reads, and an ordinary
    /// one leaves the field out entirely rather than sending a null the page
    /// would have to tell apart from an absent one.
    #[test]
    fn the_matchup_reaches_the_page_only_when_there_is_one() {
        let ordinary = serde_json::to_value(BuildLookup::found(sample_build())).unwrap();
        assert!(ordinary.get("matchup").is_none());

        let mut build = sample_build();
        build.matchup = Some(MatchupInfo {
            opponent: ChampionRef {
                key: "Zed".into(),
                name: "Zed".into(),
                id: None,
            },
            tip: None,
            lane_advantage: Some(LaneAdvantage::Theirs),
            play_style: Some("even".into()),
        });
        let json = serde_json::to_value(BuildLookup::found(build)).unwrap();
        assert_eq!(json["matchup"]["opponent"]["name"], "Zed");
        assert_eq!(json["matchup"]["laneAdvantage"], "theirs");
    }

    #[test]
    fn empty_stats_collapse_to_none() {
        assert!(BuildStats::default().non_empty().is_none());
        assert!(BuildStats {
            games: Some(1),
            ..BuildStats::default()
        }
        .non_empty()
        .is_some());
    }
}
