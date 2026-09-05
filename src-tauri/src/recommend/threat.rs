//! Check one: what the enemy composition forces you to build.
//!
//! Reads the enemy team's tags and answers them — the armour/magic-resist
//! split, antiheal and when it has to be online, tenacity. Dive does not add
//! an item of its own; it makes the resist urgent, which is the same advice a
//! player gives out loud ("they have two divers, get your armour early").
//!
//! Every rule here is gated on seeing enough of the enemy team to mean it.
//! Blind pick shows you nothing until the game starts, and a check that
//! reasons from one visible enemy is worse than a check that says nothing.

use super::tags::{Answer, DamageType, Tags};
use super::{all_or_both, count_word, list, pick, Priority, Suggestion, TeamView};

/// Below this many visible enemies, the damage split is not a reading of the
/// enemy team — it is a reading of whoever happened to lock in first.
const ENOUGH_TO_CALL_A_SPLIT: usize = 3;

/// One healer is worth answering. Two is worth hurrying for.
const HEALERS_WORTH_RUSHING: usize = 2;

/// Tenacity is a whole item slot, so it wants a team that genuinely leans on
/// crowd control rather than one champion who happens to have a stun.
const CC_WORTH_AN_ITEM: usize = 3;

/// Enough divers that resists stop being a late purchase.
const DIVERS_WORTH_HURRYING_FOR: usize = 2;

/// What the enemy team forces. `ours` is the damage type of the champion the
/// user locked, used only to keep the items suggested buildable by them.
pub fn enemy_threat(tags: &Tags, enemy: &TeamView<'_>, ours: DamageType) -> Vec<Suggestion> {
    let mut suggestions = Vec::new();

    let divers = enemy.count(|champion| champion.dive);
    let dived = divers >= DIVERS_WORTH_HURRYING_FOR;

    suggestions.extend(damage_split(tags, enemy, ours, dived));
    suggestions.extend(antiheal(tags, enemy, ours));
    suggestions.extend(tenacity(tags, enemy, ours));

    suggestions
}

/// The armour/magic-resist split.
///
/// Mixed champions count half to each side, and true damage counts to
/// neither — nothing defends against it, so it must not drag the split
/// towards a resist that will not help.
fn damage_split(
    tags: &Tags,
    enemy: &TeamView<'_>,
    ours: DamageType,
    dived: bool,
) -> Vec<Suggestion> {
    if enemy.known().len() < ENOUGH_TO_CALL_A_SPLIT {
        return Vec::new();
    }

    let mut physical = 0.0_f32;
    let mut magic = 0.0_f32;
    for champion in enemy.known() {
        match champion.damage_type {
            DamageType::Ad => physical += 1.0,
            DamageType::Ap => magic += 1.0,
            DamageType::Mixed => {
                physical += 0.5;
                magic += 0.5;
            }
            DamageType::True => {}
        }
    }

    let total = physical + magic;
    if total == 0.0 {
        return Vec::new();
    }

    // Two thirds is the point where one resist is clearly the buy and the
    // other is a wasted slot. Anything flatter is a genuinely mixed team, and
    // saying so is more useful than picking a side.
    let leaning = 2.0 / 3.0;
    let (answer, sentence) = if physical / total >= leaning {
        let names = enemy.names(|champion| champion.damage_type != DamageType::Ap);
        (
            Answer::Armor,
            format!("{} deal physical damage.", list(&names)),
        )
    } else if magic / total >= leaning {
        let names = enemy.names(|champion| champion.damage_type != DamageType::Ad);
        (Answer::Mr, format!("{} deal magic damage.", list(&names)))
    } else {
        // A split team punishes stacking one resist. Say that, and let the
        // player pick the side their lane is losing.
        let physical_names = enemy.names(|champion| champion.damage_type == DamageType::Ad);
        return pick(tags, Answer::Armor, ours)
            .into_iter()
            .take(1)
            .chain(pick(tags, Answer::Mr, ours).into_iter().take(1))
            .map(|(id, _)| {
                Suggestion::rule(
                    id,
                    Priority::Situational,
                    format!(
                        "The enemy damage is split — {} on the physical side. \
                         Stacking one resist leaves you open to the other, so buy \
                         towards whichever half is actually killing you.",
                        list(&physical_names)
                    ),
                )
            })
            .collect();
    };

    let (priority, urgency) = if dived {
        let divers = enemy.names(|champion| champion.dive);
        (
            Priority::Rush,
            format!(
                " {} can reach you, so the resist wants to be early rather than third.",
                list(&divers)
            ),
        )
    } else {
        (Priority::Core, String::new())
    };

    pick(tags, answer, ours)
        .into_iter()
        .map(|(id, _)| Suggestion::rule(id, priority, format!("{sentence}{urgency}")))
        .collect()
}

/// Antiheal, and when it has to be online.
///
/// The timing is the substance of this rule. Grievous Wounds arrives on an
/// eight-hundred gold component precisely so it can be bought before it is
/// convenient, and against two healers that is the difference between winning
/// a fight and watching it reset.
fn antiheal(tags: &Tags, enemy: &TeamView<'_>, ours: DamageType) -> Vec<Suggestion> {
    let healers = enemy.names(|champion| champion.sustain);
    if healers.is_empty() {
        return Vec::new();
    }

    let hurry = healers.len() >= HEALERS_WORTH_RUSHING;
    let picked = pick(tags, Answer::Antiheal, ours);

    picked
        .into_iter()
        .map(|(id, item)| {
            let reason = if hurry {
                format!(
                    "{} {} heal. With {} of them, antiheal is not a late \
                     purchase — the component pays for itself the first time a \
                     fight goes long.",
                    list(&healers),
                    all_or_both(healers.len()),
                    count_word(healers.len())
                )
            } else {
                format!(
                    "{} heals through damage. Antiheal cuts that roughly in half.",
                    list(&healers)
                )
            };

            // The component is the thing you can act on now; the finished item
            // is where it goes. Only the component carries the hurry.
            let priority = match (hurry, item.is_component) {
                (true, true) => Priority::Rush,
                (true, false) => Priority::Core,
                (false, true) => Priority::Core,
                (false, false) => Priority::Situational,
            };

            Suggestion::rule(id, priority, reason)
        })
        .collect()
}

/// Tenacity, when the enemy team's crowd control is the thing that kills you.
fn tenacity(tags: &Tags, enemy: &TeamView<'_>, ours: DamageType) -> Vec<Suggestion> {
    let controllers = enemy.names(|champion| champion.hard_cc);
    if controllers.len() < CC_WORTH_AN_ITEM {
        return Vec::new();
    }

    pick(tags, Answer::Tenacity, ours)
        .into_iter()
        .map(|(id, _)| {
            Suggestion::rule(
                id,
                Priority::Situational,
                format!(
                    "{} {} bring hard crowd control. Tenacity shortens every one \
                     of those, which matters more than the resist when the thing \
                     killing you is being unable to move.",
                    list(&controllers),
                    all_or_both(controllers.len())
                ),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recommend::SuggestionSource;

    fn tags() -> &'static Tags {
        Tags::get()
    }

    /// Amumu, Malphite, Lux, Brand, Sona — magic damage top to bottom.
    const ALL_MAGIC: [u32; 5] = [32, 54, 99, 63, 37];
    /// Darius, Zed, Caitlyn, Talon, Draven — physical top to bottom.
    const ALL_PHYSICAL: [u32; 5] = [122, 238, 51, 91, 119];

    #[test]
    fn a_magic_team_asks_for_magic_resist() {
        let view = TeamView::from_ids(tags(), ALL_MAGIC);
        let found = enemy_threat(tags(), &view, DamageType::Ad);

        let resists: Vec<_> = found
            .iter()
            .filter(|s| s.reason.contains("magic damage"))
            .collect();
        assert!(!resists.is_empty(), "nothing suggested magic resist");
        for suggestion in resists {
            let item = tags().item(suggestion.item_id).unwrap();
            assert!(
                item.answers.contains(&Answer::Mr),
                "{} is not an answer to magic damage",
                item.name
            );
        }

        // Sona heals, so this team earns antiheal as well — and the component
        // named has to be the one an ability power champion can actually use.
        let antiheal: Vec<_> = found
            .iter()
            .filter(|s| tags().item(s.item_id).unwrap().answers.contains(&Answer::Antiheal))
            .collect();
        assert!(!antiheal.is_empty(), "Sona went unanswered");
    }

    #[test]
    fn a_physical_team_asks_for_armour() {
        let view = TeamView::from_ids(tags(), ALL_PHYSICAL);
        let found = enemy_threat(tags(), &view, DamageType::Ap);

        let armour: Vec<_> = found
            .iter()
            .filter(|s| tags().item(s.item_id).unwrap().answers.contains(&Answer::Armor))
            .collect();
        assert!(!armour.is_empty(), "nothing suggested armour");
    }

    #[test]
    fn a_split_team_is_told_it_is_split_rather_than_sold_one_resist() {
        // Darius, Zed (physical) against Lux, Brand (magic), plus Ezreal.
        let view = TeamView::from_ids(tags(), [122, 238, 99, 63, 81]);
        let found = enemy_threat(tags(), &view, DamageType::Ad);

        let split: Vec<_> = found
            .iter()
            .filter(|s| s.reason.contains("split"))
            .collect();
        assert_eq!(split.len(), 2, "one suggestion per side of the split");
        assert!(split.iter().all(|s| s.priority == Priority::Situational));
    }

    #[test]
    fn two_enemies_are_not_enough_to_call_a_damage_split() {
        // Two magic-damage enemies visible, three seats still hidden.
        let view = TeamView::from_ids(tags(), [99, 63, 0, 0, 0]);
        let found = enemy_threat(tags(), &view, DamageType::Ad);

        assert!(
            !found.iter().any(|s| s.reason.contains("magic damage")),
            "a split was called from two visible enemies"
        );
    }

    #[test]
    fn one_healer_earns_antiheal_and_two_earn_it_early() {
        // Soraka alone.
        let one = TeamView::from_ids(tags(), [16, 51, 238, 91, 119]);
        let found = enemy_threat(tags(), &one, DamageType::Ad);
        let antiheal: Vec<_> = found
            .iter()
            .filter(|s| tags().item(s.item_id).unwrap().answers.contains(&Answer::Antiheal))
            .collect();
        assert!(!antiheal.is_empty(), "Soraka went unanswered");
        assert!(
            antiheal.iter().all(|s| s.priority != Priority::Rush),
            "one healer is not an emergency"
        );

        // Soraka and Aatrox.
        let two = TeamView::from_ids(tags(), [16, 266, 51, 238, 119]);
        let found = enemy_threat(tags(), &two, DamageType::Ad);
        assert!(
            found
                .iter()
                .any(|s| s.priority == Priority::Rush
                    && tags().item(s.item_id).unwrap().answers.contains(&Answer::Antiheal)),
            "two healers should make antiheal a rush"
        );
    }

    #[test]
    fn a_team_that_does_not_heal_is_not_sold_antiheal() {
        // Zed, Talon, Caitlyn, Lux, Xerath — no sustain among them.
        let view = TeamView::from_ids(tags(), [238, 91, 51, 99, 101]);
        let found = enemy_threat(tags(), &view, DamageType::Ad);

        assert!(
            !found
                .iter()
                .any(|s| tags().item(s.item_id).unwrap().answers.contains(&Answer::Antiheal)),
            "antiheal was suggested against a team with no healing"
        );
    }

    #[test]
    fn divers_make_the_resist_a_rush() {
        // Zed, Talon, Khazix, Rengar, Kayn — physical, and every one of them
        // is looking at your back line.
        let view = TeamView::from_ids(tags(), [238, 91, 121, 107, 141]);
        let found = enemy_threat(tags(), &view, DamageType::Ap);

        let resists: Vec<_> = found
            .iter()
            .filter(|s| s.reason.contains("physical damage"))
            .collect();
        assert!(!resists.is_empty(), "nothing suggested armour");
        assert!(
            resists.iter().all(|s| s.priority == Priority::Rush),
            "five divers and the armour is not urgent"
        );
        assert!(resists.iter().all(|s| s.reason.contains("can reach you")));
    }

    #[test]
    fn an_empty_champ_select_produces_nothing() {
        let view = TeamView::from_ids(tags(), []);
        assert!(enemy_threat(tags(), &view, DamageType::Ad).is_empty());

        // And a blind pick where nothing is visible at all.
        let hidden = TeamView::from_ids(tags(), [0, 0, 0, 0, 0]);
        assert!(enemy_threat(tags(), &hidden, DamageType::Ad).is_empty());
    }

    #[test]
    fn every_suggestion_is_a_rule_and_carries_no_number() {
        // The colour law: an amber block explains itself in words. A digit in
        // one of these reasons is a statistic the engine cannot back.
        for ids in [ALL_MAGIC, ALL_PHYSICAL] {
            let view = TeamView::from_ids(tags(), ids);
            for ours in [DamageType::Ad, DamageType::Ap, DamageType::Mixed] {
                for suggestion in enemy_threat(tags(), &view, ours) {
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

    #[test]
    fn suggested_items_are_ones_the_champion_can_use() {
        let view = TeamView::from_ids(tags(), ALL_MAGIC);

        for suggestion in enemy_threat(tags(), &view, DamageType::Ad) {
            let item = tags().item(suggestion.item_id).unwrap();
            assert_ne!(
                item.damage,
                Some(DamageType::Ap),
                "{} is an ability power item, and we are physical damage",
                item.name
            );
        }
    }
}
