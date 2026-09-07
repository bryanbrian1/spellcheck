//! Riot's static data: the champion list, and where icon art lives.
//!
//! Two unrelated-looking needs come from one place, so they arrive together
//! rather than as two fetches. The search box needs every champion's display
//! name and the key it is filed under — you type "Wukong", the provider wants
//! `MonkeyKing` — and the tiles need art.
//!
//! Data Dragon serves every icon this app needs, but it does not name them
//! consistently, and that inconsistency is the whole reason this module
//! exists rather than a URL template in the frontend:
//!
//! - **Items** are `img/item/{id}.png`. The id is enough.
//! - **Champions** are `img/champion/{Key}.png` — the Data Dragon key, so
//!   `MonkeyKing.png` rather than `Wukong.png`. The build layer already
//!   speaks in keys, so this is free.
//! - **Summoner spells** are named, not numbered. A match says `4`; the
//!   image is `SummonerFlash.png`, and only `summoner.json` connects them.
//! - **Runes** carry their own path, and it is *unversioned* —
//!   `cdn/img/perk-images/...` with no version segment, unlike everything
//!   else. Only `runesReforged.json` has those paths.
//!
//! So two of the four need a lookup table, and this fetches them once, in the
//! same pass as the champion list.
//!
//! It happens in Rust rather than in the page because the window's content
//! security policy is `default-src 'self'`: the frontend may *display* images
//! from Data Dragon but may not call it, and the UI's only surface has ever
//! been `invoke`. Opening a second one would be a bigger change than this.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

const DDRAGON: &str = "https://ddragon.leagueoflegends.com";

/// Riot's own static data, and the only source in this app that needs no key.
/// Documented and public, which is why CLAUDE.md allows it.
fn versions_url() -> String {
    format!("{DDRAGON}/api/versions.json")
}

/// One champion, as the search box needs them.
///
/// The pair is the whole point: you type the name and the provider is asked
/// for the key, and for a good number of champions those differ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Champion {
    /// Data Dragon key — `MonkeyKing`. What a build is filed under.
    pub key: String,
    /// Display name — `Wukong`. What a player types.
    pub name: String,
}

/// The champion list, plus what the UI needs to build an icon URL.
///
/// Item and champion *art* tables are absent on purpose: those URLs are
/// derivable from an id and a key the UI already holds, so shipping them
/// would be several hundred kilobytes to say what a format string says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataDragon {
    /// The full Data Dragon version, e.g. `16.17.1` — not the `16.17` patch
    /// a build is labelled with. Image paths want the build.
    pub version: String,
    /// Every champion, sorted by display name so the UI can present them in
    /// the order a person would expect without sorting again.
    pub champions: Vec<Champion>,
    /// Summoner spell id to image filename: `4` to `SummonerFlash.png`.
    pub spells: HashMap<u32, String>,
    /// Rune and rune-style id to its unversioned image path.
    pub perks: HashMap<u32, String>,
    /// Rune, rune-style and stat-shard id to its display name.
    ///
    /// Every tile draws its label before any art arrives and keeps it
    /// underneath, so this is what the page shows when an image is slow, has
    /// failed, or does not exist. Without it a tile falls back to printing a
    /// raw id, which is what stat shards did on screen for three slots.
    pub perk_names: HashMap<u32, String>,
}

/// The nine stat shards: id, art, and name.
///
/// Data Dragon serves the art — `perk-images/StatMods/…`, on the same
/// unversioned path as every other rune image — but omits the shards from
/// `runesReforged.json` entirely, which lists only the styles and the runes
/// inside their slots. So the catalogue that file produces is incomplete by
/// construction and this completes it.
///
/// Hardcoded because there is no documented endpoint that lists them, and
/// `CLAUDE.md` allows no third-party source that does. They are stable: nine
/// rows that change when Riot reworks the shard system, which is roughly
/// never and is a patch note when it happens.
const STAT_SHARDS: [(u32, &str, &str); 9] = [
    (5001, "StatModsHealthScalingIcon.png", "Health (scaling)"),
    (5002, "StatModsArmorIcon.png", "Armor"),
    (5003, "StatModsMagicResIcon.png", "Magic Resist"),
    (5005, "StatModsAttackSpeedIcon.png", "Attack Speed"),
    (5007, "StatModsCDRScalingIcon.png", "Ability Haste"),
    (5008, "StatModsAdaptiveForceIcon.png", "Adaptive Force"),
    (5010, "StatModsMovementSpeedIcon.png", "Move Speed"),
    (5011, "StatModsHealthPlusIcon.png", "Health"),
    (5013, "StatModsTenacityIcon.png", "Tenacity"),
];

/// Where the shard art sits under the unversioned rune image root.
const STAT_SHARD_DIR: &str = "perk-images/StatMods";

#[derive(Debug, thiserror::Error)]
pub enum DataDragonError {
    #[error("could not reach Data Dragon: {0}")]
    Transport(String),
    #[error("Data Dragon answered something we could not read: {0}")]
    Unexpected(String),
}

#[derive(Debug, Deserialize)]
struct ChampionFile {
    #[serde(default)]
    data: HashMap<String, ChampionEntry>,
}

#[derive(Debug, Deserialize)]
struct ChampionEntry {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct SummonerFile {
    #[serde(default)]
    data: HashMap<String, SummonerEntry>,
}

#[derive(Debug, Deserialize)]
struct SummonerEntry {
    /// A number, sent as a string. Riot has always done this here.
    #[serde(default)]
    key: String,
    #[serde(default)]
    image: SummonerImage,
}

#[derive(Debug, Default, Deserialize)]
struct SummonerImage {
    #[serde(default)]
    full: String,
}

#[derive(Debug, Deserialize)]
struct RuneStyle {
    #[serde(default)]
    id: u32,
    #[serde(default)]
    icon: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    slots: Vec<RuneSlot>,
}

#[derive(Debug, Deserialize)]
struct RuneSlot {
    #[serde(default)]
    runes: Vec<Rune>,
}

#[derive(Debug, Deserialize)]
struct Rune {
    #[serde(default)]
    id: u32,
    #[serde(default)]
    icon: String,
    #[serde(default)]
    name: String,
}

impl DataDragon {
    /// Build a catalogue from the three payloads, without fetching anything.
    ///
    /// Split out from the fetch so the shape of Riot's files is testable
    /// without the network — which matters, because a renamed field here
    /// costs the UI its icons and nothing else would notice.
    pub fn parse(
        version: String,
        champions: &str,
        summoners: &str,
        runes: &str,
    ) -> Result<DataDragon, DataDragonError> {
        let champion_file: ChampionFile = serde_json::from_str(champions)
            .map_err(|error| DataDragonError::Unexpected(format!("champion.json: {error}")))?;
        let summoner_file: SummonerFile = serde_json::from_str(summoners)
            .map_err(|error| DataDragonError::Unexpected(format!("summoner.json: {error}")))?;
        let styles: Vec<RuneStyle> = serde_json::from_str(runes)
            .map_err(|error| DataDragonError::Unexpected(format!("runesReforged.json: {error}")))?;

        let spells = summoner_file
            .data
            .into_values()
            .filter_map(|entry| {
                let id: u32 = entry.key.parse().ok()?;
                (!entry.image.full.is_empty()).then_some((id, entry.image.full))
            })
            .collect();

        // Styles and the runes inside them share one table: the UI asks for a
        // perk id without knowing or caring which of the two it is.
        let mut perks = HashMap::new();
        let mut perk_names = HashMap::new();
        for style in styles {
            if !style.icon.is_empty() {
                perks.insert(style.id, style.icon);
            }
            if !style.name.is_empty() {
                perk_names.insert(style.id, style.name);
            }
            for slot in style.slots {
                for rune in slot.runes {
                    if !rune.icon.is_empty() {
                        perks.insert(rune.id, rune.icon);
                    }
                    if !rune.name.is_empty() {
                        perk_names.insert(rune.id, rune.name);
                    }
                }
            }
        }

        // The shards are not in that file, so neither table has them yet.
        for (id, icon, name) in STAT_SHARDS {
            perks.insert(id, format!("{STAT_SHARD_DIR}/{icon}"));
            perk_names.insert(id, name.to_string());
        }

        let mut champions: Vec<Champion> = champion_file
            .data
            .into_values()
            .filter(|entry| !entry.id.is_empty() && !entry.name.is_empty())
            .map(|entry| Champion {
                key: entry.id,
                name: entry.name,
            })
            .collect();
        champions.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(DataDragon {
            version,
            champions,
            spells,
            perks,
            perk_names,
        })
    }

    pub async fn fetch(client: &reqwest::Client) -> Result<DataDragon, DataDragonError> {
        let get = |url: String| async move {
            let response = client
                .get(&url)
                .send()
                .await
                .map_err(|error| DataDragonError::Transport(error.to_string()))?;
            if !response.status().is_success() {
                return Err(DataDragonError::Transport(format!(
                    "{} for {url}",
                    response.status()
                )));
            }
            response
                .text()
                .await
                .map_err(|error| DataDragonError::Transport(error.to_string()))
        };

        let versions: Vec<String> = serde_json::from_str(&get(versions_url()).await?)
            .map_err(|error| DataDragonError::Unexpected(format!("versions.json: {error}")))?;
        let version = versions
            .into_iter()
            .next()
            .ok_or_else(|| DataDragonError::Unexpected("versions.json was empty".to_string()))?;

        let base = format!("{DDRAGON}/cdn/{version}/data/en_US");
        let (champions, summoners, runes) = tokio::join!(
            get(format!("{base}/champion.json")),
            get(format!("{base}/summoner.json")),
            get(format!("{base}/runesReforged.json"))
        );

        DataDragon::parse(version, &champions?, &summoners?, &runes?)
    }
}

/// Fetched once and kept. A few hundred short strings, held for the life of
/// the window — well inside a budget measured in megabytes, and the
/// alternative is three requests every time a build renders.
#[derive(Debug, Default)]
pub struct DataDragonState {
    catalog: OnceCell<DataDragon>,
    client: reqwest::Client,
}

impl DataDragonState {
    pub fn new() -> DataDragonState {
        DataDragonState::default()
    }

    /// The catalogue, fetching it the first time.
    ///
    /// A failure is not cached: Data Dragon being unreachable at launch is
    /// ordinary, and the next render should try again rather than leaving the
    /// window without icons until it is restarted.
    pub async fn get(&self) -> Result<&DataDragon, DataDragonError> {
        self.catalog
            .get_or_try_init(|| DataDragon::fetch(&self.client))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAMPIONS: &str = r#"{
      "type": "champion",
      "data": {
        "MonkeyKing": { "id": "MonkeyKing", "key": "62", "name": "Wukong" },
        "Ahri":       { "id": "Ahri",       "key": "103", "name": "Ahri" },
        "Nameless":   { "id": "Nameless",   "key": "1",   "name": "" }
      }
    }"#;

    const SUMMONERS: &str = r#"{
      "type": "summoner",
      "data": {
        "SummonerFlash": { "id": "SummonerFlash", "key": "4", "image": { "full": "SummonerFlash.png" } },
        "SummonerDot":   { "id": "SummonerDot",   "key": "14", "image": { "full": "SummonerDot.png" } },
        "SummonerBroken":{ "id": "SummonerBroken","key": "not-a-number", "image": { "full": "x.png" } }
      }
    }"#;

    const RUNES: &str = r#"[
      {
        "id": 8100, "key": "Domination", "name": "Domination",
        "icon": "perk-images/Styles/7200_Domination.png",
        "slots": [
          { "runes": [
              { "id": 8112, "key": "Electrocute", "name": "Electrocute",
                "icon": "perk-images/Styles/Domination/Electrocute/Electrocute.png" },
              { "id": 8124, "key": "Predator", "name": "Predator",
                "icon": "perk-images/Styles/Domination/Predator/Predator.png" }
          ] }
        ]
      },
      { "id": 8000, "key": "Precision", "name": "Precision",
        "icon": "perk-images/Styles/7201_Precision.png", "slots": [] }
    ]"#;

    #[test]
    fn spells_are_keyed_by_the_number_a_match_reports() {
        let catalog = DataDragon::parse("16.17.1".into(), CHAMPIONS, SUMMONERS, RUNES).unwrap();

        // A match says `4`; only summoner.json knows that is Flash.
        assert_eq!(catalog.spells.get(&4).map(String::as_str), Some("SummonerFlash.png"));
        assert_eq!(catalog.spells.get(&14).map(String::as_str), Some("SummonerDot.png"));
    }

    #[test]
    fn an_entry_riot_sends_in_a_shape_we_cannot_read_is_skipped_not_fatal() {
        let catalog = DataDragon::parse("16.17.1".into(), CHAMPIONS, SUMMONERS, RUNES).unwrap();
        assert_eq!(catalog.spells.len(), 2, "the unreadable entry cost us only itself");
    }

    #[test]
    fn styles_and_the_runes_inside_them_share_one_table() {
        let catalog = DataDragon::parse("16.17.1".into(), CHAMPIONS, SUMMONERS, RUNES).unwrap();

        // The UI asks for a perk id without knowing which of the two it is.
        assert_eq!(
            catalog.perks.get(&8100).map(String::as_str),
            Some("perk-images/Styles/7200_Domination.png"),
            "a style"
        );
        assert_eq!(
            catalog.perks.get(&8112).map(String::as_str),
            Some("perk-images/Styles/Domination/Electrocute/Electrocute.png"),
            "a keystone inside it"
        );
        // Four from the payload, plus the nine shards the payload omits.
        assert_eq!(catalog.perks.len(), 4 + STAT_SHARDS.len());
    }

    #[test]
    fn a_champion_carries_both_the_name_typed_and_the_key_looked_up() {
        let catalog = DataDragon::parse("16.17.1".into(), CHAMPIONS, SUMMONERS, RUNES).unwrap();

        // The whole reason the search box needs this list: nobody types
        // "MonkeyKing", and nothing else accepts "Wukong".
        let wukong = catalog
            .champions
            .iter()
            .find(|champion| champion.name == "Wukong")
            .expect("Wukong is in the list");
        assert_eq!(wukong.key, "MonkeyKing");

        // Sorted by the name a person would look for, not by the key.
        let names: Vec<&str> = catalog.champions.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Ahri", "Wukong"], "the nameless entry is dropped");
    }

    #[test]
    fn the_version_is_the_build_rather_than_the_patch() {
        let catalog = DataDragon::parse("16.17.1".into(), CHAMPIONS, SUMMONERS, RUNES).unwrap();
        // Image paths want 16.17.1; a build file is labelled 16.17. Handing
        // the patch to Data Dragon returns nothing at all.
        assert_eq!(catalog.version, "16.17.1");
    }

    #[test]
    fn a_payload_we_cannot_read_is_an_error_rather_than_an_empty_catalogue() {
        let error = DataDragon::parse("16.17.1".into(), CHAMPIONS, "not json", RUNES).unwrap_err();
        assert!(matches!(error, DataDragonError::Unexpected(_)));

        // An empty-but-valid file is not an error: Riot could legitimately
        // ship one, and the UI simply falls back to text.
        let empty = DataDragon::parse("16.17.1".into(), r#"{"data":{}}"#, r#"{"data":{}}"#, "[]").unwrap();
        assert!(empty.spells.is_empty());
        // The shards survive an empty rune file, and should: they are a
        // compiled-in constant rather than anything Riot sent us, so there is
        // nothing for an empty payload to have taken away.
        assert_eq!(empty.perks.len(), STAT_SHARDS.len());
        assert!(empty.perks.keys().all(|id| (5001..=5013).contains(id)));
    }

    /// The shards are the reason this table is completed by hand.
    ///
    /// `runesReforged.json` lists styles and the runes in their slots and
    /// stops there, so before this the three shard tiles on a rune page had
    /// neither art nor a name and drew as "#5005" — three boxes that read as
    /// empty slots. Riot serves the art; only the index is missing.
    #[test]
    fn stat_shards_carry_art_and_a_name_although_riot_omits_them() {
        let catalog = DataDragon::parse("16.17.1".into(), CHAMPIONS, SUMMONERS, RUNES).unwrap();

        assert_eq!(
            catalog.perks.get(&5008).map(String::as_str),
            Some("perk-images/StatMods/StatModsAdaptiveForceIcon.png"),
            "the unversioned rune path, same as every other perk image"
        );
        assert_eq!(
            catalog.perk_names.get(&5008).map(String::as_str),
            Some("Adaptive Force")
        );

        // Every shard the crawled data actually contains.
        for id in [5001, 5005, 5007, 5008, 5010, 5011, 5013] {
            assert!(catalog.perks.contains_key(&id), "{id} has no art");
            assert!(catalog.perk_names.contains_key(&id), "{id} has no name");
        }
    }

    /// Runes carry a name too, so a tile whose art is slow, blocked or
    /// missing says what it is rather than printing an id.
    #[test]
    fn runes_and_styles_are_named_not_just_illustrated() {
        let catalog = DataDragon::parse("16.17.1".into(), CHAMPIONS, SUMMONERS, RUNES).unwrap();

        assert_eq!(catalog.perk_names.get(&8100).map(String::as_str), Some("Domination"));
        assert_eq!(catalog.perk_names.get(&8112).map(String::as_str), Some("Electrocute"));
        assert_eq!(catalog.perk_names.get(&8000).map(String::as_str), Some("Precision"));
    }
}
