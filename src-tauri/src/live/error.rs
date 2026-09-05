//! Errors from the in-game Live Client Data API.
//!
//! As in the LCU layer, one variant is conspicuously absent: "no game is
//! running". The overwhelming majority of the time there is no game, the port
//! is closed, and the connection is refused instantly. That is a state, and
//! it travels as [`Option::None`], not as an error — logging a failure for it
//! would mean logging one every time we look.

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum LiveError {
    /// The endpoint answered with a status we did not expect. A refused
    /// connection is not this: that is "no game", and never reaches here.
    #[error("the live client answered {status} for {path}")]
    Http { status: u16, path: String },

    /// The response arrived but did not carry the shape we read. A patch that
    /// renames a field lands here, and it is worth a log line because unlike a
    /// closed port it means something we believed is no longer true.
    #[error("could not read {what} out of the live client's response: {detail}")]
    Unexpected { what: &'static str, detail: String },
}

impl LiveError {
    pub fn unexpected(what: &'static str, detail: impl std::fmt::Display) -> Self {
        LiveError::Unexpected {
            what,
            detail: detail.to_string(),
        }
    }
}

/// Same reasoning as the other error types here: these cross into JavaScript,
/// where only the message is ever shown.
impl Serialize for LiveError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
