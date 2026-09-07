//! Check three: how this game is actually going, and what that changes.
//!
//! The first two checks reason about a champ select — five names against five
//! names, nothing yet bought. This one has the game itself: everyone's items,
//! everyone's level, everyone's score. It answers a different question.
//! Against your lane opponent, are you behind, even, or ahead, and what
//! should that change about the next thing you buy?
//!
//! **The standing is a measurement, and the advice is a rule.** "Syndra is
//! four thousand gold up on you" is a fact about this game, read off the
//! live API, and it carries its number. "So buy something you can finish"
//! is reasoning, and like every other suggestion here it argues in words.
//! The two travel separately for that reason: [`Standing`] is not a
//! [`Suggestion`] and is not rendered on the amber rail.
//!
//! Like check two, this one is narrower than it looks, and for the same
//! reason. Being *behind* has an answer in the item vocabulary — cheap
//! defensive components you can actually finish. Being *ahead* does not:
//! the answer there is a snowball item or a damage spike, and `answers` in
//! `data/meta/items.json` is a closed defensive list. So the check reports
//! being ahead and suggests nothing, rather than inventing an item to name.

use serde::{Deserialize, Serialize};

use crate::live::{GameSnapshot, Player};

use super::tags::{Answer, DamageType, Tags};
use super::{pick, Priority, Suggestion};

/// The gold gap at which the game stops being even.
///
/// Roughly a completed component. Below it, the difference is a wave of
/// minions and a recall, and telling someone they are behind over that would
/// make the check cry wolf every game.
const MEANINGFUL_GOLD: i64 = 1_000;

/// The gap at which "behind" becomes "behind by an item", which is the point
/// where the advice changes from a nudge to buying differently.
const AN_ITEM: i64 = 2_500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Footing {
    Behind,
    Even,
    Ahead,
}

/// Where you stand against the one player you can be fairly compared to.
///
/// This is measured, not reasoned, so it carries its numbers. The UI renders
/// it in its own style rather than on either rail: it is neither a statistic
/// drawn from thousands of games nor an argument in words.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Standing {
    pub footing: Footing,
    /// The opponent's champion, by display name.
    pub opponent: String,
    /// Your spent gold minus theirs. Negative means they are ahead.
    ///
    /// Spent rather than total, because the live API does not show us an
    /// enemy's gold in hand — only what they have already turned into items.
    /// It is the only number that can be compared honestly.
    pub gold_delta: i64,
    pub level_delta: i32,
    /// What you are holding right now, which the API does give us for
    /// ourselves. This is what makes "finishable this trip" answerable.
    pub gold_in_hand: u32,
}

/// What an inventory is worth, from the committed item table.
///
/// `None` when the table has never heard of something they are carrying. That
/// is the same rule an untagged champion follows: a missing entry is a hole,
/// not a zero. Pricing an unknown item at nothing would quietly understate
/// whoever is holding it, which is precisely the failure this whole function
/// exists to correct — so the honest answer is to decline to total it.
///
/// The table ships with the binary and covers the whole Data Dragon
/// catalogue, so in practice this is `Some` for every item on the patch the
/// build was cut for, and `None` only once the game has moved ahead of it.
fn inventory_gold(tags: &Tags, player: &Player) -> Option<u32> {
    let mut total: u32 = 0;
    for held in &player.items {
        let item = tags.item(held.item_id)?;
        total = total.saturating_add(item.cost.saturating_mul(held.count));
    }
    Some(total)
}

/// Compare the local player to whoever is standing in their lane.
///
/// `None` whenever the comparison would be dishonest: we are spectating, the
/// mode assigns no lanes, nobody is opposite us, or either inventory holds an
/// item this build cannot price. There is no fallback to "the enemy mid laner
/// probably" — a confident number about the wrong player is worse than no
/// number, and so is a confident number about the wrong gold.
pub fn standing(tags: &Tags, snapshot: &GameSnapshot) -> Option<Standing> {
    let us = snapshot.local_player()?;
    let them = snapshot.lane_opponent()?;

    // Either side unpriceable means there is no honest comparison to draw.
    let ours = inventory_gold(tags, us)?;
    let theirs = inventory_gold(tags, them)?;

    let gold_delta = i64::from(ours) - i64::from(theirs);
    let footing = if gold_delta <= -MEANINGFUL_GOLD {
        Footing::Behind
    } else if gold_delta >= MEANINGFUL_GOLD {
        Footing::Ahead
    } else {
        Footing::Even
    };

    Some(Standing {
        footing,
        opponent: them.champion_name.clone(),
        gold_delta,
        level_delta: i32::try_from(us.level).unwrap_or(0) - i32::try_from(them.level).unwrap_or(0),
        gold_in_hand: snapshot.current_gold,
    })
}

/// What the state of the game changes about what to buy.
///
/// Empty is the common and correct answer: an even game changes nothing, and
/// a winning one has no advice this vocabulary can express.
pub fn game_state(tags: &Tags, snapshot: &GameSnapshot) -> Vec<Suggestion> {
    let Some(standing) = standing(tags, snapshot) else {
        return Vec::new();
    };
    if standing.footing != Footing::Behind {
        return Vec::new();
    }

    let Some(us) = snapshot
        .local_player()
        .and_then(|player| tags.champion_by_name(&player.champion_name))
    else {
        return Vec::new();
    };

    // Which resist depends on what is actually killing you, which in a lane
    // is one specific champion rather than a team average.
    let opponent = snapshot
        .lane_opponent()
        .and_then(|player| tags.champion_by_name(&player.champion_name));
    let answer = match opponent.map(|tags| tags.damage_type) {
        Some(DamageType::Ad) => Answer::Armor,
        Some(DamageType::Ap) => Answer::Mr,
        // Mixed, true, or a champion we have no tags for. There is no resist
        // that answers "some of both", and guessing one would spend a slot on
        // half the problem.
        _ => return Vec::new(),
    };

    let by_an_item = standing.gold_delta <= -AN_ITEM;
    let reason = if by_an_item {
        format!(
            "{} is up on you by about an item. Buy the component you can \
             finish on this trip rather than holding gold for a spike you \
             cannot afford yet — the cheap resist is what wins you the lane \
             back, and it builds into the same thing later anyway.",
            standing.opponent
        )
    } else {
        format!(
            "{} is ahead of you. A cheap resist now costs less than the trade \
             you lose without it.",
            standing.opponent
        )
    };

    // Only the component. "Components over spikes" is the whole substance of
    // the behind advice, so naming the finished item alongside it would be
    // suggesting the exact thing the rule says not to save for.
    pick(tags, answer, us.damage_type)
        .into_iter()
        .filter(|(_, item)| item.is_component)
        .map(|(id, _)| Suggestion::rule(id, Priority::Rush, reason.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recommend::SuggestionSource;
    use serde_json::json;

    fn tags() -> &'static Tags {
        Tags::get()
    }

    /// A game where we are `us` at `our_gold`, against `them` at `their_gold`.
    ///
    /// Gold is expressed as a stack of Health Potions, which the committed
    /// table prices at 50 each. Reaching a total through a *real* id is the
    /// point: a fixture that invents its own price is what let the old
    /// arithmetic agree with itself while disagreeing with the game.
    fn game(us: &str, our_gold: u32, our_level: u32, them: &str, their_gold: u32, their_level: u32) -> GameSnapshot {
        const HEALTH_POTION: u32 = 2003;
        const POTION_COST: u32 = 50;
        let player = |name: &str, team: &str, gold: u32, level: u32| {
            assert_eq!(gold % POTION_COST, 0, "test gold must divide by {POTION_COST}");
            json!({
                "championName": name, "team": team, "position": "MIDDLE",
                "riotId": format!("{name}#EUW"), "level": level, "isDead": false,
                "items": [{ "itemID": HEALTH_POTION, "count": gold / POTION_COST }],
                "scores": {},
            })
        };
        GameSnapshot::from_json(&json!({
            "activePlayer": { "riotId": format!("{us}#EUW"), "currentGold": 1340.0 },
            "allPlayers": [
                player(us, "ORDER", our_gold, our_level),
                player(them, "CHAOS", their_gold, their_level),
            ],
            "gameData": { "gameTime": 900.0, "gameMode": "CLASSIC" },
        }))
        .unwrap()
    }

    /// A real `allgamedata` body, captured from a live game and scrubbed of
    /// player names. Nothing else in this crate is built from a payload the
    /// author did not invent.
    const REAL_GAME: &str = include_str!("../live/testdata/allgamedata.json");

    /// The regression that the whole item table exists for.
    ///
    /// This one fixture would have failed the old code, and no fixture written
    /// by hand ever could have: the bug was not in the arithmetic but in what
    /// the API's `price` field *means*, so any payload invented alongside the
    /// parser agreed with it by construction.
    ///
    /// The exact totals move whenever Riot reprices one of the seven items in
    /// this capture. That is fine and the failure is informative — regenerate
    /// the table, then update the numbers here.
    #[test]
    fn a_real_payload_is_priced_from_the_table_and_not_from_its_own_price_field() {
        let body: serde_json::Value = serde_json::from_str(REAL_GAME).expect("captured payload");
        let snapshot = GameSnapshot::from_json(&body).expect("a real game parses");

        let found = standing(tags(), &snapshot).expect("a mid lane with an opponent");
        assert_eq!(found.opponent, "Vel'Koz");
        assert_eq!(found.gold_delta, 1_800, "priced from gold.total");
        assert_eq!(found.footing, Footing::Ahead);

        // And now the part that makes this a regression test rather than a
        // snapshot: the same payload, summed the way the app used to sum it,
        // produces a materially different answer. If this ever stops being
        // true the fixture has lost the property it was captured for.
        let naive: i64 = {
            let sum_for = |riot_id: &str| -> i64 {
                body["allPlayers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|p| p["riotId"] == riot_id)
                    .unwrap()["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| {
                        i["price"].as_i64().unwrap_or(0)
                            * i["count"].as_i64().unwrap_or(1).max(1)
                    })
                    .sum()
            };
            sum_for("Player3#TEST") - sum_for("Player8#TEST")
        };
        assert_eq!(naive, 800, "the old arithmetic, preserved for contrast");
        assert!(
            naive.abs() < found.gold_delta.abs(),
            "the combine-cost sum understates the real gap"
        );
    }

    #[test]
    fn a_gold_gap_is_measured_against_the_player_in_your_lane() {
        let snapshot = game("Ahri", 3000, 11, "Syndra", 7000, 13);
        let found = standing(tags(), &snapshot).expect("Syndra is mid");

        assert_eq!(found.footing, Footing::Behind);
        assert_eq!(found.opponent, "Syndra");
        assert_eq!(found.gold_delta, -4000);
        assert_eq!(found.level_delta, -2);
        assert_eq!(found.gold_in_hand, 1340);
    }

    #[test]
    fn a_small_gap_is_an_even_game_rather_than_a_losing_one() {
        let snapshot = game("Ahri", 3000, 11, "Syndra", 3400, 11);
        let found = standing(tags(), &snapshot).unwrap();
        assert_eq!(
            found.footing,
            Footing::Even,
            "a wave of minions was reported as being behind"
        );
        assert!(game_state(tags(), &snapshot).is_empty());
    }

    #[test]
    fn being_ahead_is_reported_but_suggests_nothing() {
        let snapshot = game("Ahri", 8000, 14, "Syndra", 3000, 11);
        let found = standing(tags(), &snapshot).unwrap();
        assert_eq!(found.footing, Footing::Ahead);
        assert!(found.gold_delta > 0);

        // Snowball items and damage spikes are the real answer here, and
        // neither is in the closed defensive answer vocabulary. Saying
        // nothing beats naming an item for the sake of it.
        assert!(game_state(tags(), &snapshot).is_empty());
    }

    #[test]
    fn behind_against_a_mage_asks_for_magic_resist_and_only_the_component() {
        let snapshot = game("Ahri", 2000, 10, "Syndra", 7000, 13);
        let found = game_state(tags(), &snapshot);

        assert!(!found.is_empty(), "four thousand gold down and no advice");
        for suggestion in &found {
            let item = tags().item(suggestion.item_id).unwrap();
            assert!(item.answers.contains(&Answer::Mr), "{} is not a resist", item.name);
            assert!(
                item.is_component,
                "{} is a finished item, and the advice is components over spikes",
                item.name
            );
            assert_eq!(suggestion.priority, Priority::Rush);
        }
    }

    #[test]
    fn behind_against_a_bruiser_asks_for_armour() {
        // Darius is physical damage.
        let snapshot = game("Ahri", 2000, 10, "Darius", 7000, 13);
        let found = game_state(tags(), &snapshot);

        assert!(!found.is_empty());
        for suggestion in &found {
            let item = tags().item(suggestion.item_id).unwrap();
            assert!(item.answers.contains(&Answer::Armor), "{}", item.name);
        }
    }

    #[test]
    fn a_mixed_damage_opponent_gets_no_resist_named() {
        // Kaisa deals both. There is no resist that answers half a problem,
        // and picking one would spend the slot badly.
        let snapshot = game("Ahri", 2000, 10, "Kaisa", 7000, 13);
        assert!(game_state(tags(), &snapshot).is_empty());
    }

    #[test]
    fn an_untagged_opponent_produces_no_advice_rather_than_a_guess() {
        let snapshot = game("Ahri", 2000, 10, "NotAChampion", 7000, 13);
        // The standing is still honest: gold is gold whoever is holding it.
        assert_eq!(standing(tags(), &snapshot).unwrap().footing, Footing::Behind);
        assert!(game_state(tags(), &snapshot).is_empty());
    }

    #[test]
    fn a_mode_with_no_lanes_has_no_standing_at_all() {
        let aram = GameSnapshot::from_json(&json!({
            "activePlayer": { "riotId": "Ahri#EUW" },
            "allPlayers": [
                { "championName": "Ahri", "team": "ORDER", "position": "", "riotId": "Ahri#EUW", "items": [], "scores": {} },
                { "championName": "Syndra", "team": "CHAOS", "position": "", "riotId": "Syndra#EUW", "items": [], "scores": {} },
            ],
            "gameData": { "gameMode": "ARAM" },
        }))
        .unwrap();

        assert!(standing(tags(), &aram).is_none());
        assert!(game_state(tags(), &aram).is_empty());
    }

    #[test]
    fn spectating_produces_nothing() {
        let spectated = GameSnapshot::from_json(&json!({
            "allPlayers": [{ "championName": "Ahri", "team": "ORDER", "position": "MIDDLE", "items": [], "scores": {} }],
            "gameData": {},
        }))
        .unwrap();

        assert!(standing(tags(), &spectated).is_none());
        assert!(game_state(tags(), &spectated).is_empty());
    }

    #[test]
    fn every_suggestion_is_a_rule_and_carries_no_number() {
        // The standing carries the gold gap; the sentence beside it may not.
        for gap in [3_000, 6_000, 12_000] {
            let snapshot = game("Ahri", 1000, 9, "Syndra", 1000 + gap, 13);
            for suggestion in game_state(tags(), &snapshot) {
                assert_eq!(suggestion.source, SuggestionSource::Rule);
                assert!(
                    !suggestion.reason.chars().any(|c| c.is_ascii_digit()),
                    "reason carries a number: {}",
                    suggestion.reason
                );
            }
        }
    }
}
