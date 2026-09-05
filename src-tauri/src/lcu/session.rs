//! The champ select session, reduced to the facts we act on.
//!
//! The client's session payload is large and changes shape between patches,
//! so nothing here demands more than it needs: the local player's cell id,
//! the entry in `myTeam` that matches it, and the champion ids of both teams.
//! Everything else — bans, the action list, summoner names, the whole player
//! record behind each seat — is read past. A field we do not read cannot
//! break us when Riot renames it.
//!
//! The two teams are read for the recommendation engine, which reasons about
//! compositions rather than about one champion. `theirTeam` is frequently a
//! row of zeroes and that is not a fault: blind pick hides the enemy until
//! the game starts, so "we cannot see them" is an ordinary, expected answer
//! that the engine is built to receive.

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
    /// The enemy seats. Empty before picking starts, and full of zeroes in
    /// any queue that hides the enemy team.
    pub their_team: Vec<TeamMember>,
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

/// Both teams as champion ids, in seat order.
///
/// Zero means an empty or hidden seat and is passed through rather than
/// filtered out, because the count of seats we cannot see is what stops a
/// check from reading two locked-in enemies as a whole composition.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comp {
    /// Our own five, including the local player.
    pub ally: Vec<u32>,
    pub enemy: Vec<u32>,
}

impl Comp {
    /// True when there is nothing to reason about at all.
    pub fn is_empty(&self) -> bool {
        self.ally.iter().chain(&self.enemy).all(|id| *id == 0)
    }
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

    /// Both teams, as champ select currently shows them.
    pub fn comp(&self) -> Comp {
        let ids = |team: &[TeamMember]| team.iter().map(|member| member.champion_id).collect();
        Comp {
            ally: ids(&self.my_team),
            enemy: ids(&self.their_team),
        }
    }

    /// What the user locked, once they have locked something.
    ///
    /// `None` covers the ordinary in-between states: the session exists but
    /// our row has not arrived, or the user is still hovering.
    ///
    /// A blank position is *not* one of them. It used to be — there was no
    /// way to look a build up without a lane, so a lock with no lane was
    /// dropped here and champ select simply never fired. That meant Practice
    /// Tool, customs and ARAM, where the client assigns no position at all,
    /// went through the whole of champ select in silence and the app only
    /// noticed the champion once the game had started. The build layer works
    /// the lane out for itself now, so the lock is reported and the blank
    /// travels with it.
    pub fn selection(&self) -> Option<Selection> {
        let member = self.local_player()?;
        if member.champion_id == 0 {
            return None;
        }

        Some(Selection {
            champion_id: member.champion_id,
            assigned_position: member.assigned_position.clone(),
        })
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

    /// Practice Tool, customs and ARAM assign nobody a lane. Dropping the
    /// lock here is what kept champ select silent in all three, so that the
    /// app first noticed your champion once the game was already running.
    #[test]
    fn a_queue_without_assigned_positions_still_reports_the_lock() {
        let parsed = session(
            0,
            json!([{ "cellId": 0, "championId": 103, "assignedPosition": "" }]),
        );

        let selection = parsed.selection().expect("a locked champion is news either way");
        assert_eq!(selection.champion_id, 103);
        assert_eq!(selection.assigned_position, "");
        // Still no role — the blank is passed on, not invented here.
        assert!(selection.role().is_none());
    }

    /// Hovering is not locking, whatever the lane says.
    #[test]
    fn a_hover_is_not_a_lock_even_with_a_lane() {
        let parsed = session(
            0,
            json!([{ "cellId": 0, "championId": 0, "assignedPosition": "middle" }]),
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

#[cfg(test)]
mod comp_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn both_teams_are_read_in_seat_order() {
        let parsed = ChampSelectSession::from_json(&json!({
            "localPlayerCellId": 0,
            "myTeam": [
                { "cellId": 0, "championId": 103, "assignedPosition": "middle" },
                { "cellId": 1, "championId": 64, "assignedPosition": "jungle" },
            ],
            "theirTeam": [
                { "cellId": 5, "championId": 266, "assignedPosition": "top" },
                { "cellId": 6, "championId": 16, "assignedPosition": "utility" },
            ],
        }))
        .unwrap();

        let comp = parsed.comp();
        assert_eq!(comp.ally, vec![103, 64]);
        assert_eq!(comp.enemy, vec![266, 16]);
        assert!(!comp.is_empty());
    }

    #[test]
    fn a_hidden_enemy_team_is_zeroes_rather_than_absent() {
        // Blind pick: our side is visible to us, theirs is not.
        let parsed = ChampSelectSession::from_json(&json!({
            "localPlayerCellId": 0,
            "myTeam": [{ "cellId": 0, "championId": 103, "assignedPosition": "middle" }],
            "theirTeam": [
                { "cellId": 5, "championId": 0 },
                { "cellId": 6, "championId": 0 },
            ],
        }))
        .unwrap();

        let comp = parsed.comp();
        assert_eq!(comp.enemy, vec![0, 0]);
        assert_eq!(
            comp.enemy.len(),
            2,
            "the seats are kept: five hidden enemies and two hidden enemies \
             are different situations"
        );
    }

    #[test]
    fn a_session_with_no_teams_yet_is_empty_rather_than_an_error() {
        let parsed = ChampSelectSession::from_json(&json!({})).unwrap();
        let comp = parsed.comp();
        assert!(comp.is_empty());
        assert!(comp.ally.is_empty());
    }
}
