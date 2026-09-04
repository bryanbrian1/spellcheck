//! Role normalisation.
//!
//! Every source spells the five roles differently: the LCU sends
//! `assignedPosition` values, OP.GG uses its own labels, our crawled files use
//! the LCU spelling. `Role` is the single internal spelling; each provider is
//! responsible only for converting to and from its own vocabulary.

use serde::{Deserialize, Serialize};
use std::fmt;

use super::error::ProviderError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Top,
    Jungle,
    Middle,
    Bottom,
    Utility,
}

impl Role {
    pub const ALL: [Role; 5] = [
        Role::Top,
        Role::Jungle,
        Role::Middle,
        Role::Bottom,
        Role::Utility,
    ];

    /// The canonical internal spelling, which is also the LCU's
    /// `assignedPosition` value and the `{role}.json` file stem.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Top => "top",
            Role::Jungle => "jungle",
            Role::Middle => "middle",
            Role::Bottom => "bottom",
            Role::Utility => "utility",
        }
    }

    /// Filename stem under `data/builds/{Champion}/`.
    pub fn file_stem(self) -> &'static str {
        self.as_str()
    }

    /// OP.GG's position vocabulary, which is lowercase and case-sensitive:
    /// the tool schema's enum is `all`, `none`, `top`, `mid`, `jungle`,
    /// `adc`, `support`.
    pub fn opgg_position(self) -> &'static str {
        match self {
            Role::Top => "top",
            Role::Jungle => "jungle",
            Role::Middle => "mid",
            Role::Bottom => "adc",
            Role::Utility => "support",
        }
    }

    /// Accepts any spelling we have seen from a data source: the LCU's
    /// `assignedPosition`, OP.GG's labels, and the common community names.
    /// Empty / `NONE` (an unassigned slot in blind pick) is not a role and
    /// returns `None` rather than an error.
    pub fn parse_optional(raw: &str) -> Option<Result<Role, ProviderError>> {
        let key = raw.trim().to_ascii_lowercase();
        if key.is_empty() || key == "none" || key == "unselected" {
            return None;
        }
        Some(match key.as_str() {
            "top" | "toplane" | "t" => Ok(Role::Top),
            "jungle" | "jgl" | "jg" | "j" => Ok(Role::Jungle),
            "middle" | "mid" | "midlane" | "m" => Ok(Role::Middle),
            "bottom" | "bot" | "adc" | "carry" | "botlane" | "duo_carry" => Ok(Role::Bottom),
            "utility" | "support" | "supp" | "sup" | "duo_support" => Ok(Role::Utility),
            _ => Err(ProviderError::UnknownRole(raw.to_string())),
        })
    }

    /// Strict parse: an unassigned position is an error here.
    pub fn parse(raw: &str) -> Result<Role, ProviderError> {
        Role::parse_optional(raw).unwrap_or_else(|| Err(ProviderError::UnknownRole(raw.to_string())))
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Role {
    type Err = ProviderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Role::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_source_vocabulary() {
        assert_eq!(Role::parse("MIDDLE").unwrap(), Role::Middle);
        assert_eq!(Role::parse("mid").unwrap(), Role::Middle);
        assert_eq!(Role::parse("ADC").unwrap(), Role::Bottom);
        assert_eq!(Role::parse(" utility ").unwrap(), Role::Utility);
        assert_eq!(Role::parse("SUPPORT").unwrap(), Role::Utility);
    }

    #[test]
    fn unassigned_position_is_not_an_error() {
        assert!(Role::parse_optional("").is_none());
        assert!(Role::parse_optional("NONE").is_none());
        assert!(Role::parse_optional("banana").unwrap().is_err());
    }

    #[test]
    fn round_trips_through_serde() {
        for role in Role::ALL {
            let json = serde_json::to_string(&role).unwrap();
            assert_eq!(json, format!("\"{}\"", role.as_str()));
            let back: Role = serde_json::from_str(&json).unwrap();
            assert_eq!(back, role);
        }
    }
}
