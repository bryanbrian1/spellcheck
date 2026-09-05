//! Where icon art lives, so the UI can show a picture instead of nine letters.
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
//! So two of the four need a lookup table, and this fetches them once.
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

/// What the UI needs to build an icon URL for anything.
///
/// Items and champions are absent on purpose: their URLs are derivable from
/// the id and the key the UI already holds, so shipping a table of them would
/// be several hundred kilobytes to say what a format string says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconCatalog {
    /// The full Data Dragon version, e.g. `16.17.1` — not the `16.17` patch
    /// a build is labelled with. Image paths want the build.
    pub version: String,
    /// Summoner spell id to image filename: `4` to `SummonerFlash.png`.
    pub spells: HashMap<u32, String>,
    /// Rune and rune-style id to its unversioned image path.
    pub perks: HashMap<u32, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum IconError {
    #[error("could not reach Data Dragon: {0}")]
    Transport(String),
    #[error("Data Dragon answered something we could not read: {0}")]
    Unexpected(String),
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
}

impl IconCatalog {
    /// Build a catalogue from the three payloads, without fetching anything.
    ///
    /// Split out from the fetch so the shape of Riot's files is testable
    /// without the network — which matters, because a renamed field here
    /// costs the UI its icons and nothing else would notice.
    pub fn parse(
        version: String,
        summoners: &str,
        runes: &str,
    ) -> Result<IconCatalog, IconError> {
        let summoner_file: SummonerFile = serde_json::from_str(summoners)
            .map_err(|error| IconError::Unexpected(format!("summoner.json: {error}")))?;
        let styles: Vec<RuneStyle> = serde_json::from_str(runes)
            .map_err(|error| IconError::Unexpected(format!("runesReforged.json: {error}")))?;

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
        for style in styles {
            if !style.icon.is_empty() {
                perks.insert(style.id, style.icon);
            }
            for slot in style.slots {
                for rune in slot.runes {
                    if !rune.icon.is_empty() {
                        perks.insert(rune.id, rune.icon);
                    }
                }
            }
        }

        Ok(IconCatalog {
            version,
            spells,
            perks,
        })
    }

    pub async fn fetch(client: &reqwest::Client) -> Result<IconCatalog, IconError> {
        let get = |url: String| async move {
            let response = client
                .get(&url)
                .send()
                .await
                .map_err(|error| IconError::Transport(error.to_string()))?;
            if !response.status().is_success() {
                return Err(IconError::Transport(format!(
                    "{} for {url}",
                    response.status()
                )));
            }
            response
                .text()
                .await
                .map_err(|error| IconError::Transport(error.to_string()))
        };

        let versions: Vec<String> = serde_json::from_str(&get(versions_url()).await?)
            .map_err(|error| IconError::Unexpected(format!("versions.json: {error}")))?;
        let version = versions
            .into_iter()
            .next()
            .ok_or_else(|| IconError::Unexpected("versions.json was empty".to_string()))?;

        let base = format!("{DDRAGON}/cdn/{version}/data/en_US");
        let (summoners, runes) = tokio::join!(
            get(format!("{base}/summoner.json")),
            get(format!("{base}/runesReforged.json"))
        );

        IconCatalog::parse(version, &summoners?, &runes?)
    }
}

/// Fetched once and kept. A few hundred short strings, held for the life of
/// the window — well inside a budget measured in megabytes, and the
/// alternative is three requests every time a build renders.
#[derive(Debug, Default)]
pub struct IconState {
    catalog: OnceCell<IconCatalog>,
    client: reqwest::Client,
}

impl IconState {
    pub fn new() -> IconState {
        IconState::default()
    }

    /// The catalogue, fetching it the first time.
    ///
    /// A failure is not cached: Data Dragon being unreachable at launch is
    /// ordinary, and the next render should try again rather than leaving the
    /// window without icons until it is restarted.
    pub async fn get(&self) -> Result<&IconCatalog, IconError> {
        self.catalog
            .get_or_try_init(|| IconCatalog::fetch(&self.client))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        "id": 8100, "key": "Domination", "icon": "perk-images/Styles/7200_Domination.png",
        "slots": [
          { "runes": [
              { "id": 8112, "key": "Electrocute", "icon": "perk-images/Styles/Domination/Electrocute/Electrocute.png" },
              { "id": 8124, "key": "Predator", "icon": "perk-images/Styles/Domination/Predator/Predator.png" }
          ] }
        ]
      },
      { "id": 8000, "key": "Precision", "icon": "perk-images/Styles/7201_Precision.png", "slots": [] }
    ]"#;

    #[test]
    fn spells_are_keyed_by_the_number_a_match_reports() {
        let catalog = IconCatalog::parse("16.17.1".into(), SUMMONERS, RUNES).unwrap();

        // A match says `4`; only summoner.json knows that is Flash.
        assert_eq!(catalog.spells.get(&4).map(String::as_str), Some("SummonerFlash.png"));
        assert_eq!(catalog.spells.get(&14).map(String::as_str), Some("SummonerDot.png"));
    }

    #[test]
    fn an_entry_riot_sends_in_a_shape_we_cannot_read_is_skipped_not_fatal() {
        let catalog = IconCatalog::parse("16.17.1".into(), SUMMONERS, RUNES).unwrap();
        assert_eq!(catalog.spells.len(), 2, "the unreadable entry cost us only itself");
    }

    #[test]
    fn styles_and_the_runes_inside_them_share_one_table() {
        let catalog = IconCatalog::parse("16.17.1".into(), SUMMONERS, RUNES).unwrap();

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
        assert_eq!(catalog.perks.len(), 4);
    }

    #[test]
    fn the_version_is_the_build_rather_than_the_patch() {
        let catalog = IconCatalog::parse("16.17.1".into(), SUMMONERS, RUNES).unwrap();
        // Image paths want 16.17.1; a build file is labelled 16.17. Handing
        // the patch to Data Dragon returns nothing at all.
        assert_eq!(catalog.version, "16.17.1");
    }

    #[test]
    fn a_payload_we_cannot_read_is_an_error_rather_than_an_empty_catalogue() {
        let error = IconCatalog::parse("16.17.1".into(), "not json", RUNES).unwrap_err();
        assert!(matches!(error, IconError::Unexpected(_)));

        // An empty-but-valid file is not an error: Riot could legitimately
        // ship one, and the UI simply falls back to text.
        let empty = IconCatalog::parse("16.17.1".into(), r#"{"data":{}}"#, "[]").unwrap();
        assert!(empty.spells.is_empty() && empty.perks.is_empty());
    }
}
