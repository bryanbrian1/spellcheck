//! Check two: what your own composition leaves you to cover.
//!
//! This check is deliberately narrow, and the reason is worth stating plainly
//! rather than hiding behind a thin implementation.
//!
//! Most team gaps are answered by a champion pick or by how the team plays,
//! not by an item. An all-physical-damage team is a real problem — the enemy
//! buys armour once and halves all five of you — but the item that answers it
//! is armour penetration, and `answers` in `data/meta/items.json` is a closed
//! defensive vocabulary: armour, magic resist, antiheal, tenacity, shield
//! reduction, crit reduction. The same is true of "nobody can start a fight"
//! and "nobody has hard crowd control": both are real readings of a champ
//! select, and neither has an item behind it.
//!
//! So this check emits the one gap that *is* an item decision — nobody can
//! hold a front line, therefore you are going to be touched — and stays quiet
//! about the rest rather than inventing advice to fill the space. Extending
//! it means extending the answer vocabulary first.

use super::tags::{Answer, DamageType, Tags};
use super::{list, pick, Priority, Suggestion, TeamView};

/// Below this many visible allies, "nobody has a front line" is a statement
/// about champ select being incomplete, not about the team.
const ENOUGH_TO_CALL_A_GAP: usize = 4;

/// What your own team leaves uncovered.
///
/// `ally` is your whole team including you — you are part of your own
/// composition, and a team whose only durable body is yours has no gap.
pub fn team_gaps(tags: &Tags, ally: &TeamView<'_>, ours: DamageType) -> Vec<Suggestion> {
    no_frontline(tags, ally, ours)
}

/// Nobody on the team can hold a front line.
///
/// The consequence is specific: fights start on top of whoever is squishiest,
/// and there is no body between the enemy and you. Resists are the answer
/// available to any champion, which is why this is the one team gap that
/// survives into an item suggestion.
fn no_frontline(tags: &Tags, ally: &TeamView<'_>, ours: DamageType) -> Vec<Suggestion> {
    if ally.known().len() < ENOUGH_TO_CALL_A_GAP {
        return Vec::new();
    }
    if ally.count(|champion| champion.frontline) > 0 {
        return Vec::new();
    }

    let names = ally.names(|_| true);

    // Which resist is not knowable from your own team — that is check one's
    // question. Name the cheap component of each and let the enemy decide.
    pick(tags, Answer::Armor, ours)
        .into_iter()
        .take(1)
        .chain(pick(tags, Answer::Mr, ours).into_iter().take(1))
        .map(|(id, _)| {
            Suggestion::rule(
                id,
                Priority::Situational,
                format!(
                    "Nobody on your team can hold a front line — {} are all squishy. \
                     Fights will start on top of you, so a cheap resist buys more \
                     than the same gold spent on damage.",
                    list(&names)
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

    /// Lux, Zed, Caitlyn, Soraka, Ezreal — not a durable body among them.
    const NO_FRONTLINE: [u32; 5] = [99, 238, 51, 16, 81];
    /// Malphite is the front line; the rest are not.
    const HAS_FRONTLINE: [u32; 5] = [54, 238, 51, 16, 81];

    #[test]
    fn a_team_with_no_durable_body_is_told_so() {
        let view = TeamView::from_ids(tags(), NO_FRONTLINE);
        let found = team_gaps(tags(), &view, DamageType::Ap);

        assert!(!found.is_empty());
        assert!(found[0].reason.contains("front line"));
        for suggestion in &found {
            let item = tags().item(suggestion.item_id).unwrap();
            assert!(
                item.answers.contains(&Answer::Armor) || item.answers.contains(&Answer::Mr),
                "{} is not a resist",
                item.name
            );
        }
    }

    #[test]
    fn one_tank_is_enough_to_close_the_gap() {
        let view = TeamView::from_ids(tags(), HAS_FRONTLINE);
        assert!(
            team_gaps(tags(), &view, DamageType::Ap).is_empty(),
            "Malphite is a front line and the check did not notice"
        );
    }

    #[test]
    fn a_half_finished_champ_select_is_not_a_team_gap() {
        // Three squishy picks locked, two seats still empty. The team may yet
        // pick a tank, so claiming it has no front line would be a guess.
        let view = TeamView::from_ids(tags(), [99, 238, 51, 0, 0]);
        assert!(team_gaps(tags(), &view, DamageType::Ap).is_empty());
    }

    #[test]
    fn an_empty_team_produces_nothing() {
        let view = TeamView::from_ids(tags(), []);
        assert!(team_gaps(tags(), &view, DamageType::Ad).is_empty());
    }

    #[test]
    fn every_suggestion_is_a_rule_and_carries_no_number() {
        let view = TeamView::from_ids(tags(), NO_FRONTLINE);
        for ours in [DamageType::Ad, DamageType::Ap, DamageType::Mixed] {
            for suggestion in team_gaps(tags(), &view, ours) {
                assert_eq!(suggestion.source, SuggestionSource::Rule);
                assert!(
                    !suggestion.reason.chars().any(|c| c.is_ascii_digit()),
                    "reason carries a number: {}",
                    suggestion.reason
                );
            }
        }
    }

    #[test]
    fn suggested_items_are_ones_the_champion_can_use() {
        let view = TeamView::from_ids(tags(), NO_FRONTLINE);
        for suggestion in team_gaps(tags(), &view, DamageType::Ad) {
            let item = tags().item(suggestion.item_id).unwrap();
            assert_ne!(item.damage, Some(DamageType::Ap), "{}", item.name);
        }
    }
}
