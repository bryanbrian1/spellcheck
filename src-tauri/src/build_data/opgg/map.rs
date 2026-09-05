//! OP.GG payload → internal schema.
//!
//! The payload arrives as [`wire`](super::wire) JSON: a `data` object holding
//! one section per part of a build, each section carrying parallel `ids` and
//! `ids_names` arrays plus its own `play` and `win` counts. The shape is known
//! exactly — it was read off the live endpoint — so this maps it directly
//! rather than searching for candidate key names.
//!
//! Every section is optional. A champion-role pair with no core items is a
//! pair we have nothing to show, which is [`BuildLookup::NoData`], not an
//! error; a missing rune page just means no rune block.

use serde_json::{json, Value};

use crate::build_data::mapping::{ids_field, skill_plan, stats_from, str_field};
use crate::build_data::schema::{
    BuildLookup, BuildRequest, BuildStats, ChampionBuild, ChampionRef, ItemGroup, ItemPlan, ItemRef,
    LaneAdvantage, MatchupInfo, RunePage, SkillPlan, SourceInfo, SummonerSet, SummonerSpell,
};

/// Translate one `lol_get_champion_analysis` payload.
pub fn build_from_payload(
    payload: &Value,
    request: &BuildRequest,
    provider_label: &str,
) -> BuildLookup {
    // A bare string is the endpoint answering in prose rather than data — in
    // practice, "nothing found for this pair".
    if let Value::String(text) = payload {
        return BuildLookup::no_data(request, summarise(text));
    }

    let data = payload.get("data").unwrap_or(payload);

    let core = group(data.get("core_items"));
    let items = ItemPlan {
        starters: group(data.get("starter_items")).into_iter().collect(),
        boots: group(data.get("boots")).into_iter().collect(),
        situational: situational(data, core.as_ref()),
        core: core.into_iter().collect(),
    };

    // Items are what the app exists to show. Without them there is no build,
    // whatever else came back.
    if items.is_empty() {
        return BuildLookup::no_data(
            request,
            format!(
                "no build data for {} {} from this source",
                request.display_name(),
                request.role
            ),
        );
    }

    let build = ChampionBuild {
        champion: ChampionRef {
            key: request.champion_key.clone(),
            // The payload echoes the champion in the endpoint's own spelling
            // (`AHRI`, `MONKEYKING`), which is an argument, not a display
            // name. What the caller already knew is better.
            name: request.display_name().to_string(),
            id: request.champion_id,
        },
        role: request.role,
        source: SourceInfo {
            provider_label: provider_label.to_string(),
            patch: patch(data),
            // Region and tier are query parameters, not payload fields; the
            // provider fills them in from the configuration it asked with.
            region: None,
            tier: None,
            updated_at: None,
        },
        stats: data
            .get("summary")
            .and_then(|summary| summary.get("average_stats"))
            .and_then(stats_from),
        // The ordinary lookup asks about a champion in a lane, not about a
        // lane against somebody. Nothing here is filtered to an opponent, so
        // nothing here may claim to be.
        matchup: None,
        items,
        runes: runes(data.get("runes")).into_iter().collect(),
        summoners: summoners(data.get("summoner_spells")).into_iter().collect(),
        skills: skills(data),
    };

    BuildLookup::found(build)
}

/// Translate one `lol_get_lane_matchup_guide` payload.
///
/// A different tool from the one [`build_from_payload`] reads, and a
/// different shape, but the same destination: the UI must not be able to tell
/// a matchup build from an ordinary one except by
/// [`ChampionBuild::matchup`] being filled in.
///
/// Two things about this payload are worth stating plainly, because both are
/// easy to get wrong in a way that produces a confident, wrong number.
///
/// **Not everything in it is about the matchup.** `data.summary` is the
/// champion's overall record — 181,664 games of Ahri, the same figure the
/// ordinary lookup returns — sitting in the same object as sections that
/// *are* filtered to the opponent. Reading the build's headline stats from
/// there would print Ahri's global win rate under the words "vs Zed". The
/// honest sample is in `data.counters[]`, which is the per-opponent
/// breakdown and is where [`matchup_stats`] reads it from.
///
/// **Every section is a menu, not an answer.** Where the analysis tool
/// returns one core path, this returns fifteen ordered by how often they were
/// built. The first entry is the most-played one, which is the same thing the
/// other tool would have given us, so each section is narrowed to its head.
pub fn matchup_from_payload(
    payload: &Value,
    request: &BuildRequest,
    provider_label: &str,
) -> BuildLookup {
    if let Value::String(text) = payload {
        return BuildLookup::no_data(request, summarise(text));
    }

    let data = payload.get("data").unwrap_or(payload);

    // The most-played option in each section. See the doc comment: these are
    // menus, and their heads are the build.
    let core = group(first(data.get("core_items")));
    let items = ItemPlan {
        starters: group(first(data.get("starter_items"))).into_iter().collect(),
        boots: group(first(data.get("boots"))).into_iter().collect(),
        // `last_items` is this payload's only pool of alternatives — it has
        // no fourth/fifth/sixth slots — and here, unlike in the ordinary
        // lookup, it is filtered to the matchup. Its head is still the core
        // over again, which is what `situational_from` drops.
        situational: situational_from(data.get("last_items"), core.as_ref()),
        core: core.into_iter().collect(),
    };

    if items.is_empty() {
        return BuildLookup::no_data(
            request,
            format!(
                "no build data for {} {} against {} from this source",
                request.display_name(),
                request.role,
                opponent_name(payload, request),
            ),
        );
    }

    let build = ChampionBuild {
        champion: ChampionRef {
            key: request.champion_key.clone(),
            name: request.display_name().to_string(),
            id: request.champion_id,
        },
        role: request.role,
        source: SourceInfo {
            provider_label: provider_label.to_string(),
            patch: patch(data),
            region: None,
            // This tool takes no tier argument, so no tier was asked for and
            // none may be claimed. The ordinary lookup fills this in from the
            // configuration it queried with; there is nothing to fill in here.
            tier: None,
            updated_at: None,
        },
        stats: matchup_stats(data, payload, request),
        matchup: Some(MatchupInfo {
            opponent: ChampionRef {
                key: request.opponent_key.clone().unwrap_or_default(),
                name: opponent_name(payload, request),
                id: None,
            },
            tip: str_field(data, &["opponent_champion_tip"]),
            lane_advantage: lane_advantage(data, payload, request),
            play_style: str_field(data, &["recommended_play_style"]),
        }),
        items,
        runes: runes(first(data.get("runes"))).into_iter().collect(),
        summoners: summoners(first(data.get("summoner_spells")))
            .into_iter()
            .collect(),
        skills: matchup_skills(data),
    };

    BuildLookup::found(build)
}

/// The head of a section that arrives as a list of options.
///
/// Passed through unchanged when it is not a list, so a section that arrives
/// as a bare object — which the other tool's payloads do — still reads.
fn first(section: Option<&Value>) -> Option<&Value> {
    match section {
        Some(Value::Array(entries)) => entries.first(),
        other => other,
    }
}

/// How this champion actually does against this one opponent.
///
/// `data.counters[]` is the only honestly matchup-scoped sample in the
/// payload: one row per opponent, carrying the games played and won. It is
/// matched by champion name because that is what the rows carry — the key we
/// asked with never appears in them.
///
/// `None` when the opponent has no row, which is what a matchup too thin to
/// have been counted looks like. No fallback to the champion's overall
/// record: a number that says "51%" under the words "vs Zed" while meaning
/// "vs everyone" is worse than no number at all.
fn matchup_stats(data: &Value, payload: &Value, request: &BuildRequest) -> Option<BuildStats> {
    let opponent = opponent_name(payload, request);
    let row = data
        .get("counters")?
        .as_array()?
        .iter()
        .find(|row| str_field(row, &["champion_name"]).as_deref() == Some(opponent.as_str()))?;

    let play = row.get("play").and_then(Value::as_u64)?;
    let win = row.get("win").and_then(Value::as_u64);

    BuildStats {
        games: Some(play),
        // Guarded rather than assumed: a row with no games would divide by
        // zero, and a source is not obliged to omit one.
        win_rate: win.filter(|_| play > 0).map(|win| win as f64 / play as f64),
        pick_rate: None,
        ban_rate: None,
    }
    .non_empty()
}

/// Who the source thinks wins the lane.
///
/// It names a champion rather than a side, so the name is compared against
/// both seats. A name matching neither — or absent — is `Even` rather than a
/// guess at which side it meant.
fn lane_advantage(data: &Value, payload: &Value, request: &BuildRequest) -> Option<LaneAdvantage> {
    let named = str_field(data, &["lane_advantage_champion"])?;
    let opponent = opponent_name(payload, request);

    Some(if named == opponent {
        LaneAdvantage::Theirs
    } else if named == request.display_name() || Some(named.as_str()) == str_field(payload, &["my_champion"]).as_deref() {
        LaneAdvantage::Ours
    } else {
        LaneAdvantage::Even
    })
}

/// The opponent's display name.
///
/// The payload echoes it in proper case (`Zed`), which is better to show than
/// the `ZED` we asked with. Falls back to the key when the echo is missing.
fn opponent_name(payload: &Value, request: &BuildRequest) -> String {
    str_field(payload, &["opponent_champion"])
        .filter(|name| !name.is_empty())
        .or_else(|| request.opponent_key.clone())
        .unwrap_or_default()
}

/// The alternatives, from a single pool rather than three ordered slots.
///
/// Same rule as the ordinary lookup's `situational`: anything already in the
/// core is dropped, because a menu that lists what you are already building
/// is worth nothing.
fn situational_from(section: Option<&Value>, core: Option<&ItemGroup>) -> Vec<ItemGroup> {
    let mut seen: Vec<u32> = core
        .into_iter()
        .flat_map(|group| group.items.iter().map(|item| item.id))
        .collect();

    let mut out = Vec::new();
    for candidate in groups(section) {
        if candidate.items.iter().any(|item| seen.contains(&item.id)) {
            continue;
        }
        seen.extend(candidate.items.iter().map(|item| item.id));
        out.push(candidate);
    }
    out
}

/// Both halves of the skill plan, each the head of its own menu.
fn matchup_skills(data: &Value) -> SkillPlan {
    let order = first(data.get("skills"))
        .and_then(|skills| skills.get("order"))
        .cloned()
        .unwrap_or(Value::Null);
    let priority = first(data.get("skill_masteries"))
        .and_then(|masteries| masteries.get("ids"))
        .cloned()
        .unwrap_or(Value::Null);

    skill_plan(&json!({ "order": order, "priority": priority }))
}

/// One item section: `{ ids, ids_names, play, win }`.
///
/// The two arrays are parallel, so the names are zipped onto the ids here —
/// the UI shows "Malignance" rather than "#3118" only because of this step.
/// An id with no matching name keeps the id.
fn group(section: Option<&Value>) -> Option<ItemGroup> {
    let section = section?;
    let ids = ids_field(section, &["ids"]);
    if ids.is_empty() {
        return None;
    }

    let names = section.get("ids_names").and_then(Value::as_array);
    let items = ids
        .iter()
        .enumerate()
        .map(|(index, id)| ItemRef {
            id: *id,
            name: names
                .and_then(|names| names.get(index))
                .and_then(Value::as_str)
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty()),
        })
        .collect();

    Some(ItemGroup {
        items,
        stats: stats_from(section),
        label: None,
    })
}

/// What to build once the core is done.
///
/// The fourth, fifth and sixth slots in order, each a short menu the endpoint
/// has already sorted by how often it is picked. They are concatenated rather
/// than kept apart because a slot number is not advice — an item that is a
/// popular fourth on one game is a fifth on the next, and the reader wants
/// the pool, not the arithmetic.
///
/// Anything already in the core is dropped, and so is anything named twice
/// across the three slots. Without that the list repeats itself: Serylda's
/// Grudge finishes Zed's core *and* leads his fourth-item menu, and Edge of
/// Night is both a fourth and a fifth. A menu that lists what you are already
/// building is what this section used to be, and it is worth nothing.
fn situational(data: &Value, core: Option<&ItemGroup>) -> Vec<ItemGroup> {
    let mut seen: Vec<u32> = core
        .into_iter()
        .flat_map(|group| group.items.iter().map(|item| item.id))
        .collect();

    let mut out = Vec::new();
    for slot in ["fourth_items", "fifth_items", "sixth_items"] {
        for candidate in groups(data.get(slot)) {
            // Every entry here is a one-item option, but the shape does not
            // promise it, so an entry is kept only if something in it is new.
            if candidate.items.iter().any(|item| seen.contains(&item.id)) {
                continue;
            }
            seen.extend(candidate.items.iter().map(|item| item.id));
            out.push(candidate);
        }
    }
    out
}

/// A list of one-item sections, each its own option.
fn groups(section: Option<&Value>) -> Vec<ItemGroup> {
    section
        .and_then(Value::as_array)
        .map(|entries| entries.iter().filter_map(|entry| group(Some(entry))).collect())
        .unwrap_or_default()
}

fn runes(section: Option<&Value>) -> Option<RunePage> {
    let section = section?;
    let page = RunePage {
        primary_style: section.get("primary_page_id").and_then(Value::as_u64).map(|id| id as u32),
        secondary_style: section.get("secondary_page_id").and_then(Value::as_u64).map(|id| id as u32),
        primary: ids_field(section, &["primary_rune_ids"]),
        secondary: ids_field(section, &["secondary_rune_ids"]),
        shards: ids_field(section, &["stat_mod_ids"]),
        stats: stats_from(section),
        // "Domination", "Precision" — the tree, which is what the block is
        // usefully called.
        label: str_field(section, &["primary_page_name"]),
    };

    (!page.is_empty()).then_some(page)
}

fn summoners(section: Option<&Value>) -> Option<SummonerSet> {
    let section = section?;
    let spells: Vec<SummonerSpell> = ids_field(section, &["ids"])
        .into_iter()
        // OP.GG returns the ids twice rather than naming them, so there is no
        // name to carry here.
        .map(|id| SummonerSpell { id, name: None })
        .collect();

    if spells.is_empty() {
        return None;
    }

    Some(SummonerSet {
        spells,
        stats: stats_from(section),
    })
}

/// The level-by-level order and the max order arrive in separate sections.
fn skills(data: &Value) -> SkillPlan {
    let order = data
        .get("skills")
        .and_then(|skills| skills.get("order"))
        .cloned()
        .unwrap_or(Value::Null);
    let priority = data
        .get("skill_masteries")
        .and_then(|masteries| masteries.get("ids"))
        .cloned()
        .unwrap_or(Value::Null);

    skill_plan(&json!({ "order": order, "priority": priority }))
}

/// The patch these numbers describe, which the endpoint reports as a trend
/// version rather than a field of its own.
///
/// A trend is a series when the whole payload comes back — `16.17`, `16.16`,
/// `16.15`, newest first — and a single flattened value when the ordinary
/// lookup asked for `data.trends.win.version` by name. `first` reads both:
/// the head of the series, or the lone value unchanged.
fn patch(data: &Value) -> Option<String> {
    let trends = data.get("trends")?;
    for key in ["win", "pick", "ban"] {
        if let Some(version) = first(trends.get(key)).and_then(|trend| str_field(trend, &["version"]))
        {
            return Some(version);
        }
    }
    None
}

fn summarise(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= 200 {
        return trimmed.to_string();
    }
    trimmed.chars().take(200).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_data::opgg::wire;
    use crate::build_data::role::Role;
    use crate::build_data::schema::Skill;

    /// Ahri mid, captured from the live endpoint.
    const AHRI: &str = include_str!("testdata/ahri_mid.txt");
    /// Briar support: real, and thin — 23 games on the core path. The kind of
    /// sample the confidence rail exists to dim.
    const BRIAR: &str = include_str!("testdata/briar_support.txt");

    fn request() -> BuildRequest {
        BuildRequest::new("Ahri", Role::Middle).with_champion_id(103)
    }

    fn ahri() -> ChampionBuild {
        let payload = wire::parse(AHRI).unwrap();
        build_from_payload(&payload, &request(), "OP.GG")
            .build()
            .expect("expected a build")
            .clone()
    }

    #[test]
    fn maps_a_live_response() {
        let build = ahri();

        assert_eq!(build.champion.key, "Ahri");
        assert_eq!(build.champion.name, "Ahri");
        assert_eq!(build.champion.id, Some(103));
        assert_eq!(build.role, Role::Middle);
        assert_eq!(build.source.provider_label, "OP.GG");
        assert_eq!(build.source.patch.as_deref(), Some("16.17"));

        let stats = build.stats.expect("build-level stats");
        assert_eq!(stats.games, Some(171985));
        assert_eq!(stats.win_rate, Some(0.51));
        assert_eq!(stats.pick_rate, Some(0.1));
    }

    #[test]
    fn item_names_are_carried_next_to_their_ids() {
        let build = ahri();
        let core = &build.items.core[0];
        assert_eq!(core.items[0].id, 3118);
        assert_eq!(core.items[0].name.as_deref(), Some("Malignance"));
        assert_eq!(core.items[2].name.as_deref(), Some("Zhonya's Hourglass"));
        assert_eq!(build.items.boots[0].items[0].name.as_deref(), Some("Sorcerer's Shoes"));
        assert_eq!(build.items.starters[0].items[0].name.as_deref(), Some("Doran's Ring"));
    }

    /// The four item sections share one class in the wire format and are told
    /// apart only by position, so this is the test that catches a parser that
    /// lost track of which is which.
    #[test]
    fn each_item_section_lands_where_it_belongs() {
        let build = ahri();
        assert_eq!(build.items.core[0].items.len(), 3);
        assert_eq!(build.items.boots[0].items[0].id, 3020);
        assert_eq!(build.items.starters[0].items[0].id, 1056);

        // The fixture's three late slots offer Rabadon's, Zhonya's and Void
        // Staff; then Rabadon's, Void Staff and Zhonya's again; then Cosmic
        // Drive, Void Staff and Stormsurge. Zhonya's finishes the core, and
        // the rest repeat across slots, so four distinct items survive.
        let situational: Vec<u32> = build
            .items
            .situational
            .iter()
            .flat_map(|group| group.items.iter().map(|item| item.id))
            .collect();
        assert_eq!(situational, vec![3089, 3135, 4629, 4646]);
    }

    /// The bug this section had: every item it offered was already in the
    /// core, so it told the player to build what they were building.
    #[test]
    fn situational_never_repeats_the_core() {
        let build = ahri();
        let core: Vec<u32> = build.items.core[0].items.iter().map(|item| item.id).collect();

        for group in &build.items.situational {
            for item in &group.items {
                assert!(
                    !core.contains(&item.id),
                    "{:?} is in the core and offered as situational",
                    item.name
                );
            }
        }
    }

    /// And it must not repeat itself either: an item is a popular fourth on
    /// one game and a fifth on the next, so the three slots overlap heavily.
    #[test]
    fn situational_never_repeats_itself() {
        let build = ahri();
        let mut seen = Vec::new();
        for group in &build.items.situational {
            for item in &group.items {
                assert!(!seen.contains(&item.id), "{:?} listed twice", item.name);
                seen.push(item.id);
            }
        }
    }

    /// `play` and `win` are counts, not a rate. 5,972 wins from 11,341 games
    /// is 52.7%, and it must reach the UI as a fraction.
    #[test]
    fn win_rates_are_derived_from_counts_as_fractions() {
        let build = ahri();
        let stats = build.items.core[0].stats.expect("core stats");
        assert_eq!(stats.games, Some(11980));
        let rate = stats.win_rate.unwrap();
        assert!((rate - 0.5268).abs() < 0.001, "{rate}");
        assert!((0.0..=1.0).contains(&rate));
    }

    #[test]
    fn maps_runes_summoners_and_both_skill_orders() {
        let build = ahri();

        let page = &build.runes[0];
        assert_eq!(page.primary_style, Some(8100));
        assert_eq!(page.secondary_style, Some(8200));
        assert_eq!(page.primary, vec![8112, 8139, 8140, 8106]);
        assert_eq!(page.secondary, vec![8210, 8226]);
        assert_eq!(page.shards, vec![5005, 5008, 5001]);
        assert_eq!(page.label.as_deref(), Some("Domination"));

        assert_eq!(build.summoners[0].spells.len(), 2);
        assert_eq!(build.summoners[0].spells[0].id, 4);

        assert_eq!(build.skills.priority, vec![Skill::Q, Skill::W, Skill::E]);
        assert_eq!(build.skills.order.len(), 15);
        assert_eq!(build.skills.order[0], Skill::W);
    }

    /// The extra `counters_meta` field this response carries shifts every
    /// field after it; nothing may be read off by one.
    #[test]
    fn a_thin_sample_maps_without_shifting() {
        let payload = wire::parse(BRIAR).unwrap();
        let request = BuildRequest::new("Briar", Role::Utility).with_champion_id(233);
        let build = build_from_payload(&payload, &request, "OP.GG")
            .build()
            .expect("expected a build")
            .clone();

        assert_eq!(build.items.core[0].items[0].name.as_deref(), Some("Hubris"));
        assert_eq!(build.items.core[0].stats.unwrap().games, Some(23));
        assert_eq!(build.source.patch.as_deref(), Some("16.17"));
        assert_eq!(build.skills.priority, vec![Skill::W, Skill::Q, Skill::E]);
    }

    #[test]
    fn a_payload_with_no_items_is_no_data_not_an_error() {
        let payload = json!({ "data": { "summary": { "average_stats": { "play": 12 } } } });
        match build_from_payload(&payload, &request(), "OP.GG") {
            BuildLookup::NoData(no_data) => assert!(no_data.detail.contains("no build data")),
            BuildLookup::Found(_) => panic!("expected no data"),
        }
    }

    #[test]
    fn prose_answer_is_no_data() {
        let lookup = build_from_payload(&json!("No data found for Ahri TOP."), &request(), "OP.GG");
        match lookup {
            BuildLookup::NoData(no_data) => assert!(no_data.detail.contains("No data found")),
            BuildLookup::Found(_) => panic!("expected no data"),
        }
    }

    #[test]
    fn missing_optional_sections_do_not_fail_the_lookup() {
        let payload = json!({ "data": { "core_items": { "ids": [3118] } } });
        let build = build_from_payload(&payload, &request(), "OP.GG")
            .build()
            .expect("expected a build")
            .clone();

        assert_eq!(build.items.core[0].items[0].id, 3118);
        assert!(build.items.core[0].items[0].name.is_none());
        assert!(build.runes.is_empty());
        assert!(build.summoners.is_empty());
        assert!(build.skills.is_empty());
        assert!(build.stats.is_none());
        assert_eq!(build.champion.name, "Ahri");
    }
}

/// The matchup mapper, against a payload written by hand.
///
/// Hand-authored on purpose. `CLAUDE.md` forbids caching OP.GG data into the
/// repository, and a fixture is a cache that never expires. What these tests
/// need is the payload's *shape* and its two traps, both of which are cheaper
/// to state deliberately than to capture: sections that arrive as menus, and
/// a `summary` that is not about the matchup at all.
#[cfg(test)]
mod matchup_tests {
    use super::*;
    use crate::build_data::role::Role;
    use crate::build_data::schema::{LaneAdvantage, Skill};
    use serde_json::json;

    fn request() -> BuildRequest {
        BuildRequest::new("Ahri", Role::Middle)
            .with_champion_id(103)
            .with_opponent("Zed")
    }

    /// The shape the live tool answers in, reduced to what the mapper reads.
    ///
    /// The numbers are chosen so a mistake cannot pass: the summary's sample
    /// (181,664) and the counters' (6,431) are nothing like each other, and
    /// the head of every menu differs from its tail.
    fn payload() -> Value {
        json!({
            "position": "mid",
            "my_champion": "Ahri",
            "opponent_champion": "Zed",
            "data": {
                "summary": {
                    "average_stats": {
                        "play": 181664,
                        "win_rate": 0.509721,
                        "pick_rate": 0.0995705,
                        "ban_rate": 0.0309292
                    }
                },
                "core_items": [
                    { "ids": [3118, 4645, 3157],
                      "ids_names": ["Malignance", "Shadowflame", "Zhonya's Hourglass"],
                      "play": 530, "win": 284 },
                    { "ids": [3118, 3157, 4645],
                      "ids_names": ["Malignance", "Zhonya's Hourglass", "Shadowflame"],
                      "play": 325, "win": 165 }
                ],
                "boots": [
                    { "ids": [3020], "ids_names": ["Sorcerer's Shoes"], "play": 4311, "win": 2181 },
                    { "ids": [3158], "ids_names": ["Ionian Boots of Lucidity"], "play": 2159, "win": 1061 }
                ],
                "starter_items": [
                    { "ids": [1056, 2003, 2003],
                      "ids_names": ["Doran's Ring", "Health Potion", "Health Potion"],
                      "play": 7679, "win": 3837 },
                    { "ids": [1082, 2031], "ids_names": ["Dark Seal", "Refillable Potion"],
                      "play": 13, "win": 7 }
                ],
                "last_items": [
                    { "ids": [3118], "ids_names": ["Malignance"], "play": 4985, "win": 2571 },
                    { "ids": [3157], "ids_names": ["Zhonya's Hourglass"], "play": 4182, "win": 2213 },
                    { "ids": [3100], "ids_names": ["Lich Bane"], "play": 1907, "win": 1059 },
                    { "ids": [3089], "ids_names": ["Rabadon's Deathcap"], "play": 1721, "win": 821 }
                ],
                "runes": [
                    { "primary_page_id": 8100, "primary_page_name": "Domination",
                      "primary_rune_ids": [8112, 8139, 8140, 8106],
                      "secondary_page_id": 8200, "secondary_rune_ids": [8210, 8226],
                      "stat_mod_ids": [5005, 5008, 5001], "play": 3547, "win": 1758 },
                    { "primary_page_id": 8200, "primary_rune_ids": [8214],
                      "play": 400, "win": 200 }
                ],
                "summoner_spells": [
                    { "ids": [4, 14], "play": 4228, "win": 2164 },
                    { "ids": [4, 12], "play": 3099, "win": 1509 }
                ],
                "skills": [
                    { "order": ["W", "Q", "E", "Q", "Q", "R"], "play": 3354, "win": 1877 },
                    { "order": ["Q", "W", "E", "Q", "Q", "R"], "play": 404, "win": 237 }
                ],
                "skill_masteries": [
                    { "ids": ["Q", "W", "E"], "play": 4861, "win": 2763 },
                    { "ids": ["Q", "E", "W"], "play": 427, "win": 236 }
                ],
                "counters": [
                    { "champion_id": 50, "champion_name": "Swain", "play": 490, "win": 216 },
                    { "champion_id": 238, "champion_name": "Zed", "play": 6431, "win": 3222 },
                    { "champion_id": 134, "champion_name": "Syndra", "play": 7226, "win": 3574 }
                ],
                "trends": { "win": [{ "version": "16.17", "rate": 0.5095 }] },
                "opponent_champion_tip": "Do not use Charm [E] until Zed closes in.",
                "lane_advantage_champion": "Zed",
                "recommended_play_style": "even"
            }
        })
    }

    fn build() -> ChampionBuild {
        matchup_from_payload(&payload(), &request(), "OP.GG")
            .build()
            .expect("expected a build")
            .clone()
    }

    /// The whole point of the feature: the build is about this pairing, and
    /// says so.
    #[test]
    fn the_build_names_the_opponent_it_is_against() {
        let matchup = build().matchup.expect("a matchup build carries its matchup");

        assert_eq!(matchup.opponent.key, "Zed");
        // Proper case from the payload's echo, not the `ZED` we asked with.
        assert_eq!(matchup.opponent.name, "Zed");
        assert_eq!(matchup.lane_advantage, Some(LaneAdvantage::Theirs));
        assert_eq!(matchup.play_style.as_deref(), Some("even"));
        assert!(matchup.tip.unwrap().starts_with("Do not use Charm"));
    }

    /// The trap this mapper exists to avoid.
    ///
    /// `summary.average_stats` sits in the same object as the matchup
    /// sections and is not matchup data — it is the champion's whole record.
    /// Reading the headline stats from there would print 181,664 games and a
    /// 51% win rate under the words "vs Zed". The honest sample is the
    /// opponent's row in `counters`.
    #[test]
    fn the_headline_stats_are_the_matchup_not_the_champion() {
        let stats = build().stats.expect("the counters row is a sample");

        assert_eq!(stats.games, Some(6431), "this is the Ahri-vs-Zed sample");
        assert_ne!(stats.games, Some(181664), "that is Ahri against everybody");

        let win_rate = stats.win_rate.expect("a row with games has a win rate");
        assert!((win_rate - 3222.0 / 6431.0).abs() < 1e-9);
        // Nowhere near the summary's 0.5097, which is the point.
        assert!((win_rate - 0.509721).abs() > 0.005);
    }

    /// An opponent nobody has a row for gets no number rather than the
    /// champion's overall one. A confident figure about the wrong population
    /// is worse than an absent one.
    #[test]
    fn a_matchup_too_thin_to_have_been_counted_reports_no_sample() {
        let mut value = payload();
        value["data"]["counters"] = json!([
            { "champion_id": 50, "champion_name": "Swain", "play": 490, "win": 216 }
        ]);

        let build = matchup_from_payload(&value, &request(), "OP.GG")
            .build()
            .expect("the build is still there")
            .clone();

        assert!(build.stats.is_none(), "no row for Zed means no Zed number");
        // The build itself is unaffected: it is still the matchup's build.
        assert!(build.matchup.is_some());
        assert_eq!(build.items.core[0].items[0].id, 3118);
    }

    /// Every section arrives as a menu ordered by how often it was built, and
    /// the head is the answer. A mapper that read the array itself, or the
    /// last entry, would produce a build nobody plays.
    #[test]
    fn each_section_is_narrowed_to_its_most_played_option() {
        let build = build();

        assert_eq!(build.items.core.len(), 1, "one core path, not fifteen");
        assert_eq!(
            build.items.core[0].items.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![3118, 4645, 3157],
            "the 530-game path, not the 325-game one"
        );
        assert_eq!(build.items.boots[0].items[0].id, 3020);
        assert_eq!(build.items.starters[0].items[0].id, 1056);
        assert_eq!(build.runes.len(), 1);
        assert_eq!(build.runes[0].primary_style, Some(8100));
        assert_eq!(build.summoners[0].spells[0].id, 4);
        assert_eq!(build.skills.order.first(), Some(&Skill::W));
        assert_eq!(build.skills.priority, vec![Skill::Q, Skill::W, Skill::E]);
    }

    /// Names travel with ids here as everywhere else — it is what lets the UI
    /// write "Malignance" instead of "#3118".
    #[test]
    fn item_names_are_carried_next_to_their_ids() {
        let build = build();
        assert_eq!(build.items.core[0].items[0].name.as_deref(), Some("Malignance"));
        assert_eq!(build.items.boots[0].items[0].name.as_deref(), Some("Sorcerer's Shoes"));
    }

    /// `last_items` leads with the core over again — Malignance and Zhonya's
    /// are the first two entries and both finish the core path. A situational
    /// list that opens by telling you to build what you are already building
    /// is worth nothing, which is why the core is subtracted from it.
    #[test]
    fn the_alternatives_exclude_what_the_core_already_buys() {
        let situational: Vec<u32> = build()
            .items
            .situational
            .iter()
            .flat_map(|group| group.items.iter().map(|item| item.id))
            .collect();

        assert!(!situational.contains(&3118), "Malignance is the core's first item");
        assert!(!situational.contains(&3157), "Zhonya's finishes the core");
        assert_eq!(situational, vec![3100, 3089], "what is left is a real menu");
    }

    /// This tool takes no tier argument, so no tier was asked for and none
    /// may be reported. The ordinary lookup fills the field in from the
    /// config it queried with; there is nothing here to fill it in from.
    #[test]
    fn no_tier_is_claimed_for_a_question_that_could_not_name_one() {
        let build = build();
        assert_eq!(build.source.tier, None);
        assert_eq!(build.source.patch.as_deref(), Some("16.17"));
        assert_eq!(build.source.provider_label, "OP.GG");
    }

    /// The advantage field names a champion rather than a side, so it is
    /// compared against both seats — and a name matching neither is `Even`
    /// rather than a guess at which side it meant.
    #[test]
    fn the_lane_advantage_is_read_as_a_side_not_a_name() {
        let ours = {
            let mut value = payload();
            value["data"]["lane_advantage_champion"] = json!("Ahri");
            matchup_from_payload(&value, &request(), "OP.GG").build().unwrap().clone()
        };
        assert_eq!(
            ours.matchup.unwrap().lane_advantage,
            Some(LaneAdvantage::Ours)
        );

        let neither = {
            let mut value = payload();
            value["data"]["lane_advantage_champion"] = json!("Teemo");
            matchup_from_payload(&value, &request(), "OP.GG").build().unwrap().clone()
        };
        assert_eq!(
            neither.matchup.unwrap().lane_advantage,
            Some(LaneAdvantage::Even)
        );
    }

    /// A pairing the source has nothing for is an answer, not a failure — and
    /// it says who it could not answer about.
    #[test]
    fn a_pairing_with_no_items_is_no_data_naming_the_opponent() {
        let mut value = payload();
        value["data"]["core_items"] = json!([]);
        value["data"]["boots"] = json!([]);
        value["data"]["starter_items"] = json!([]);
        value["data"]["last_items"] = json!([]);

        match matchup_from_payload(&value, &request(), "OP.GG") {
            BuildLookup::NoData(no_data) => {
                assert!(no_data.detail.contains("Zed"), "{}", no_data.detail);
                assert!(no_data.detail.contains("Ahri"), "{}", no_data.detail);
            }
            BuildLookup::Found(_) => panic!("expected no data"),
        }
    }

    /// The endpoint answering in prose is how it says "nothing found", and it
    /// reads the same on this route as on the ordinary one.
    #[test]
    fn prose_is_an_answer_rather_than_a_parse_failure() {
        let payload = Value::String("No data found for this matchup.".to_string());
        match matchup_from_payload(&payload, &request(), "OP.GG") {
            BuildLookup::NoData(no_data) => {
                assert_eq!(no_data.detail, "No data found for this matchup.");
            }
            BuildLookup::Found(_) => panic!("expected no data"),
        }
    }

    /// The ordinary lookup must not start claiming matchups because this one
    /// exists. Nothing it reads is filtered to an opponent.
    #[test]
    fn the_ordinary_lookup_never_claims_a_matchup() {
        use crate::build_data::opgg::wire;
        let payload = wire::parse(include_str!("testdata/ahri_mid.txt")).unwrap();
        let build = build_from_payload(&payload, &BuildRequest::new("Ahri", Role::Middle), "OP.GG")
            .build()
            .expect("the fixture is a build")
            .clone();

        assert!(build.matchup.is_none());
    }
}
