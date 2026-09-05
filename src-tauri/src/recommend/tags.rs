//! The two tag files, and the vocabulary the checks reason in.
//!
//! `data/meta/champions.json` says what a champion *is* — how it deals damage
//! and which of a handful of threats it represents. `data/meta/items.json`
//! says what an item *answers*. Neither file describes a build; together they
//! are the entire input to the rule-based checks.
//!
//! Both are compiled into the binary with `include_str!`. They are small,
//! constant, and needed on every champ select, so a file read would buy
//! nothing and add a failure path — and unlike build data, which is fetched
//! per champion on demand, there is no version of this that is too big to
//! hold. The parsed form is built once on first use and shared thereafter.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// The files as committed. Compiled in, so a shipped binary and its tags can
/// never disagree.
const CHAMPIONS_JSON: &str = include_str!("../../../data/meta/champions.json");
const ITEMS_JSON: &str = include_str!("../../../data/meta/items.json");

/// Bumped when either file changes shape incompatibly.
pub const TAGS_SCHEMA_VERSION: u32 = 1;

/// How a champion deals its damage. This is what the armour/magic-resist
/// split is computed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DamageType {
    Ad,
    Ap,
    Mixed,
    /// True damage as a champion's defining output. Nothing defends against
    /// it, which is exactly why it has to be nameable.
    True,
}

/// What an item answers. This vocabulary is closed on purpose: a check may
/// only ask for a threat that some item actually solves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Answer {
    Armor,
    Mr,
    Antiheal,
    Tenacity,
    AntiShield,
    AntiCrit,
}

impl Answer {
    /// For rendering inside a reason sentence.
    pub fn describe(self) -> &'static str {
        match self {
            Answer::Armor => "armour",
            Answer::Mr => "magic resist",
            Answer::Antiheal => "antiheal",
            Answer::Tenacity => "tenacity",
            Answer::AntiShield => "shield reduction",
            Answer::AntiCrit => "crit reduction",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampionTags {
    pub id: u32,
    pub name: String,
    pub damage_type: DamageType,
    /// Healing or drain worth spending gold to answer.
    pub sustain: bool,
    /// Stuns, roots, knock-ups, suppressions — the CC tenacity shortens.
    #[serde(rename = "hardCC")]
    pub hard_cc: bool,
    /// Can cross the front line and reach a carry.
    pub dive: bool,
    /// Chips from outside the fight.
    pub poke: bool,
    /// Can hold a front line.
    pub frontline: bool,
    /// Can start a fight on its own terms.
    pub engage: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemTags {
    pub name: String,
    pub cost: u32,
    pub answers: Vec<Answer>,
    /// The item's own offensive stat line, when it has one. Held only so a
    /// suggestion never hands an ability-power item to a champion who deals
    /// physical damage; it is not an answer and no check asks for it.
    #[serde(default)]
    pub damage: Option<DamageType>,
    #[serde(default)]
    pub builds_into: Vec<u32>,
    /// True when the item builds into something. A component is what you buy
    /// on the back that answers a threat *now*, at the cost of a slot later.
    #[serde(default)]
    pub is_component: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChampionFile {
    schema_version: u32,
    /// Champions the roster knows about but nobody has tagged. Listed rather
    /// than omitted so a new release shows up as a known hole instead of
    /// silence — see the test that holds this file to the roster.
    #[serde(default)]
    untagged: Vec<String>,
    champions: HashMap<String, ChampionTags>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemFile {
    schema_version: u32,
    items: HashMap<String, ItemTags>,
}

/// Both files, parsed.
#[derive(Debug)]
pub struct Tags {
    champions: HashMap<String, ChampionTags>,
    by_champion_id: HashMap<u32, String>,
    untagged: Vec<String>,
    items: HashMap<u32, ItemTags>,
}

impl Tags {
    /// The compiled-in tags, parsed once.
    ///
    /// Panics if the committed files do not parse or disagree with the schema
    /// version. That is deliberate: they are build-time constants, so a
    /// failure here is a broken build rather than a runtime condition the app
    /// could sensibly carry on from, and the test suite reaches it first.
    pub fn get() -> &'static Tags {
        static TAGS: OnceLock<Tags> = OnceLock::new();
        TAGS.get_or_init(|| Tags::parse(CHAMPIONS_JSON, ITEMS_JSON).expect("committed tag files"))
    }

    fn parse(champions_json: &str, items_json: &str) -> Result<Tags, String> {
        let champion_file: ChampionFile =
            serde_json::from_str(champions_json).map_err(|e| format!("champions.json: {e}"))?;
        let item_file: ItemFile =
            serde_json::from_str(items_json).map_err(|e| format!("items.json: {e}"))?;

        for (what, version) in [
            ("champions.json", champion_file.schema_version),
            ("items.json", item_file.schema_version),
        ] {
            if version != TAGS_SCHEMA_VERSION {
                return Err(format!(
                    "{what} declares schema version {version}, this build reads {TAGS_SCHEMA_VERSION}"
                ));
            }
        }

        let by_champion_id = champion_file
            .champions
            .iter()
            .map(|(key, tags)| (tags.id, key.clone()))
            .collect();

        let mut items = HashMap::with_capacity(item_file.items.len());
        for (raw_id, tags) in item_file.items {
            let id: u32 = raw_id
                .parse()
                .map_err(|_| format!("items.json: {raw_id} is not an item id"))?;
            items.insert(id, tags);
        }

        Ok(Tags {
            champions: champion_file.champions,
            by_champion_id,
            untagged: champion_file.untagged,
            items,
        })
    }

    /// Tags for a Data Dragon champion key, e.g. `Ahri`, `MonkeyKing`.
    ///
    /// `None` is ordinary: a champion released after the tag file was last
    /// written has no tags, and a check that cannot see a champion simply
    /// says less about it. It never guesses.
    pub fn champion(&self, key: &str) -> Option<&ChampionTags> {
        self.champions.get(key)
    }

    /// Tags for a Riot numeric champion id, which is what champ select sends.
    pub fn champion_by_id(&self, id: u32) -> Option<&ChampionTags> {
        let key = self.by_champion_id.get(&id)?;
        self.champions.get(key)
    }

    pub fn item(&self, id: u32) -> Option<&ItemTags> {
        self.items.get(&id)
    }

    /// Every item answering `answer`, cheapest first. Ties break on id so the
    /// order is stable across runs rather than however the map iterated.
    pub fn items_answering(&self, answer: Answer) -> Vec<(u32, &ItemTags)> {
        let mut found: Vec<(u32, &ItemTags)> = self
            .items
            .iter()
            .filter(|(_, tags)| tags.answers.contains(&answer))
            .map(|(id, tags)| (*id, tags))
            .collect();
        found.sort_by_key(|(id, tags)| (tags.cost, *id));
        found
    }

    /// Champions on the roster that nobody has tagged yet.
    pub fn untagged(&self) -> &[String] {
        &self.untagged
    }

    pub fn champion_count(&self) -> usize {
        self.champions.len()
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_files_parse() {
        let tags = Tags::get();
        assert!(tags.champion_count() > 150, "the roster is nearly complete");
        assert!(tags.item_count() > 40);
    }

    #[test]
    fn champions_resolve_by_key_and_by_id() {
        let tags = Tags::get();

        let ahri = tags.champion("Ahri").expect("Ahri is tagged");
        assert_eq!(ahri.damage_type, DamageType::Ap);
        assert_eq!(ahri.id, 103);
        assert!(ahri.hard_cc, "charm is hard crowd control");

        // The id is the only handle champ select gives us for the other nine
        // players, so it has to reach the same row as the key does.
        assert_eq!(tags.champion_by_id(103), Some(ahri));

        // Wukong is the standing proof that key and display name differ.
        let wukong = tags.champion("MonkeyKing").expect("MonkeyKing is tagged");
        assert_eq!(wukong.name, "Wukong");
    }

    #[test]
    fn an_untagged_champion_is_a_none_rather_than_a_guess() {
        let tags = Tags::get();
        assert!(tags.champion("NotAChampion").is_none());
        assert!(tags.champion_by_id(999_999).is_none());

        // Whatever is listed as untagged must genuinely not be tagged, or the
        // list is lying about the hole it documents.
        for key in tags.untagged() {
            assert!(
                tags.champion(key).is_none(),
                "{key} is listed as untagged but has tags"
            );
        }
    }

    #[test]
    fn every_answer_has_at_least_one_item_behind_it() {
        let tags = Tags::get();
        for answer in [
            Answer::Armor,
            Answer::Mr,
            Answer::Antiheal,
            Answer::Tenacity,
            Answer::AntiShield,
            Answer::AntiCrit,
        ] {
            assert!(
                !tags.items_answering(answer).is_empty(),
                "nothing answers {answer:?}, so a check asking for it can only stay silent"
            );
        }
    }

    #[test]
    fn items_answering_comes_back_cheapest_first() {
        let tags = Tags::get();
        let antiheal = tags.items_answering(Answer::Antiheal);
        let costs: Vec<u32> = antiheal.iter().map(|(_, tags)| tags.cost).collect();
        let mut sorted = costs.clone();
        sorted.sort_unstable();
        assert_eq!(costs, sorted);

        // The cheapest antiheal is a component, which is the whole reason the
        // timing advice can exist: it is buyable on a first back.
        let (_, cheapest) = antiheal.first().expect("something applies Grievous Wounds");
        assert!(cheapest.is_component);
        assert!(cheapest.cost <= 1000);
    }

    #[test]
    fn build_paths_point_at_items_we_know() {
        let tags = Tags::get();
        for (id, item) in &tags.items {
            for into in &item.builds_into {
                assert!(
                    tags.item(*into).is_some(),
                    "{} ({id}) builds into {into}, which is not in the file",
                    item.name
                );
            }
        }
    }

    #[test]
    fn a_file_from_a_newer_build_is_refused_rather_than_misread() {
        let champions = r#"{"schemaVersion": 99, "champions": {}}"#;
        let items = r#"{"schemaVersion": 1, "items": {}}"#;
        let error = Tags::parse(champions, items).unwrap_err();
        assert!(error.contains("schema version 99"), "{error}");
    }
}
