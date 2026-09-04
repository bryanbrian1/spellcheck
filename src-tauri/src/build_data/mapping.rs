//! Shared translation helpers used to turn a provider payload into the
//! internal schema.
//!
//! Remote build data is loosely shaped and changes without notice, so mapping
//! is done by *asking for a field under any of the names a source might use*
//! rather than by deriving a strict struct per provider. A shape change then
//! degrades to a missing optional field instead of failing the whole lookup.
//!
//! Adding a third provider means writing a `map.rs` that calls these helpers
//! with that source's key names — no new schema types, no UI change.

use serde_json::Value;

use super::schema::{
    BuildStats, ItemGroup, ItemRef, RunePage, Skill, SkillPlan, SummonerSet, SummonerSpell,
};

/// First present, non-null field among `keys`.
pub fn field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    let object = value.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key))
        .filter(|found| !found.is_null())
}

pub fn str_field(value: &Value, keys: &[&str]) -> Option<String> {
    let found = field(value, keys)?;
    match found {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

pub fn f64_field(value: &Value, keys: &[&str]) -> Option<f64> {
    as_f64(field(value, keys)?)
}

pub fn u64_field(value: &Value, keys: &[&str]) -> Option<u64> {
    let number = as_f64(field(value, keys)?)?;
    if number.is_finite() && number >= 0.0 {
        Some(number.round() as u64)
    } else {
        None
    }
}

pub fn u32_field(value: &Value, keys: &[&str]) -> Option<u32> {
    u64_field(value, keys).and_then(|number| u32::try_from(number).ok())
}

pub fn array_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Vec<Value>> {
    field(value, keys)?.as_array()
}

/// Numbers sometimes arrive as strings (`"52.4"`), so accept both.
pub fn as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().trim_end_matches('%').parse().ok(),
        _ => None,
    }
}

/// Normalise a rate to a `0.0..=1.0` fraction.
///
/// Sources disagree on whether a win rate is `0.524` or `52.4`. Anything above
/// `1.0` is read as a percentage. A literal 100% is therefore indistinguishable
/// from `1.0` — both land on `1.0`, which is the same answer either way.
pub fn as_rate(raw: f64) -> Option<f64> {
    if !raw.is_finite() || raw < 0.0 {
        return None;
    }
    let fraction = if raw > 1.0 { raw / 100.0 } else { raw };
    if fraction > 1.0 {
        None
    } else {
        Some(fraction)
    }
}

pub fn rate_field(value: &Value, keys: &[&str]) -> Option<f64> {
    as_rate(f64_field(value, keys)?)
}

/// Key names every source we have met so far uses for sample statistics.
/// A provider with different names should call the field helpers directly.
pub const GAMES_KEYS: &[&str] = &["games", "play", "plays", "gameCount", "game_count", "count"];
pub const WIN_RATE_KEYS: &[&str] = &["winRate", "win_rate", "winrate", "wr"];
pub const PICK_RATE_KEYS: &[&str] = &["pickRate", "pick_rate", "pickrate"];
pub const BAN_RATE_KEYS: &[&str] = &["banRate", "ban_rate", "banrate"];

/// Pull a sample out of `value`, deriving the win rate from raw win/loss
/// counts when no rate field is present.
pub fn stats_from(value: &Value) -> Option<BuildStats> {
    let games = u64_field(value, GAMES_KEYS);
    let wins = u64_field(value, &["win", "wins", "winCount", "win_count"]);
    let losses = u64_field(value, &["lose", "loss", "losses", "loseCount", "lose_count"]);

    let games = games.or(match (wins, losses) {
        (Some(win), Some(loss)) => Some(win + loss),
        _ => None,
    });

    let win_rate = rate_field(value, WIN_RATE_KEYS).or_else(|| match (wins, games) {
        (Some(win), Some(total)) if total > 0 => Some(win as f64 / total as f64),
        _ => None,
    });

    BuildStats {
        games,
        win_rate,
        pick_rate: rate_field(value, PICK_RATE_KEYS),
        ban_rate: rate_field(value, BAN_RATE_KEYS),
    }
    .non_empty()
}

const ID_KEYS: &[&str] = &["id", "itemId", "item_id", "perkId", "perk_id", "spellId", "spell_id"];
const NAME_KEYS: &[&str] = &["name", "label", "title"];

/// A single numeric id, whether it arrived bare (`3157`), as a numeric string
/// (`"3157"`), or wrapped in an object (`{"id": 3157, ...}`).
pub fn id_of(value: &Value) -> Option<u32> {
    match value {
        Value::Number(_) | Value::String(_) => {
            as_f64(value).and_then(|number| u32::try_from(number.round() as i64).ok())
        }
        Value::Object(_) => u32_field(value, ID_KEYS),
        _ => None,
    }
}

/// Ids out of an array, silently dropping entries we cannot read — a single
/// unrecognised entry should not cost the user the whole build.
pub fn ids(values: &[Value]) -> Vec<u32> {
    values.iter().filter_map(id_of).collect()
}

pub fn ids_field(value: &Value, keys: &[&str]) -> Vec<u32> {
    array_field(value, keys).map(|array| ids(array)).unwrap_or_default()
}

pub fn item_ref(value: &Value) -> Option<ItemRef> {
    Some(ItemRef {
        id: id_of(value)?,
        name: value.as_object().and_then(|_| str_field(value, NAME_KEYS)),
    })
}

/// One item option out of `value`, which may be a bare id, an array of ids, or
/// an object holding an id list plus its own sample stats.
pub fn item_group(value: &Value, list_keys: &[&str]) -> Option<ItemGroup> {
    let items: Vec<ItemRef> = match value {
        Value::Array(entries) => entries.iter().filter_map(item_ref).collect(),
        Value::Number(_) | Value::String(_) => item_ref(value).into_iter().collect(),
        Value::Object(_) => match array_field(value, list_keys) {
            Some(entries) => entries.iter().filter_map(item_ref).collect(),
            // An object with no id list may still *be* a single item.
            None => item_ref(value).into_iter().collect(),
        },
        _ => Vec::new(),
    };

    if items.is_empty() {
        return None;
    }

    Some(ItemGroup {
        items,
        stats: value.as_object().and_then(|_| stats_from(value)),
        label: value.as_object().and_then(|_| str_field(value, NAME_KEYS)),
    })
}

pub const ITEM_LIST_KEYS: &[&str] = &["items", "itemIds", "item_ids", "build", "ids"];

/// Item options out of an array of entries.
pub fn item_groups(values: &[Value]) -> Vec<ItemGroup> {
    values
        .iter()
        .filter_map(|entry| item_group(entry, ITEM_LIST_KEYS))
        .collect()
}

pub fn item_groups_field(value: &Value, keys: &[&str]) -> Vec<ItemGroup> {
    match field(value, keys) {
        Some(Value::Array(entries)) => item_groups(entries),
        Some(single) => item_group(single, ITEM_LIST_KEYS).into_iter().collect(),
        None => Vec::new(),
    }
}

pub fn summoner_set(value: &Value) -> Option<SummonerSet> {
    let spells: Vec<SummonerSpell> = match value {
        Value::Array(entries) => entries
            .iter()
            .filter_map(|entry| {
                Some(SummonerSpell {
                    id: id_of(entry)?,
                    name: entry.as_object().and_then(|_| str_field(entry, NAME_KEYS)),
                })
            })
            .collect(),
        Value::Object(_) => {
            let ids = ids_field(value, &["ids", "spells", "spellIds", "spell_ids", "summoners"]);
            ids.into_iter()
                .map(|id| SummonerSpell { id, name: None })
                .collect()
        }
        _ => Vec::new(),
    };

    if spells.is_empty() {
        return None;
    }

    Some(SummonerSet {
        spells,
        stats: value.as_object().and_then(|_| stats_from(value)),
    })
}

pub fn summoner_sets_field(value: &Value, keys: &[&str]) -> Vec<SummonerSet> {
    match field(value, keys) {
        Some(Value::Array(entries)) => {
            // Either a list of pairs, or one flat pair like [4, 14].
            let nested: Vec<SummonerSet> = entries.iter().filter_map(summoner_set).collect();
            if nested.is_empty() {
                summoner_set(&Value::Array(entries.clone()))
                    .into_iter()
                    .collect()
            } else {
                nested
            }
        }
        Some(single) => summoner_set(single).into_iter().collect(),
        None => Vec::new(),
    }
}

pub fn rune_page(value: &Value) -> Option<RunePage> {
    let page = RunePage {
        primary_style: u32_field(value, &["primaryStyle", "primary_style", "primaryStyleId", "primaryPageId"]),
        secondary_style: u32_field(value, &["secondaryStyle", "secondary_style", "secondaryStyleId", "subStyleId", "subPageId"]),
        primary: ids_field(value, &["primary", "primaryPerks", "primary_perks", "primaryRunes"]),
        secondary: ids_field(value, &["secondary", "secondaryPerks", "secondary_perks", "secondaryRunes", "sub"]),
        shards: ids_field(value, &["shards", "statPerks", "stat_perks", "statMods", "fragments"]),
        stats: value.as_object().and_then(|_| stats_from(value)),
        label: value.as_object().and_then(|_| str_field(value, NAME_KEYS)),
    };

    // Some sources give one flat perk list instead of split trees. Keystone
    // first, so the head of the list still reads correctly in the UI.
    if page.is_empty() {
        let flat = ids_field(value, &["perks", "perkIds", "runes", "ids"]);
        if flat.is_empty() {
            return None;
        }
        return Some(RunePage {
            primary: flat,
            ..page
        });
    }

    Some(page)
}

pub fn rune_pages_field(value: &Value, keys: &[&str]) -> Vec<RunePage> {
    match field(value, keys) {
        Some(Value::Array(entries)) => entries.iter().filter_map(rune_page).collect(),
        Some(single) => rune_page(single).into_iter().collect(),
        None => Vec::new(),
    }
}

fn skills_from_value(value: &Value) -> Vec<Skill> {
    match value {
        Value::Array(entries) => entries
            .iter()
            .filter_map(|entry| match entry {
                Value::String(text) => Skill::parse(text),
                Value::Number(_) => as_f64(entry).and_then(|n| Skill::parse(&n.to_string())),
                _ => None,
            })
            .collect(),
        // "QEWQ" or "Q>E>W"
        Value::String(text) => text
            .split(|c: char| !c.is_ascii_alphanumeric())
            .flat_map(|part| {
                if part.len() > 1 {
                    part.chars().map(|c| c.to_string()).collect::<Vec<_>>()
                } else {
                    vec![part.to_string()]
                }
            })
            .filter_map(|part| Skill::parse(&part))
            .collect(),
        _ => Vec::new(),
    }
}

pub const SKILL_PRIORITY_KEYS: &[&str] = &["priority", "skillPriority", "skill_priority", "masterOrder", "maxOrder"];
pub const SKILL_ORDER_KEYS: &[&str] = &["order", "skillOrder", "skill_order", "levelOrder", "sequence"];

pub fn skill_plan(value: &Value) -> SkillPlan {
    SkillPlan {
        priority: field(value, SKILL_PRIORITY_KEYS)
            .map(skills_from_value)
            .unwrap_or_default(),
        order: field(value, SKILL_ORDER_KEYS)
            .map(skills_from_value)
            .unwrap_or_default(),
    }
}

/// Breadth-first search for the object that holds any of `keys`.
///
/// Sources wrap the same payload differently — `{"data": {...}}`,
/// `{"result": {"analysis": {...}}}` — and the nesting changes without notice.
/// Searching by key name instead of by path means a new wrapper level does not
/// break the mapping. Breadth-first, so the shallowest match wins.
///
/// Returning the *container* rather than the value lets a caller read a
/// section's siblings too: locating `items` also locates the object whose
/// win rate belongs to the build as a whole, rather than to one item path.
pub fn find_container<'a>(root: &'a Value, keys: &[&str], max_depth: usize) -> Option<&'a Value> {
    let mut frontier = vec![root];

    for _ in 0..=max_depth {
        if frontier.is_empty() {
            return None;
        }

        for value in frontier.iter().copied() {
            if field(value, keys).is_some() {
                return Some(value);
            }
        }

        let mut next = Vec::new();
        for value in frontier {
            match value {
                Value::Object(map) => next.extend(map.values()),
                Value::Array(entries) => next.extend(entries.iter()),
                _ => {}
            }
        }
        frontier = next;
    }

    None
}

/// The value stored under any of `keys`, wherever it is nested.
pub fn find_nested<'a>(root: &'a Value, keys: &[&str], max_depth: usize) -> Option<&'a Value> {
    field(find_container(root, keys, max_depth)?, keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_a_field_under_any_alias() {
        let value = json!({ "win_rate": 52.4 });
        assert_eq!(rate_field(&value, WIN_RATE_KEYS), Some(0.524));
        assert_eq!(rate_field(&json!({ "winRate": 0.524 }), WIN_RATE_KEYS), Some(0.524));
        assert_eq!(rate_field(&json!({ "wr": "52.4%" }), WIN_RATE_KEYS), Some(0.524));
        assert_eq!(rate_field(&json!({ "other": 1 }), WIN_RATE_KEYS), None);
    }

    #[test]
    fn null_fields_read_as_absent() {
        assert_eq!(str_field(&json!({ "name": null }), NAME_KEYS), None);
        assert_eq!(str_field(&json!({ "name": "  " }), NAME_KEYS), None);
    }

    #[test]
    fn derives_win_rate_from_counts() {
        let stats = stats_from(&json!({ "win": 60, "lose": 40 })).unwrap();
        assert_eq!(stats.games, Some(100));
        assert_eq!(stats.win_rate, Some(0.6));
    }

    #[test]
    fn rejects_impossible_rates() {
        assert_eq!(as_rate(-1.0), None);
        assert_eq!(as_rate(101.0), None);
        assert_eq!(as_rate(1.0), Some(1.0));
    }

    #[test]
    fn reads_ids_in_every_shape() {
        assert_eq!(id_of(&json!(3157)), Some(3157));
        assert_eq!(id_of(&json!("3157")), Some(3157));
        assert_eq!(id_of(&json!({ "itemId": 3157 })), Some(3157));
        assert_eq!(id_of(&json!(true)), None);
        assert_eq!(ids(&[json!(1), json!("nope"), json!(3)]), vec![1, 3]);
    }

    #[test]
    fn builds_item_groups_from_mixed_shapes() {
        let groups = item_groups(&[
            json!([3157, 3089]),
            json!({ "items": [{ "id": 6655, "name": "Ludens" }], "win_rate": 53.0, "games": 10 }),
            json!("garbage"),
        ]);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].items.len(), 2);
        assert_eq!(groups[1].items[0].name.as_deref(), Some("Ludens"));
        assert_eq!(groups[1].stats.unwrap().win_rate, Some(0.53));
    }

    #[test]
    fn reads_summoners_flat_or_nested() {
        let flat = summoner_sets_field(&json!({ "spells": [4, 14] }), &["spells"]);
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].spells.len(), 2);

        let nested = summoner_sets_field(&json!({ "spells": [[4, 14], [4, 12]] }), &["spells"]);
        assert_eq!(nested.len(), 2);
    }

    #[test]
    fn falls_back_to_a_flat_perk_list() {
        let page = rune_page(&json!({ "perks": [8112, 8143] })).unwrap();
        assert_eq!(page.primary, vec![8112, 8143]);
    }

    #[test]
    fn finds_a_section_through_unknown_wrappers() {
        let payload = json!({ "data": { "analysis": { "runes": [{ "perks": [1, 2] }] } } });
        let found = find_nested(&payload, &["runes"], 4).unwrap();
        assert!(found.is_array());
        assert!(find_nested(&payload, &["runes"], 1).is_none());
        assert!(find_nested(&payload, &["nothing"], 6).is_none());
    }

    #[test]
    fn shallowest_match_wins() {
        let payload = json!({ "patch": "14.18", "meta": { "patch": "13.1" } });
        assert_eq!(find_nested(&payload, &["patch"], 4).unwrap(), "14.18");
    }

    #[test]
    fn container_exposes_a_sections_siblings() {
        let payload = json!({ "data": { "items": [1], "win_rate": 52.0 } });
        let container = find_container(&payload, &["items"], 4).unwrap();
        assert_eq!(rate_field(container, WIN_RATE_KEYS), Some(0.52));
    }

    #[test]
    fn parses_skill_orders_in_prose_and_arrays() {
        let plan = skill_plan(&json!({ "priority": "Q>E>W", "order": ["Q", "E", "Q", "W"] }));
        assert_eq!(plan.priority, vec![Skill::Q, Skill::E, Skill::W]);
        assert_eq!(plan.order.len(), 4);

        let packed = skill_plan(&json!({ "skillPriority": "QEW" }));
        assert_eq!(packed.priority, vec![Skill::Q, Skill::E, Skill::W]);
    }
}
