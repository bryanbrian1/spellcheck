//! The champ select session, reduced to the two facts we act on.
//!
//! The client's session payload is large and changes shape between patches,
//! so nothing here demands more than it needs: the local player's cell id,
//! and the entry in `myTeam` that matches it. Everything else — bans, the
//! action list, the other nine players — is read past. A field we do not read
//! cannot break us when Riot renames it.

use serde::{Deserialize, Serialize};

use crate::build_data::Role;

/// The client's own path for this resource. Both the REST read and the
/// WebSocket event carry it, which is how the two agree on what changed.
pub const CHAMP_SELECT_SESSION_URI: &str = "/lol-champ-select/v1/session";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChampSelectSession {
    /// Which seat is ours. The same list is sent to all ten players, so this
    /// is the only thing that says which row is the user.
    pub local_player_cell_id: i64,
    pub my_team: Vec<TeamMember>,
    pub timer: Timer,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TeamMember {
    pub cell_id: i64,
    /// Zero until the pick completes. Hovering a champion sets
    /// `championPickIntent` instead, which we deliberately ignore: the app
    /// answers for what you locked, not what you are considering.
    pub champion_id: u32,
    /// `top`, `jungle`, `middle`, `bottom`, `utility` — or empty in the
    /// queues that assign no position.
    pub assigned_position: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Timer {
    /// `PLANNING`, `BAN_PICK`, `FINALIZATION`, `GAME_STARTING`.
    pub phase: String,
}

/// A champion locked in a seat that has a role. This is the whole output of
/// the LCU layer: it is exactly the pair
/// [`BuildService::build_for`](crate::BuildService::build_for) takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub champion_id: u32,
    /// Kept as the client sent it. `Role::parse` already speaks this
    /// vocabulary, so no translation layer stands between the client and a
    /// lookup.
    pub assigned_position: String,
}

impl Selection {
    pub fn role(&self) -> Option<Role> {
        Role::parse_optional(&self.assigned_position).and_then(Result::ok)
    }
}

impl ChampSelectSession {
    /// Parse a session payload. Unknown fields are ignored, and a missing
    /// field falls back to its default rather than failing the whole read.
    pub fn from_json(value: &serde_json::Value) -> Result<ChampSelectSession, super::LcuError> {
        serde_json::from_value(value.clone())
            .map_err(|error| super::LcuError::unexpected("the champ select session", error))
    }

    /// Our own row in `myTeam`, if the client has sent one yet.
    pub fn local_player(&self) -> Option<&TeamMember> {
        self.my_team
            .iter()
            .find(|member| member.cell_id == self.local_player_cell_id)
    }

    /// What the user locked, once they have locked something.
    ///
    /// `None` covers every ordinary in-between state: the session exists but
    /// our row has not arrived, the user is still hovering, or the queue
    /// assigned no position and there is no role to look a build up for.
    pub fn selection(&self) -> Option<Selection> {
        let member = self.local_player()?;
        if member.champion_id == 0 {
            return None;
        }

        let selection = Selection {
            champion_id: member.champion_id,
            assigned_position: member.assigned_position.clone(),
        };
        selection.role().map(|_| selection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session(local_cell: i64, team: serde_json::Value) -> ChampSelectSession {
        ChampSelectSession::from_json(&json!({
            "localPlayerCellId": local_cell,
            "myTeam": team,
            "timer": { "phase": "BAN_PICK" },
            "bans": { "myTeamBans": [] },
            "actions": [[{ "id": 1, "type": "pick" }]],
        }))
        .unwrap()
    }

    #[test]
    fn finds_our_own_row_among_five() {
        let parsed = session(
            2,
            json!([
                { "cellId": 0, "championId": 266, "assignedPosition": "top" },
                { "cellId": 1, "championId": 64, "assignedPosition": "jungle" },
                { "cellId": 2, "championId": 103, "assignedPosition": "middle" },
                { "cellId": 3, "championId": 22, "assignedPosition": "bottom" },
                { "cellId": 4, "championId": 412, "assignedPosition": "utility" },
            ]),
        );

        let selection = parsed.selection().unwrap();
        assert_eq!(selection.champion_id, 103);
        assert_eq!(selection.assigned_position, "middle");
        assert_eq!(selection.role(), Some(Role::Middle));
    }

    #[test]
    fn hovering_is_not_locking() {
        let parsed = ChampSelectSession::from_json(&json!({
            "localPlayerCellId": 0,
            "myTeam": [
                { "cellId": 0, "championId": 0, "championPickIntent": 103, "assignedPosition": "middle" },
            ],
        }))
        .unwrap();

        assert!(parsed.selection().is_none());
    }

    #[test]
    fn a_queue_without_assigned_positions_yields_nothing_to_look_up() {
        let parsed = session(
            0,
            json!([{ "cellId": 0, "championId": 103, "assignedPosition": "" }]),
        );

        assert!(parsed.local_player().is_some());
        assert!(parsed.selection().is_none());
    }

    #[test]
    fn an_empty_team_is_read_without_complaint() {
        let parsed = ChampSelectSession::from_json(&json!({})).unwrap();
        assert!(parsed.local_player().is_none());
        assert!(parsed.selection().is_none());
        assert_eq!(parsed.timer.phase, "");
    }

    #[test]
    fn unknown_fields_and_new_phases_do_not_break_the_read() {
        let parsed = ChampSelectSession::from_json(&json!({
            "localPlayerCellId": 1,
            "somethingRiotAddedLater": { "nested": true },
            "timer": { "phase": "A_NEW_PHASE", "adjustedTimeLeftInPhase": 4200 },
            "myTeam": [
                { "cellId": 1, "championId": 555, "assignedPosition": "SUPPORT", "puuid": "x" },
            ],
        }))
        .unwrap();

        let selection = parsed.selection().unwrap();
        assert_eq!(selection.role(), Some(Role::Utility));
        assert_eq!(parsed.timer.phase, "A_NEW_PHASE");
    }
}
