//! OP.GG payload → internal schema.
//!
//! The response shape is not something we control, so nothing here is a strict
//! struct: sections are located by name via
//! [`find_container`](crate::build_data::mapping::find_container) and read with
//! the shared field helpers. A new wrapper level or a renamed field costs one
//! optional value, not the whole lookup.

use serde_json::{json, Value};

use crate::build_data::mapping::{
    self, field, find_container, find_nested, item_groups_field, rune_pages_field, skill_plan,
    stats_from, summoner_sets_field,
};
use crate::build_data::schema::{
    BuildLookup, BuildRequest, ChampionBuild, ChampionRef, ItemPlan, SkillPlan, SourceInfo,
};

/// How deep to look for the analysis object before giving up.
const MAX_DEPTH: usize = 5;
/// Sections sit alongside `items` on the analysis object; a small budget here
/// keeps the search from wandering into per-build sub-objects.
const SIBLING_DEPTH: usize = 1;

const ITEMS_KEYS: &[&str] = &["items", "itemBuilds", "item_builds", "builds", "itemSets"];
const STARTER_KEYS: &[&str] = &["starters", "starterItems", "starter_items", "startItems", "start"];
const BOOTS_KEYS: &[&str] = &["boots", "bootItems", "boot_items", "shoes"];
const CORE_KEYS: &[&str] = &["core", "coreItems", "core_items", "mythic", "main", "recommended"];
const SITUATIONAL_KEYS: &[&str] = &["situational", "options", "lastItems", "last_items", "late", "optional"];
const RUNES_KEYS: &[&str] = &["runes", "runePages", "rune_pages", "perks", "runeBuilds"];
const SUMMONERS_KEYS: &[&str] = &["summonerSpells", "summoner_spells", "summoners", "spells"];
const SKILLS_KEYS: &[&str] = &["skills", "skillOrder", "skill_order", "skillTree", "skillBuilds"];
const PATCH_KEYS: &[&str] = &["patch", "version", "gameVersion", "game_version"];
const REGION_KEYS: &[&str] = &["region", "server"];
const TIER_KEYS: &[&str] = &["tier", "rank", "elo", "averageTier"];
const UPDATED_KEYS: &[&str] = &["updatedAt", "updated_at", "lastUpdated", "last_updated"];
const CHAMPION_NAME_KEYS: &[&str] = &["championName", "champion_name", "name"];
const CHAMPION_ID_KEYS: &[&str] = &["championId", "champion_id"];

/// Translate one `lol_get_champion_analysis` payload.
///
/// Off-meta champion-role pairs legitimately have no build, so an empty
/// payload maps to [`BuildLookup::NoData`], not an error.
pub fn build_from_payload(
    payload: &Value,
    request: &BuildRequest,
    provider_label: &str,
) -> BuildLookup {
    // A bare string means the server answered in prose rather than data —
    // in practice, "nothing found for this pair".
    if let Value::String(text) = payload {
        return BuildLookup::no_data(request, summarise(text));
    }

    // The object that holds `items` is the analysis itself. Anchoring here
    // keeps build-level stats from being read off a single item path.
    let analysis = find_container(payload, ITEMS_KEYS, MAX_DEPTH).unwrap_or(payload);

    let items = map_items(analysis);

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

    let runes = section(analysis, payload, RUNES_KEYS)
        .map(|found| rune_pages_field(&wrap(found), &["value"]))
        .unwrap_or_default();
    let summoners = section(analysis, payload, SUMMONERS_KEYS)
        .map(|found| summoner_sets_field(&wrap(found), &["value"]))
        .unwrap_or_default();
    let skills = section(analysis, payload, SKILLS_KEYS)
        .map(map_skills)
        .unwrap_or_default();

    let build = ChampionBuild {
        champion: ChampionRef {
            key: request.champion_key.clone(),
            // Only the analysis object itself, never a deep search: `name`
            // is a common key and an item's name must not become the
            // champion's.
            name: find_nested(analysis, CHAMPION_NAME_KEYS, 0)
                .and_then(|value| value.as_str())
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| request.display_name().to_string()),
            id: find_nested(analysis, CHAMPION_ID_KEYS, 0)
                .and_then(mapping::id_of)
                .or(request.champion_id),
        },
        role: request.role,
        source: SourceInfo {
            provider_label: provider_label.to_string(),
            patch: text_at(analysis, payload, PATCH_KEYS),
            region: text_at(analysis, payload, REGION_KEYS),
            tier: text_at(analysis, payload, TIER_KEYS),
            updated_at: text_at(analysis, payload, UPDATED_KEYS),
        },
        stats: stats_from(analysis),
        items,
        runes,
        summoners,
        skills,
    };

    BuildLookup::found(build)
}

/// Look on the analysis object first, then fall back to a wider search of the
/// whole payload for sources that hang metadata off the envelope.
fn section<'a>(analysis: &'a Value, payload: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    find_nested(analysis, keys, SIBLING_DEPTH).or_else(|| find_nested(payload, keys, MAX_DEPTH))
}

fn map_items(analysis: &Value) -> ItemPlan {
    let Some(items) = field(analysis, ITEMS_KEYS) else {
        return ItemPlan::default();
    };

    // A bare array under `items` is a list of builds with no categories.
    if items.is_array() {
        return ItemPlan {
            core: item_groups_field(&wrap(items), &["value"]),
            ..ItemPlan::default()
        };
    }

    let plan = ItemPlan {
        starters: item_groups_field(items, STARTER_KEYS),
        boots: item_groups_field(items, BOOTS_KEYS),
        core: item_groups_field(items, CORE_KEYS),
        situational: item_groups_field(items, SITUATIONAL_KEYS),
    };

    // None of the category names matched, but the object may hold ids
    // directly (`{"items": {"items": [...]}}`).
    if plan.is_empty() {
        return ItemPlan {
            core: item_groups_field(items, mapping::ITEM_LIST_KEYS),
            ..ItemPlan::default()
        };
    }

    plan
}

fn map_skills(found: &Value) -> SkillPlan {
    match found {
        // A bare list or string is the level-by-level order.
        Value::Array(_) | Value::String(_) => skill_plan(&json!({ "order": found.clone() })),
        _ => skill_plan(found),
    }
}

/// The field helpers read named fields off an object, so a located section
/// that is itself an array gets wrapped to be readable the same way.
fn wrap(value: &Value) -> Value {
    json!({ "value": value.clone() })
}

fn text_at(analysis: &Value, payload: &Value, keys: &[&str]) -> Option<String> {
    match section(analysis, payload, keys)? {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
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
    use crate::build_data::role::Role;
    use crate::build_data::schema::Skill;

    fn request() -> BuildRequest {
        BuildRequest::new("Ahri", Role::Middle).with_champion_id(103)
    }

    /// Shaped the way an MCP analysis response tends to arrive: a wrapper
    /// object, categorised items, rates as percentages.
    fn payload() -> Value {
        json!({
            "data": {
                "championName": "Ahri",
                "championId": 103,
                "patch": "14.18",
                "region": "GLOBAL",
                "tier": "EMERALD_PLUS",
                "games": 240000,
                "win_rate": 51.8,
                "pick_rate": 9.2,
                "items": {
                    "starterItems": [{ "items": [1056, 2003], "win_rate": 52.0, "games": 90000 }],
                    "boots": [{ "items": [3020] }],
                    "coreItems": [
                        { "items": [6655, 3020, 3089], "win_rate": 53.5, "games": 40000 },
                        { "items": [6653, 3020, 3089], "win_rate": 51.0, "games": 12000 }
                    ],
                    "lastItems": [{ "items": [3135] }]
                },
                "runes": [{
                    "primaryStyle": 8100,
                    "secondaryStyle": 8200,
                    "primary": [8112, 8139, 8138, 8106],
                    "secondary": [8226, 8210],
                    "shards": [5008, 5008, 5001],
                    "win_rate": 52.4
                }],
                "summonerSpells": [{ "spells": [4, 12], "win_rate": 53.0 }],
                "skills": { "priority": "Q>E>W", "order": ["Q", "E", "W", "Q"] }
            }
        })
    }

    #[test]
    fn maps_a_full_analysis() {
        let lookup = build_from_payload(&payload(), &request(), "OP.GG");
        let build = lookup.build().expect("expected a build");

        assert_eq!(build.champion.key, "Ahri");
        assert_eq!(build.champion.name, "Ahri");
        assert_eq!(build.champion.id, Some(103));
        assert_eq!(build.role, Role::Middle);
        assert_eq!(build.source.provider_label, "OP.GG");
        assert_eq!(build.source.patch.as_deref(), Some("14.18"));
        assert_eq!(build.source.tier.as_deref(), Some("EMERALD_PLUS"));

        let stats = build.stats.expect("expected build-level stats");
        assert_eq!(stats.games, Some(240000));
        assert_eq!(stats.win_rate, Some(0.518));
        assert_eq!(stats.pick_rate, Some(0.092));

        assert_eq!(build.items.starters.len(), 1);
        assert_eq!(build.items.boots[0].items[0].id, 3020);
        assert_eq!(build.items.core.len(), 2);
        assert_eq!(build.items.core[0].stats.unwrap().win_rate, Some(0.535));
        assert_eq!(build.items.situational[0].items[0].id, 3135);

        assert_eq!(build.runes[0].primary_style, Some(8100));
        assert_eq!(build.runes[0].shards.len(), 3);
        assert_eq!(build.summoners[0].spells.len(), 2);
        assert_eq!(build.skills.priority, vec![Skill::Q, Skill::E, Skill::W]);
        assert_eq!(build.skills.order.len(), 4);
    }

    #[test]
    fn build_stats_are_not_taken_from_one_item_path() {
        // No build-level sample; the only `games` in the payload belongs to an
        // item path and must not be promoted to the build.
        let payload = json!({
            "data": { "items": { "coreItems": [{ "items": [6655], "games": 40000 }] } }
        });
        let build = build_from_payload(&payload, &request(), "OP.GG");
        assert!(build.build().unwrap().stats.is_none());
    }

    #[test]
    fn rates_are_fractions_never_percentages() {
        let lookup = build_from_payload(&payload(), &request(), "OP.GG");
        let build = lookup.build().unwrap();
        for group in &build.items.core {
            if let Some(stats) = group.stats {
                let rate = stats.win_rate.unwrap();
                assert!((0.0..=1.0).contains(&rate), "rate out of range: {rate}");
            }
        }
        assert!(build.stats.unwrap().win_rate.unwrap() <= 1.0);
    }

    #[test]
    fn handles_a_flat_item_list() {
        let payload = json!({ "items": [[1056, 2003], [6655, 3020]] });
        let build = build_from_payload(&payload, &request(), "OP.GG");
        let build = build.build().unwrap();
        assert_eq!(build.items.core.len(), 2);
        assert!(build.items.starters.is_empty());
    }

    #[test]
    fn empty_payload_is_no_data_not_an_error() {
        let lookup = build_from_payload(&json!({}), &request(), "OP.GG");
        match lookup {
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
    fn survives_an_extra_wrapper_level() {
        let wrapped = json!({ "result": { "analysis": payload() } });
        let build = build_from_payload(&wrapped, &request(), "OP.GG");
        let build = build.build().expect("expected a build");
        assert_eq!(build.items.core.len(), 2);
        assert_eq!(build.source.patch.as_deref(), Some("14.18"));
    }

    #[test]
    fn missing_optional_sections_do_not_fail_the_lookup() {
        let payload = json!({ "items": { "coreItems": [{ "items": [6655] }] } });
        let build = build_from_payload(&payload, &request(), "OP.GG");
        let build = build.build().unwrap();
        assert!(build.runes.is_empty());
        assert!(build.summoners.is_empty());
        assert!(build.skills.is_empty());
        // Falls back to what the request already knew.
        assert_eq!(build.champion.name, "Ahri");
        assert_eq!(build.champion.id, Some(103));
    }
}
