//! Errors from talking to the League client.
//!
//! One variant is conspicuously absent: "League is not running". That is the
//! normal state of this machine — the player is at their desk, the game is
//! closed — so it travels as [`Option::None`] out of
//! [`Lockfile::read`](super::lockfile::Lockfile::read) and as an ordinary
//! state out of everything built on top of it. Making it an error would mean
//! logging a failure every few seconds for the twenty-three hours a day
//! nobody is in champ select.

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum LcuError {
    /// The lockfile exists but we could not read it. Usually a permissions
    /// problem, which is worth surfacing — a missing file is not this.
    #[error("could not read the League lockfile at {path}: {detail}")]
    LockfileUnreadable { path: String, detail: String },

    /// The file exists and was read, but is not
    /// `ProcessName:PID:Port:Password:Protocol`. Either the client is
    /// mid-write or Riot changed the format.
    #[error("the League lockfile at {path} is not in the expected format: {detail}")]
    LockfileMalformed { path: String, detail: String },

    /// The client went away, or never answered. Extremely ordinary: the
    /// player quit the client between our reading the lockfile and our
    /// connecting to it.
    #[error("the League client is not answering on {endpoint}: {detail}")]
    Transport { endpoint: String, detail: String },

    /// A request reached the client and came back with a status we did not
    /// expect. 404 is handled as "nothing there" and never reaches here.
    #[error("the League client answered {status} for {path}")]
    Http { status: u16, path: String },

    /// The response arrived but did not carry the shape we read. A client
    /// update that renames a field lands here.
    #[error("could not read {what} out of the League client's response: {detail}")]
    Unexpected { what: &'static str, detail: String },

    /// The champ-select event socket failed. Reconnecting is the answer, not
    /// giving up.
    #[error("the champ select event socket failed: {0}")]
    Socket(String),
}

impl LcuError {
    pub fn transport(endpoint: impl Into<String>, detail: impl std::fmt::Display) -> Self {
        LcuError::Transport {
            endpoint: endpoint.into(),
            detail: detail.to_string(),
        }
    }

    pub fn unexpected(what: &'static str, detail: impl std::fmt::Display) -> Self {
        LcuError::Unexpected {
            what,
            detail: detail.to_string(),
        }
    }

    /// Whether waiting and trying again is likely to help. A malformed
    /// lockfile is retryable because the client writes it in pieces; a
    /// response we cannot parse is not, because the next one will look the
    /// same.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            LcuError::Transport { .. } | LcuError::Socket(_) | LcuError::LockfileMalformed { .. }
        )
    }
}

/// Same reasoning as `ProviderError`: these cross into JavaScript, where only
/// the message is ever shown.
impl Serialize for LcuError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
