//! The recommendation engine.
//!
//! Each check reads the champions in a champ select and returns zero or more
//! suggestions. The checks are independent: they do not consult each other,
//! they may disagree, and any of them staying silent is a normal result.
//!
//! Everything in here is a **rule**. A rule reasons from tags — "Aatrox and
//! Soraka both heal" — and it is rendered in amber, which the app promises
//! means reasoning rather than measurement. Nothing in this module may
//! produce [`SuggestionSource::Stat`]; that colour belongs to the build data
//! layer, which has sample sizes to back it. Conflating them is the one
//! styling mistake in this project that is a correctness bug, so a test holds
//! every reason written here to containing no digit at all.

pub mod gaps;
pub mod state;
pub mod tags;
pub mod threat;

use serde::{Deserialize, Serialize};

pub use gaps::team_gaps;
pub use state::{game_state, standing, Footing, Standing};
pub use tags::{Answer, ChampionTags, DamageType, ItemTags, Tags};
pub use threat::enemy_threat;

/// Where a suggestion came from, and therefore which colour it wears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SuggestionSource {
    /// Measured. Carries a sample size, rendered teal.
    Stat,
    /// Reasoned from tags. Carries a sentence, rendered amber, never a number.
    Rule,
}

/// How much of a hurry the suggestion is in. This is the antiheal-timing
/// distinction generalised: the same item is a different piece of advice
/// depending on whether you want it on your next back or by the third item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// Buy it on the next back, before it is comfortable to.
    Rush,
    /// Fold it into the main path.
    Core,
    /// Only if the game turns this way.
    Situational,
}

/// One piece of advice about one item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub item_id: u32,
    pub priority: Priority,
    /// A whole sentence, shown as written. It explains itself because an
    /// amber block has no number to lean on.
    pub reason: String,
    pub source: SuggestionSource,
}

impl Suggestion {
    /// Every constructor in this module goes through here, so there is one
    /// place where "a check produces rules" is enforced rather than remembered.
    fn rule(item_id: u32, priority: Priority, reason: impl Into<String>) -> Suggestion {
        Suggestion {
            item_id,
            priority,
            reason: reason.into(),
            source: SuggestionSource::Rule,
        }
    }
}

/// One team as champ select currently shows it.
///
/// `unknown` is the count of seats we cannot reason about: still hovering,
/// hidden by a blind pick queue, or a champion the tag file has never heard
/// of. It is kept rather than discarded because "we can only see two of them"
/// is the difference between a check that stays quiet and one that guesses.
#[derive(Debug, Clone, Default)]
pub struct TeamView<'a> {
    known: Vec<&'a ChampionTags>,
    unknown: usize,
}

impl<'a> TeamView<'a> {
    /// Build a view from whatever champ select has given us. `None` entries
    /// are seats we cannot see.
    pub fn new(seats: impl IntoIterator<Item = Option<&'a ChampionTags>>) -> TeamView<'a> {
        let mut view = TeamView::default();
        for seat in seats {
            match seat {
                Some(tags) => view.known.push(tags),
                None => view.unknown += 1,
            }
        }
        view
    }

    /// Resolve numeric champion ids — what champ select actually sends — into
    /// a view. An id with no tags counts as unknown, never as a blank.
    pub fn from_ids(tags: &'a Tags, ids: impl IntoIterator<Item = u32>) -> TeamView<'a> {
        TeamView::new(ids.into_iter().map(|id| tags.champion_by_id(id)))
    }

    pub fn known(&self) -> &[&'a ChampionTags] {
        &self.known
    }

    pub fn unknown(&self) -> usize {
        self.unknown
    }

    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }

    fn count(&self, predicate: impl Fn(&ChampionTags) -> bool) -> usize {
        self.known.iter().filter(|tags| predicate(tags)).count()
    }

    fn names(&self, predicate: impl Fn(&ChampionTags) -> bool) -> Vec<&'a str> {
        self.known
            .iter()
            .filter(|tags| predicate(tags))
            .map(|tags| tags.name.as_str())
            .collect()
    }
}

/// Join names the way a sentence would: "Aatrox", "Aatrox and Soraka",
/// "Aatrox, Soraka and Vladimir".
fn list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The floor for what counts as an item you *finish*.
///
/// Doran's Helm carries armour and builds into nothing, which makes it the
/// cheapest finished armour item in the file and terrible advice: it is a
/// starting item, not the thing a resist plan ends at. Anything under this is
/// something you open with.
const FINISHED_ITEM_FLOOR: u32 = 1_000;

/// "both" for two, "all" for more. A sentence that says "Darius and Warwick
/// all heal" reads as a machine wrote it, which undermines the one thing an
/// amber block has going for it — that it argues in words.
fn all_or_both(n: usize) -> &'static str {
    if n == 2 {
        "both"
    } else {
        "all"
    }
}

/// Counts as words. Amber blocks carry no digits, so a check that wants to
/// say how many enemies heal has to spell it.
fn count_word(n: usize) -> &'static str {
    match n {
        0 => "none",
        1 => "one",
        2 => "two",
        3 => "three",
        4 => "four",
        5 => "five",
        _ => "most",
    }
}

/// Whether an item's own stat line suits the champion holding it. An item
/// with no offensive stats suits everyone.
fn suits(item: &ItemTags, ours: DamageType) -> bool {
    match (item.damage, ours) {
        (None, _) => true,
        (Some(_), DamageType::Mixed) => true,
        (Some(item_damage), champion) => item_damage == champion,
    }
}

/// Pick the items to actually name for an answer.
///
/// At most one component and one finished item: the component is what makes
/// the advice actionable on the next back, and the finished item is where it
/// ends up. Naming all twenty-six armour items would be a list, not advice.
///
/// The two slots are ranked differently, because they answer different
/// questions.
///
/// The component answers "what can I buy on this back?", so cost leads and a
/// tie goes to the item that also carries your damage. The finished item
/// answers "what am I holding at the end?", so carrying your damage leads and
/// cost only breaks ties — Thornmail is the cheapest completed answer to
/// healing, but a mage who buys it has spent a slot on armour they cannot
/// use, and Morellonomicon is worth the extra four hundred gold.
fn pick(tags: &Tags, answer: Answer, ours: DamageType) -> Vec<(u32, &ItemTags)> {
    let candidates: Vec<(u32, &ItemTags)> = tags
        .items_answering(answer)
        .into_iter()
        .filter(|(_, item)| suits(item, ours))
        .collect();

    let carries_our_damage =
        |item: &ItemTags| item.damage.is_some_and(|damage| damage == ours);

    let component = candidates
        .iter()
        .filter(|(_, item)| item.is_component)
        .min_by_key(|(id, item)| (item.cost, !carries_our_damage(item), *id));

    let finished = candidates
        .iter()
        .filter(|(_, item)| !item.is_component && item.cost >= FINISHED_ITEM_FLOOR)
        .min_by_key(|(id, item)| (!carries_our_damage(item), item.cost, *id));

    let mut picked = Vec::new();
    if let Some(entry) = component {
        picked.push(*entry);
    }
    if let Some(entry) = finished {
        picked.push(*entry);
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_as_a_sentence() {
        assert_eq!(list(&[]), "");
        assert_eq!(list(&["Aatrox"]), "Aatrox");
        assert_eq!(list(&["Aatrox", "Soraka"]), "Aatrox and Soraka");
        assert_eq!(
            list(&["Aatrox", "Soraka", "Vladimir"]),
            "Aatrox, Soraka and Vladimir"
        );
    }

    #[test]
    fn an_unseen_seat_is_counted_not_dropped() {
        let tags = Tags::get();
        // A blind pick queue: two enemies visible, three still hidden.
        let view = TeamView::from_ids(tags, [266, 103, 0, 0, 0]);
        assert_eq!(view.known().len(), 2);
        assert_eq!(view.unknown(), 3);
    }

    #[test]
    fn an_untagged_champion_counts_as_unseen_rather_than_as_nothing() {
        let tags = Tags::get();
        let view = TeamView::from_ids(tags, [103, 999_999]);
        assert_eq!(view.known().len(), 1);
        assert_eq!(
            view.unknown(),
            1,
            "a champion we have no tags for is a hole in what we can see, \
             not a champion with every tag false"
        );
    }

    #[test]
    fn items_are_filtered_to_the_champion_holding_them() {
        let tags = Tags::get();

        let zhonyas = tags.item(3157).expect("Zhonya's Hourglass");
        assert!(suits(zhonyas, DamageType::Ap));
        assert!(!suits(zhonyas, DamageType::Ad), "Garen has no use for it");
        assert!(suits(zhonyas, DamageType::Mixed));

        let chain_vest = tags.item(1031).expect("Chain Vest");
        assert!(suits(chain_vest, DamageType::Ad), "a bare resist suits all");
        assert!(suits(chain_vest, DamageType::Ap));
    }

    #[test]
    fn a_pick_names_something_to_buy_now_and_something_to_finish() {
        let tags = Tags::get();
        let picked = pick(tags, Answer::Antiheal, DamageType::Ad);
        assert_eq!(picked.len(), 2);
        assert!(picked[0].1.is_component, "the first is buyable early");
        assert!(!picked[1].1.is_component);
        assert!(picked.iter().all(|(_, item)| suits(item, DamageType::Ad)));
    }

    #[test]
    fn a_starting_item_is_not_offered_as_the_item_you_finish() {
        let tags = Tags::get();
        // Doran's Helm carries armour, builds into nothing, and costs less
        // than every real defensive item. Naming it as the end of an armour
        // path would be worse than naming nothing.
        let picked = pick(tags, Answer::Armor, DamageType::Ad);
        let dorans = tags.item(1120).expect("Doran's Helm");
        assert!(dorans.answers.contains(&Answer::Armor));
        assert!(
            !picked.iter().any(|(id, _)| *id == 1120),
            "a starting item was offered as a finished one"
        );
        assert!(picked
            .iter()
            .any(|(_, item)| !item.is_component && item.cost >= FINISHED_ITEM_FLOOR));
    }

    #[test]
    fn a_mage_finishes_the_ability_power_antiheal_even_though_it_costs_more() {
        let tags = Tags::get();

        let ap = pick(tags, Answer::Antiheal, DamageType::Ap);
        let (_, finished) = ap.last().expect("something finishes the path");
        assert_eq!(finished.name, "Morellonomicon");

        let ad = pick(tags, Answer::Antiheal, DamageType::Ad);
        let (_, finished) = ad.last().expect("something finishes the path");
        assert_eq!(finished.damage, Some(DamageType::Ad));

        // The component is still whatever is cheapest and usable, because it
        // is bought on a back rather than kept for the game.
        let (_, component) = ap.first().unwrap();
        assert_eq!(component.cost, 800);
    }

    #[test]
    fn two_are_both_and_more_are_all() {
        assert_eq!(all_or_both(2), "both");
        assert_eq!(all_or_both(3), "all");
        assert_eq!(all_or_both(5), "all");
    }
}
