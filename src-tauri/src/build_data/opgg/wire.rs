//! OP.GG's compact response format.
//!
//! The MCP endpoint does not answer with JSON. It answers with a
//! token-compressed encoding meant to be read by a language model: a header
//! declaring each class's field order, a blank line, then one positional
//! constructor call.
//!
//! ```text
//! class LolGetChampionAnalysis: champion,position,data
//! class Data: summary,core_items,boots,starter_items,last_items,...
//! class CoreItems: ids,ids_names,play,win
//!
//! LolGetChampionAnalysis("AHRI","MID",Data(Summary(...),CoreItems([3118,...],...)))
//! ```
//!
//! This turns that back into JSON so the rest of the provider can read it the
//! way it reads anything else. Two properties of the format make the header
//! load-bearing rather than decorative, and both are why nothing here may
//! hardcode a field order:
//!
//! - **Classes are structural, not nominal.** `CoreItems` is the class of
//!   `core_items`, `boots`, `starter_items` *and* each entry of `last_items`,
//!   because those four happen to share a shape. The constructor name says
//!   nothing about which field you are looking at; only its position in the
//!   parent does.
//! - **The field list changes between responses.** `Data` gains a
//!   `counters_meta` field when a champion's matchup sample is too thin, so
//!   two lookups a second apart can disagree about what the fifth field of
//!   `Data` is. The header in the same message is the only authority.
//!
//! Nothing is discarded: a class the header does not declare becomes a JSON
//! array of its arguments rather than an error, so an unannounced addition
//! costs one unreadable section instead of the whole lookup.

use std::collections::HashMap;

use serde_json::{Map, Value};

/// Parse one payload into JSON.
///
/// The error is a sentence for a log line — a payload we cannot read is a
/// protocol problem, and the caller turns it into one.
pub fn parse(text: &str) -> Result<Value, String> {
    let (classes, expression) = split(text)?;
    let mut parser = Parser {
        chars: expression.chars().collect(),
        at: 0,
        classes,
    };
    let value = parser.value()?;
    parser.skip_space();
    if parser.at < parser.chars.len() {
        return Err(format!(
            "trailing input after the payload at byte {}",
            parser.at
        ));
    }
    Ok(value)
}

/// Field order per class, and the expression that uses it.
fn split(text: &str) -> Result<(HashMap<String, Vec<String>>, String), String> {
    let mut classes: HashMap<String, Vec<String>> = HashMap::new();
    let mut expression = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match trimmed.strip_prefix("class ").and_then(|rest| rest.split_once(':')) {
            Some((name, fields)) => {
                classes.insert(
                    name.trim().to_string(),
                    fields
                        .split(',')
                        .map(|field| field.trim().to_string())
                        .filter(|field| !field.is_empty())
                        .collect(),
                );
            }
            // Everything that is not a class declaration is the payload. A
            // long one may wrap.
            None => expression.push_str(trimmed),
        }
    }

    if expression.is_empty() {
        return Err("the payload carried no value, only class declarations".to_string());
    }
    Ok((classes, expression))
}

struct Parser {
    chars: Vec<char>,
    at: usize,
    classes: HashMap<String, Vec<String>>,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.at += 1;
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), String> {
        if self.peek() == Some(expected) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!(
                "expected {expected:?} at position {} but found {:?}",
                self.at,
                self.peek()
            ))
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        self.skip_space();
        match self.peek() {
            Some('"') => self.string().map(Value::String),
            Some('[') => self.list(),
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            Some(c) if c.is_alphabetic() || c == '_' => self.named(),
            other => Err(format!("unexpected {other:?} at position {}", self.at)),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err("a string was never closed".to_string()),
                Some('"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some('\\') => {
                    self.at += 1;
                    let escaped = self.peek().ok_or("a string ended inside an escape")?;
                    out.push(match escaped {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        other => other,
                    });
                    self.at += 1;
                }
                Some(other) => {
                    out.push(other);
                    self.at += 1;
                }
            }
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.at;
        if self.peek() == Some('-') {
            self.at += 1;
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-')
        {
            self.at += 1;
        }
        let raw: String = self.chars[start..self.at].iter().collect();
        serde_json::from_str::<Value>(&raw).map_err(|error| format!("{raw:?}: {error}"))
    }

    fn list(&mut self) -> Result<Value, String> {
        self.expect('[')?;
        let mut items = Vec::new();
        loop {
            self.skip_space();
            if self.peek() == Some(']') {
                self.at += 1;
                return Ok(Value::Array(items));
            }
            items.push(self.value()?);
            self.skip_space();
            if self.peek() == Some(',') {
                self.at += 1;
            }
        }
    }

    /// An identifier: a constructor call, or one of the bare literals.
    fn named(&mut self) -> Result<Value, String> {
        let start = self.at;
        while matches!(self.peek(), Some(c) if c.is_alphanumeric() || c == '_') {
            self.at += 1;
        }
        let name: String = self.chars[start..self.at].iter().collect();

        if self.peek() != Some('(') {
            return Ok(match name.as_str() {
                "null" | "None" => Value::Null,
                "true" | "True" => Value::Bool(true),
                "false" | "False" => Value::Bool(false),
                _ => Value::String(name),
            });
        }

        self.expect('(')?;
        let mut arguments = Vec::new();
        loop {
            self.skip_space();
            if self.peek() == Some(')') {
                self.at += 1;
                break;
            }
            arguments.push(self.value()?);
            self.skip_space();
            if self.peek() == Some(',') {
                self.at += 1;
            }
        }

        Ok(self.shape(&name, arguments))
    }

    /// Name the arguments using the header's field order.
    ///
    /// A class we were never told about, or one that sent more arguments than
    /// it declared, keeps its data as an array. Losing the labels is
    /// recoverable; losing the values is not.
    fn shape(&self, class: &str, arguments: Vec<Value>) -> Value {
        let Some(fields) = self.classes.get(class) else {
            return Value::Array(arguments);
        };
        if arguments.len() > fields.len() {
            return Value::Array(arguments);
        }

        let mut object = Map::new();
        for (field, argument) in fields.iter().zip(arguments) {
            object.insert(field.clone(), argument);
        }
        Value::Object(object)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Ahri mid, captured from the live endpoint.
    const AHRI: &str = r#"class LolGetChampionAnalysis: champion,position,data
class Data: summary,core_items,boots,starter_items,last_items,summoner_spells,runes,skills,skill_masteries,trends
class Summary: average_stats
class AverageStats: play,win_rate,pick_rate,ban_rate,tier_data
class TierData: tier
class CoreItems: ids,ids_names,play,win
class SummonerSpells: ids,win,play
class Runes: primary_page_id,primary_page_name,primary_rune_ids,secondary_page_id,secondary_rune_ids,stat_mod_ids,play,win
class Skills: order,play,win
class SkillMasteries: ids
class Trends: win,pick
class Win: version

LolGetChampionAnalysis("AHRI","MID",Data(Summary(AverageStats(162598,0.51,0.1,0.03,TierData(2))),CoreItems([3118,4645,3157],["Malignance","Shadowflame","Zhonya's Hourglass"],11341,5972),CoreItems([3020],["Sorcerer's Shoes"],87760,45385),CoreItems([1056,2003,2003],["Doran's Ring","Health Potion","Health Potion"],149570,76280),[CoreItems([3118],["Malignance"],95908,49034),CoreItems([4645],["Shadowflame"],68889,36125)],SummonerSpells([4,14],40096,76906),Runes(8100,"Domination",[8112,8139,8140,8106],8200,[8210,8226],[5005,5008,5001],72933,36651),Skills(["W","Q","E","Q","Q","R","Q","W","Q","W","R","W","W","E","E"],57157,33189),SkillMasteries(["Q","W","E"]),Trends(Win("16.17"),Win("16.17"))))"#;

    #[test]
    fn reads_a_real_response() {
        let value = parse(AHRI).unwrap();

        assert_eq!(value["champion"], "AHRI");
        assert_eq!(value["position"], "MID");

        let data = &value["data"];
        assert_eq!(data["summary"]["average_stats"]["play"], 162598);
        assert_eq!(data["summary"]["average_stats"]["win_rate"], 0.51);
        assert_eq!(data["core_items"]["ids"], json!([3118, 4645, 3157]));
        assert_eq!(data["core_items"]["ids_names"][0], "Malignance");
        assert_eq!(data["core_items"]["play"], 11341);
        assert_eq!(data["core_items"]["win"], 5972);
        assert_eq!(data["runes"]["primary_page_name"], "Domination");
        assert_eq!(data["runes"]["stat_mod_ids"], json!([5005, 5008, 5001]));
        assert_eq!(data["skills"]["order"][0], "W");
        assert_eq!(data["skill_masteries"]["ids"], json!(["Q", "W", "E"]));
        assert_eq!(data["trends"]["win"]["version"], "16.17");
    }

    /// The same class serves four different fields. Only position tells them
    /// apart, which is the whole reason the header is read.
    #[test]
    fn the_same_class_name_lands_in_the_right_fields() {
        let data = &parse(AHRI).unwrap()["data"];
        assert_eq!(data["boots"]["ids"], json!([3020]));
        assert_eq!(data["starter_items"]["ids"], json!([1056, 2003, 2003]));
        assert_eq!(data["last_items"][0]["ids"], json!([3118]));
        assert_eq!(data["last_items"][1]["ids_names"][0], "Shadowflame");
    }

    /// `Data` gains `counters_meta` when the matchup sample is thin, so the
    /// field order is a property of the message, not of the format.
    #[test]
    fn a_field_list_that_grew_is_still_read_correctly() {
        let text = "class Root: a,b,extra\nclass Leaf: id\n\nRoot(Leaf(1),Leaf(2),\"late addition\")";
        let value = parse(text).unwrap();
        assert_eq!(value["a"]["id"], 1);
        assert_eq!(value["b"]["id"], 2);
        assert_eq!(value["extra"], "late addition");
    }

    #[test]
    fn a_class_the_header_never_declared_keeps_its_values() {
        let value = parse("class Root: a\n\nRoot(Mystery(1,\"two\"))").unwrap();
        assert_eq!(value["a"], json!([1, "two"]));
    }

    #[test]
    fn strings_carry_their_punctuation_and_escapes() {
        let value = parse(
            "class Root: text,name\n\nRoot(\"comma, paren ) bracket ] \\\"quoted\\\"\",\"Zhonya's Hourglass\")",
        )
        .unwrap();
        assert_eq!(value["text"], "comma, paren ) bracket ] \"quoted\"");
        assert_eq!(value["name"], "Zhonya's Hourglass");
    }

    #[test]
    fn missing_trailing_fields_are_simply_absent() {
        let value = parse("class Root: a,b,c\n\nRoot(1)").unwrap();
        assert_eq!(value["a"], 1);
        assert!(value.get("b").is_none());
    }

    #[test]
    fn literals_and_negative_numbers() {
        let value = parse("class Root: n,f,t,x\n\nRoot(-7,1.5,true,null)").unwrap();
        assert_eq!(value["n"], -7);
        assert_eq!(value["f"], 1.5);
        assert_eq!(value["t"], true);
        assert_eq!(value["x"], Value::Null);
    }

    #[test]
    fn malformed_input_is_an_error_not_a_panic() {
        for text in [
            "",
            "class Root: a",
            "class Root: a\n\nRoot(",
            "class Root: a\n\nRoot(\"unclosed)",
            "class Root: a\n\nRoot(1) trailing",
        ] {
            assert!(parse(text).is_err(), "accepted {text:?}");
        }
    }
}
