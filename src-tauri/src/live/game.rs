//! The `allgamedata` payload, reduced to what the third check reads.
//!
//! The live payload is far larger than anything else this app parses — every
//! player's full rune page, every ability, the whole event log. Nothing here
//! demands more than it needs, and every field is defaulted, so a patch that
//! renames something we do not read cannot break us.
//!
//! Two shapes matter: who we are, and who is standing in our lane.

use serde::{Deserialize, Serialize};

use crate::build_data::Role;

/// Which side of the map. `ORDER` is blue, `CHAOS` is red; the app never
/// cares which is which, only whether two players share one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Team {
    Order,
    Chaos,
    /// Anything else the game starts sending. Treated as its own side, so an
    /// unknown value can never accidentally match ours.
    #[serde(other)]
    Unknown,
}

/// One inventory entry.
///
/// **`price` is deliberately not read**, and that is the whole lesson of this
/// struct. The live API's `price` is Data Dragon's `gold.base` — the *combine*
/// cost — not `gold.total`. Summing it counts only the last purchase in each
/// item's build path and silently discards every component already consumed
/// into it: a finished Rabadon's Deathcap reports 1100 against a real 3500,
/// and a finished pair of boots reports zero.
///
/// It reads correctly for a base component, because there combine cost *is*
/// the total, which is exactly why the mistake survived a hundred tests and a
/// hand-driven stand-in client. What a player is holding is worth what the
/// committed item table says it is worth; see [`crate::recommend::state`].
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawItem {
    /// `itemID`, with both letters capitalised, and it is the only field in
    /// this payload shaped that way — `camelCase` renders it `itemId` and the
    /// value silently defaults to zero. That went unnoticed for as long as
    /// nothing read it; the moment gold started coming from the item table
    /// instead of from `price`, every item became unpriceable at once.
    #[serde(rename = "itemID")]
    item_id: u32,
    count: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawScores {
    kills: u32,
    deaths: u32,
    assists: u32,
    creep_score: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawPlayer {
    champion_name: String,
    team: Team,
    /// `TOP`, `JUNGLE`, `MIDDLE`, `BOTTOM`, `UTILITY`, or empty in the modes
    /// that assign no lane. Uppercase here and lowercase in champ select,
    /// which `Role::parse` already absorbs.
    position: String,
    level: u32,
    is_dead: bool,
    respawn_timer: f64,
    items: Vec<RawItem>,
    scores: RawScores,
    riot_id: String,
    summoner_name: String,
}

impl Default for RawPlayer {
    fn default() -> Self {
        RawPlayer {
            champion_name: String::new(),
            team: Team::Unknown,
            position: String::new(),
            level: 0,
            is_dead: false,
            respawn_timer: 0.0,
            items: Vec::new(),
            scores: RawScores::default(),
            riot_id: String::new(),
            summoner_name: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawActivePlayer {
    current_gold: f64,
    level: u32,
    riot_id: String,
    summoner_name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawGameData {
    game_time: f64,
    game_mode: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawAllGameData {
    active_player: Option<RawActivePlayer>,
    all_players: Vec<RawPlayer>,
    game_data: RawGameData,
}

/// One thing in a player's inventory, and how many of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryItem {
    pub item_id: u32,
    /// Stack size. Consumables stack; everything else is one.
    pub count: u32,
}

/// One player, as the third check reads them.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    /// Display name — `Ahri`, `Wukong` — not the Data Dragon key. The live
    /// API has no notion of the key, so tags are looked up by name here.
    pub champion_name: String,
    pub team: Team,
    pub position: Option<Role>,
    pub level: u32,
    pub is_dead: bool,
    /// Seconds until respawn. Zero when alive.
    pub respawn_timer: f64,
    /// What they are carrying, as ids and stack sizes.
    ///
    /// Deliberately not a gold total. Pricing an inventory needs the committed
    /// item table, which lives a layer up in [`crate::recommend`], and this
    /// layer may not reach for it. Handing out ids keeps the one place that
    /// knows what an item costs the only place that can say.
    pub items: Vec<InventoryItem>,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub creep_score: u32,
}

impl Player {
    fn from_raw(raw: &RawPlayer) -> Player {
        Player {
            champion_name: raw.champion_name.clone(),
            team: raw.team,
            position: Role::parse_optional(&raw.position).and_then(Result::ok),
            level: raw.level,
            is_dead: raw.is_dead,
            respawn_timer: raw.respawn_timer,
            items: raw
                .items
                .iter()
                .map(|item| InventoryItem {
                    item_id: item.item_id,
                    // A stack the API reports as zero is still one thing held.
                    count: item.count.max(1),
                })
                .collect(),
            kills: raw.scores.kills,
            deaths: raw.scores.deaths,
            assists: raw.scores.assists,
            creep_score: raw.scores.creep_score,
        }
    }
}

/// One reading of a game in progress.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSnapshot {
    /// Seconds since the game started.
    pub game_time: f64,
    pub game_mode: String,
    /// Gold in hand, which the live API gives us only for ourselves.
    pub current_gold: u32,
    pub players: Vec<Player>,
    /// Index into `players`. `None` while spectating, or before the game has
    /// told us who we are.
    pub local: Option<usize>,
}

impl GameSnapshot {
    /// Parse an `allgamedata` body.
    ///
    /// Everything is defaulted, so this fails only when the body is not JSON
    /// of the right general shape — not when a field we never read has moved.
    pub fn from_json(value: &serde_json::Value) -> Result<GameSnapshot, super::LiveError> {
        let raw: RawAllGameData = serde_json::from_value(value.clone())
            .map_err(|error| super::LiveError::unexpected("the live game data", error))?;

        let players: Vec<Player> = raw.all_players.iter().map(Player::from_raw).collect();

        // The API identifies us by name rather than by index, and has used two
        // different names for it across patches. Try the Riot id first and
        // fall back to the old summoner name; matching on neither is a state
        // (we are spectating) rather than a failure.
        let local = raw.active_player.as_ref().and_then(|active| {
            let matches = |raw_player: &RawPlayer| {
                (!active.riot_id.is_empty() && raw_player.riot_id == active.riot_id)
                    || (!active.summoner_name.is_empty()
                        && raw_player.summoner_name == active.summoner_name)
            };
            raw.all_players.iter().position(matches)
        });

        Ok(GameSnapshot {
            game_time: raw.game_data.game_time,
            game_mode: raw.game_data.game_mode,
            current_gold: raw
                .active_player
                .as_ref()
                .map(|active| active.current_gold.max(0.0) as u32)
                .unwrap_or(0),
            players,
            local,
        })
    }

    pub fn local_player(&self) -> Option<&Player> {
        self.players.get(self.local?)
    }

    /// The enemy standing in our lane.
    ///
    /// `None` whenever the comparison would be meaningless: no lane assigned
    /// (ARAM and the rotating modes), or nobody opposite us. Guessing an
    /// opponent would produce a confident number about the wrong player.
    pub fn lane_opponent(&self) -> Option<&Player> {
        let us = self.local_player()?;
        let position = us.position?;
        self.players
            .iter()
            .find(|player| player.team != us.team && player.position == Some(position))
    }

    /// Everyone on the other side, whether or not they hold a lane.
    pub fn enemies(&self) -> Vec<&Player> {
        let Some(us) = self.local_player() else {
            return Vec::new();
        };
        self.players
            .iter()
            .filter(|player| player.team != us.team)
            .collect()
    }

    pub fn allies(&self) -> Vec<&Player> {
        let Some(us) = self.local_player() else {
            return Vec::new();
        };
        self.players
            .iter()
            .filter(|player| player.team == us.team)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn player(name: &str, team: &str, position: &str, level: u32, items: &[(u32, u32)]) -> serde_json::Value {
        json!({
            "championName": name,
            "team": team,
            "position": position,
            "level": level,
            "isDead": false,
            "respawnTimer": 0.0,
            "riotId": format!("{name}#EUW"),
            "summonerName": name,
            "items": items
                .iter()
                // No `price`. The real payload carries one and it is a
                // combine cost; a fixture that invents a price is how the old
                // arithmetic stayed green while being wrong. See RawItem.
                .map(|(id, count)| json!({ "itemID": id, "count": count }))
                .collect::<Vec<_>>(),
            "scores": { "kills": 1, "deaths": 2, "assists": 3, "creepScore": 100 },
        })
    }

    fn game() -> serde_json::Value {
        json!({
            "activePlayer": { "currentGold": 1340.7, "level": 11, "riotId": "Ahri#EUW", "summonerName": "Ahri" },
            "allPlayers": [
                player("Ahri", "ORDER", "MIDDLE", 11, &[(3020, 1), (3165, 1)]),
                player("Leona", "ORDER", "UTILITY", 9, &[(3190, 1)]),
                player("Syndra", "CHAOS", "MIDDLE", 12, &[(3020, 1), (3165, 1), (3157, 1)]),
                player("Thresh", "CHAOS", "UTILITY", 10, &[]),
            ],
            "gameData": { "gameTime": 1104.5, "gameMode": "CLASSIC", "mapName": "Map11" },
            "events": { "Events": [{ "EventID": 0, "EventName": "GameStart" }] },
        })
    }

    #[test]
    fn reads_a_game_down_to_who_is_in_our_lane() {
        let snapshot = GameSnapshot::from_json(&game()).unwrap();

        assert_eq!(snapshot.game_time, 1104.5);
        assert_eq!(snapshot.current_gold, 1340, "gold is truncated, not rounded");

        let us = snapshot.local_player().expect("we are in the game");
        assert_eq!(us.champion_name, "Ahri");
        assert_eq!(us.position, Some(Role::Middle));
        assert_eq!(
            us.items,
            vec![
                InventoryItem { item_id: 3020, count: 1 },
                InventoryItem { item_id: 3165, count: 1 },
            ],
            "ids and stacks come through; pricing them is the tag layer's job"
        );

        let opponent = snapshot.lane_opponent().expect("Syndra is mid");
        assert_eq!(opponent.champion_name, "Syndra");
        assert_eq!(opponent.items.len(), 3);
        assert_eq!(opponent.level, 12);
    }

    #[test]
    fn the_uppercase_positions_the_live_api_uses_parse() {
        let snapshot = GameSnapshot::from_json(&game()).unwrap();
        let positions: Vec<_> = snapshot.players.iter().map(|p| p.position).collect();
        assert_eq!(
            positions,
            vec![
                Some(Role::Middle),
                Some(Role::Utility),
                Some(Role::Middle),
                Some(Role::Utility)
            ]
        );
    }

    #[test]
    fn sides_are_split_by_our_own_team_rather_than_by_colour() {
        let snapshot = GameSnapshot::from_json(&game()).unwrap();
        let enemy_names: Vec<_> = snapshot.enemies().iter().map(|p| p.champion_name.as_str()).collect();
        assert_eq!(enemy_names, vec!["Syndra", "Thresh"]);
        let ally_names: Vec<_> = snapshot.allies().iter().map(|p| p.champion_name.as_str()).collect();
        assert_eq!(ally_names, vec!["Ahri", "Leona"]);
    }

    #[test]
    fn a_mode_with_no_lanes_has_no_lane_opponent() {
        // ARAM: everyone is mid, and nobody has a position.
        let aram = json!({
            "activePlayer": { "riotId": "Ahri#EUW" },
            "allPlayers": [
                player("Ahri", "ORDER", "", 11, &[]),
                player("Syndra", "CHAOS", "", 12, &[]),
            ],
            "gameData": { "gameMode": "ARAM" },
        });

        let snapshot = GameSnapshot::from_json(&aram).unwrap();
        assert!(snapshot.local_player().is_some());
        assert!(
            snapshot.lane_opponent().is_none(),
            "an opponent was guessed for a mode that assigns no lanes"
        );
        assert_eq!(snapshot.enemies().len(), 1, "sides still work without lanes");
    }

    #[test]
    fn spectating_identifies_nobody_rather_than_the_first_player() {
        let spectated = json!({
            "allPlayers": [player("Ahri", "ORDER", "MIDDLE", 11, &[])],
            "gameData": { "gameTime": 60.0 },
        });

        let snapshot = GameSnapshot::from_json(&spectated).unwrap();
        assert!(snapshot.local_player().is_none());
        assert!(snapshot.lane_opponent().is_none());
        assert!(snapshot.enemies().is_empty());
    }

    #[test]
    fn an_old_client_that_only_sends_a_summoner_name_still_finds_us() {
        let old = json!({
            "activePlayer": { "summonerName": "Ahri", "currentGold": 10.0 },
            "allPlayers": [{
                "championName": "Ahri", "team": "ORDER", "position": "MIDDLE",
                "summonerName": "Ahri", "items": [], "scores": {},
            }],
            "gameData": {},
        });

        let snapshot = GameSnapshot::from_json(&old).unwrap();
        assert_eq!(snapshot.local_player().map(|p| p.champion_name.as_str()), Some("Ahri"));
    }

    #[test]
    fn an_unknown_side_never_counts_as_ours() {
        let odd = json!({
            "activePlayer": { "riotId": "Ahri#EUW" },
            "allPlayers": [
                player("Ahri", "ORDER", "MIDDLE", 11, &[]),
                player("Something", "A_NEW_TEAM", "MIDDLE", 11, &[]),
            ],
            "gameData": {},
        });

        let snapshot = GameSnapshot::from_json(&odd).unwrap();
        assert_eq!(snapshot.allies().len(), 1, "an unreadable side joined ours");
        assert_eq!(snapshot.enemies().len(), 1);
    }

    #[test]
    fn a_stacked_item_counts_once_per_copy() {
        let stacked = json!({
            "activePlayer": { "riotId": "Ahri#EUW" },
            "allPlayers": [{
                "championName": "Ahri", "team": "ORDER", "position": "MIDDLE",
                "riotId": "Ahri#EUW",
                "items": [{ "itemID": 2003, "count": 4 }],
                "scores": {},
            }],
            "gameData": {},
        });

        let snapshot = GameSnapshot::from_json(&stacked).unwrap();
        assert_eq!(
            snapshot.local_player().unwrap().items,
            vec![InventoryItem { item_id: 2003, count: 4 }],
            "a stack keeps its size, so the tag layer can price it four times"
        );
    }

    #[test]
    fn an_empty_body_is_read_without_complaint() {
        let snapshot = GameSnapshot::from_json(&json!({})).unwrap();
        assert!(snapshot.players.is_empty());
        assert!(snapshot.local_player().is_none());
    }
}
