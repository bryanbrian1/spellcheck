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
    BuildLookup, BuildRequest, ChampionBuild, ChampionRef, ItemGroup, ItemPlan, ItemRef, RunePage,
    SkillPlan, SourceInfo, SummonerSet, SummonerSpell,
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
        items,
        runes: runes(data.get("runes")).into_iter().collect(),
        summoners: summoners(data.get("summoner_spells")).into_iter().collect(),
        skills: skills(data),
    };

    BuildLookup::found(build)
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
fn patch(data: &Value) -> Option<String> {
    let trends = data.get("trends")?;
    for key in ["win", "pick", "ban"] {
        if let Some(version) = trends.get(key).and_then(|trend| str_field(trend, &["version"])) {
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
