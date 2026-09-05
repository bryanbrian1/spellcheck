//! Live build data from the OP.GG MCP endpoint.
//!
//! **Per-request only.** OP.GG's dataset is theirs; nothing fetched here is
//! written to disk, committed, or redistributed. There is deliberately no
//! cache in this module — a lookup either hits the endpoint or it does not
//! happen. The distributable path is
//! [`RiotProvider`](crate::build_data::riot::RiotProvider).

pub mod map;
pub mod mcp;
pub mod wire;

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::error::ProviderError;
use super::role::Role;
use super::schema::{BuildLookup, BuildRequest};
use super::{validate_champion_key, BuildDataProvider};
use crate::build_data::schema::BuildLookup as Lookup;

use mcp::McpClient;

pub const PROVIDER_LABEL: &str = "OP.GG";
pub const DEFAULT_ENDPOINT: &str = "https://mcp-api.op.gg/mcp";
pub const DEFAULT_TOOL: &str = "lol_get_champion_analysis";
const DEFAULT_TIMEOUT_SECS: u64 = 10;
/// The tool requires a game mode and has no default. Ranked solo queue is
/// what champion select is usually feeding.
pub const DEFAULT_GAME_MODE: &str = "ranked";

/// Exactly the fields this provider maps, and no others.
///
/// `desired_output_fields` is required and closed: the endpoint checks every
/// name against the tool's field list, skips the ones it does not recognise,
/// and reports them back. Asking for everything would multiply the payload
/// for data nothing reads — and an array field needs `[]` before the dot,
/// which is why `last_items` is spelled differently from its neighbours.
const OUTPUT_FIELDS: &[&str] = &[
    "champion",
    "position",
    "data.summary.average_stats.play",
    "data.summary.average_stats.win_rate",
    "data.summary.average_stats.pick_rate",
    "data.summary.average_stats.ban_rate",
    "data.core_items.ids",
    "data.core_items.ids_names",
    "data.core_items.play",
    "data.core_items.win",
    "data.boots.ids",
    "data.boots.ids_names",
    "data.boots.play",
    "data.boots.win",
    "data.starter_items.ids",
    "data.starter_items.ids_names",
    "data.starter_items.play",
    "data.starter_items.win",
    // The three slots after the core, each a menu of options. NOT
    // `last_items`, which sounds like the same thing and is not: that is the
    // champion's most-built items overall, so its top entries are the core
    // items again — 130,000 games of Voltaic Cyclosword on Zed, whose core
    // starts with Voltaic Cyclosword. Showing that under "Situational" told
    // the player to build what they were already building, and hid the half
    // of the build that actually varies.
    "data.fourth_items[].ids",
    "data.fourth_items[].ids_names",
    "data.fourth_items[].play",
    "data.fourth_items[].win",
    "data.fifth_items[].ids",
    "data.fifth_items[].ids_names",
    "data.fifth_items[].play",
    "data.fifth_items[].win",
    "data.sixth_items[].ids",
    "data.sixth_items[].ids_names",
    "data.sixth_items[].play",
    "data.sixth_items[].win",
    "data.summoner_spells.ids",
    "data.summoner_spells.play",
    "data.summoner_spells.win",
    "data.runes.primary_page_id",
    "data.runes.primary_page_name",
    "data.runes.primary_rune_ids",
    "data.runes.secondary_page_id",
    "data.runes.secondary_rune_ids",
    "data.runes.stat_mod_ids",
    "data.runes.play",
    "data.runes.win",
    "data.skills.order",
    "data.skills.play",
    "data.skills.win",
    "data.skill_masteries.ids",
    "data.trends.win.version",
];

/// Short name used in error messages.
const PROVIDER: &str = "OP.GG";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OpggConfig {
    pub endpoint: String,
    /// MCP tool to call. Configurable so a renamed tool is a config change,
    /// not a release.
    pub tool: String,
    /// Which queue the numbers come from. Required by the tool:
    /// `ranked`, `flex`, `urf`, `aram` or `nexus_blitz`.
    pub game_mode: String,
    /// Optional rank filter. Omitted for the all-tier aggregate.
    pub tier: Option<String>,
    /// Champ select is short. A lookup that has not answered by now is not
    /// going to be useful.
    pub timeout_secs: u64,
    /// Attribution shown in the UI.
    pub label: String,
}

impl Default for OpggConfig {
    fn default() -> Self {
        OpggConfig {
            endpoint: DEFAULT_ENDPOINT.to_string(),
            tool: DEFAULT_TOOL.to_string(),
            game_mode: DEFAULT_GAME_MODE.to_string(),
            tier: None,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            label: PROVIDER_LABEL.to_string(),
        }
    }
}

#[derive(Debug)]
pub struct OpggProvider {
    config: OpggConfig,
    client: McpClient,
}

impl OpggProvider {
    pub fn new(config: OpggConfig) -> Result<OpggProvider, ProviderError> {
        if config.endpoint.trim().is_empty() {
            return Err(ProviderError::Config("opgg.endpoint is empty".to_string()));
        }

        let client = McpClient::new(
            config.endpoint.clone(),
            Duration::from_secs(config.timeout_secs.max(1)),
            PROVIDER,
        )?;

        Ok(OpggProvider { config, client })
    }

    /// Arguments for the analysis tool.
    ///
    /// Kept in one place: the tool's exact parameter names are the endpoint's
    /// to define, and [`Self::tool_catalogue`] reports them from the live
    /// server if they ever change. Four of them are required — the endpoint
    /// rejects the call outright without `game_mode` or
    /// `desired_output_fields`.
    fn arguments(&self, request: &BuildRequest) -> Value {
        let mut arguments = json!({
            "game_mode": self.config.game_mode,
            "champion": opgg_champion(&request.champion_key),
            "position": request.role.opgg_position(),
            "desired_output_fields": OUTPUT_FIELDS,
        });

        let object = arguments.as_object_mut().expect("json! built an object");
        if let Some(tier) = &self.config.tier {
            object.insert("tier".to_string(), json!(tier));
        }

        arguments
    }

    /// Which lane this champion is played in, and how often.
    ///
    /// A different question from a build, and a far smaller answer — a couple
    /// of hundred bytes against a couple of thousand.
    const POSITION_FIELDS: &[&str] = &[
        "data.summary.positions[].name",
        "data.summary.positions[].stats.play",
    ];

    /// One call, mapped through the same wire handling as a build.
    async fn call(&self, arguments: Value) -> Result<Value, ProviderError> {
        let payload = self.client.call_tool(&self.config.tool, arguments).await?;
        Ok(match payload.as_str() {
            Some(text) => wire::parse(text).unwrap_or_else(|_| payload.clone()),
            None => payload,
        })
    }

    /// The endpoint's tool list with input schemas. Not used in the lookup
    /// path — it exists so the argument names above can be checked against
    /// the live server.
    pub async fn tool_catalogue(&self) -> Result<Value, ProviderError> {
        self.client.list_tools().await
    }
}

/// A Data Dragon key in the spelling the tool documents: UPPER_SNAKE_CASE.
///
/// `Ahri` is `AHRI`, `MonkeyKing` is `MONKEY_KING`, `JarvanIV` is `JARVAN_IV` —
/// a run of capitals is one word, so the numeral does not become its own.
/// Keys are ASCII alphanumeric by the time they reach here, having been
/// through `validate_champion_key`.
fn opgg_champion(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 2);
    let mut previous_was_lower = false;

    for character in key.chars() {
        if character.is_ascii_uppercase() && previous_was_lower {
            out.push('_');
        }
        previous_was_lower = character.is_ascii_lowercase() || character.is_ascii_digit();
        out.push(character.to_ascii_uppercase());
    }
    out
}

#[async_trait]
impl BuildDataProvider for OpggProvider {
    fn label(&self) -> &str {
        &self.config.label
    }

    async fn fetch_build(&self, request: &BuildRequest) -> Result<BuildLookup, ProviderError> {
        // Validated even though this is not a filesystem path: the key goes
        // into a request we make on the user's behalf.
        validate_champion_key(&request.champion_key)?;

        // The endpoint answers in its own compact format rather than JSON.
        // Anything that does not parse is left as text, which the mapper
        // reads as the endpoint speaking prose — usually "nothing found".
        let payload = self.call(self.arguments(request)).await?;

        // Mapped and returned; never persisted.
        let mut lookup = map::build_from_payload(&payload, request, self.label());

        // The tier is a property of the question, not of the answer: the
        // payload has no field for it, so the only place that knows is the
        // configuration we asked with.
        if let Lookup::Found(build) = &mut lookup {
            build.source.tier = self.config.tier.clone();
        }

        Ok(lookup)
    }

    /// Ask the summary which lane this champion is actually played in.
    ///
    /// `data.summary.positions[]` is a property of the champion, not of the
    /// lane asked about: querying Yasuo as top still reports mid 59%, top
    /// 23%, adc 17%. So any valid lane will do as the question, and mid is
    /// used for no better reason than that it has to be something.
    ///
    /// The tool's own schema advertises `all` and `none` for this parameter
    /// and the server rejects both — "The selected position is invalid." —
    /// which is why this asks a real lane and reads the answer sideways
    /// rather than simply omitting one.
    async fn primary_role(&self, champion_key: &str) -> Result<Option<Role>, ProviderError> {
        validate_champion_key(champion_key)?;

        let payload = self
            .call(json!({
                "game_mode": self.config.game_mode,
                "champion": opgg_champion(champion_key),
                "position": Role::Middle.opgg_position(),
                "desired_output_fields": Self::POSITION_FIELDS,
            }))
            .await?;

        let positions = payload
            .pointer("/data/summary/positions")
            .and_then(Value::as_array);
        let Some(positions) = positions else {
            return Ok(None);
        };

        // Most played wins. The array arrives sorted that way, but ordering
        // is the endpoint's to change and the count is right here.
        let best = positions
            .iter()
            .filter_map(|entry| {
                let name = entry.get("name")?.as_str()?;
                let role = Role::parse_optional(name)?.ok()?;
                let play = entry.pointer("/stats/play")?.as_u64()?;
                Some((role, play))
            })
            .max_by_key(|(_, play)| *play);

        Ok(best.map(|(role, _)| role))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_data::role::Role;

    fn provider() -> OpggProvider {
        OpggProvider::new(OpggConfig::default()).unwrap()
    }

    #[test]
    fn defaults_point_at_the_documented_endpoint() {
        let provider = provider();
        assert_eq!(provider.client.endpoint(), DEFAULT_ENDPOINT);
        assert_eq!(provider.config.tool, DEFAULT_TOOL);
        assert_eq!(provider.label(), PROVIDER_LABEL);
    }

    /// Every one of these was wrong until the live endpoint was asked what it
    /// actually takes, and each error was silent in a different way: the two
    /// missing fields were a rejected request, the two casings were accepted
    /// arguments that matched nothing.
    #[test]
    fn sends_the_four_arguments_the_tool_requires() {
        let arguments = provider().arguments(&BuildRequest::new("Ahri", Role::Middle));

        assert_eq!(arguments["game_mode"], "ranked");
        assert_eq!(arguments["champion"], "AHRI");
        assert_eq!(arguments["position"], "mid");
        assert!(
            arguments["desired_output_fields"].as_array().is_some_and(|f| !f.is_empty()),
            "the tool rejects the call without this list"
        );

        let support = provider().arguments(&BuildRequest::new("Thresh", Role::Utility));
        assert_eq!(support["position"], "support");
    }

    /// An array field needs `[]` before the dot; without it the endpoint
    /// silently skips the field and the situational block goes missing.
    #[test]
    fn array_fields_are_spelled_as_arrays() {
        for slot in ["fourth_items", "fifth_items", "sixth_items"] {
            assert!(
                OUTPUT_FIELDS.contains(&&*format!("data.{slot}[].ids")),
                "{slot} is not being asked for at all"
            );
            assert!(
                !OUTPUT_FIELDS.iter().any(|f| f.starts_with(&format!("data.{slot}."))),
                "{slot} lost its brackets"
            );
        }
    }

    /// `last_items` reads like "the items you finish on" and is not: it is the
    /// champion's most-built items overall, so its top entries are the core
    /// items again. Asking for it put the core under "Situational" and left
    /// the back half of the build off the screen entirely.
    #[test]
    fn the_most_built_items_are_not_mistaken_for_the_late_ones() {
        assert!(
            !OUTPUT_FIELDS.iter().any(|field| field.contains("last_items")),
            "last_items is back, and situational will repeat the core again"
        );
    }

    #[test]
    fn champion_keys_become_upper_snake_case() {
        assert_eq!(opgg_champion("Ahri"), "AHRI");
        assert_eq!(opgg_champion("MonkeyKing"), "MONKEY_KING");
        assert_eq!(opgg_champion("TwistedFate"), "TWISTED_FATE");
        assert_eq!(opgg_champion("DrMundo"), "DR_MUNDO");
        assert_eq!(opgg_champion("KogMaw"), "KOG_MAW");
        // A run of capitals is one word: not JARVAN_I_V.
        assert_eq!(opgg_champion("JarvanIV"), "JARVAN_IV");
        assert_eq!(opgg_champion("KSante"), "KSANTE");
    }

    #[test]
    fn the_tier_filter_is_omitted_when_unset() {
        let arguments = provider().arguments(&BuildRequest::new("Ahri", Role::Middle));
        assert!(arguments.get("tier").is_none());

        let narrowed = OpggProvider::new(OpggConfig {
            tier: Some("diamond_plus".to_string()),
            ..OpggConfig::default()
        })
        .unwrap();
        let arguments = narrowed.arguments(&BuildRequest::new("Ahri", Role::Middle));
        assert_eq!(arguments["tier"], "diamond_plus");
    }

    #[test]
    fn the_game_mode_is_a_config_value() {
        let aram = OpggProvider::new(OpggConfig {
            game_mode: "aram".to_string(),
            ..OpggConfig::default()
        })
        .unwrap();
        assert_eq!(aram.arguments(&BuildRequest::new("Ahri", Role::Middle))["game_mode"], "aram");
    }

    #[test]
    fn rejects_an_empty_endpoint() {
        let error = OpggProvider::new(OpggConfig {
            endpoint: "  ".to_string(),
            ..OpggConfig::default()
        })
        .unwrap_err();
        assert!(matches!(error, ProviderError::Config(_)));
    }

    #[tokio::test]
    async fn rejects_a_bad_champion_key_before_any_request() {
        let error = provider()
            .fetch_build(&BuildRequest::new("../etc", Role::Middle))
            .await
            .unwrap_err();
        assert!(matches!(error, ProviderError::InvalidChampionKey(_)));
    }
}
