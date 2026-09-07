//! The Live Client Data API, which the game serves on this machine while a
//! match is running.
//!
//! It is a different server from the LCU: a fixed port, no lockfile, and no
//! authentication of any kind — being able to reach it *is* the credential,
//! since it only listens on loopback. It exists for exactly this purpose and
//! is documented by Riot, so nothing here is scraped or reverse-engineered.
//!
//! TLS verification is disabled, and for the same narrow reason as in the LCU
//! client: the game presents a self-signed certificate for 127.0.0.1, there is
//! no authority that could vouch for it and no name resolution involved. The
//! base URL hardcodes the loopback address, so this client cannot be pointed
//! at the network.

use std::time::Duration;

use serde_json::Value;

use super::error::LiveError;
use super::game::GameSnapshot;

/// The port the game listens on. Fixed by Riot, not discovered.
pub const LIVE_CLIENT_PORT: u16 = 2999;

/// Everything about the current game in one read. One request rather than
/// four is the whole reason this path is preferred over the per-section ones.
pub const ALL_GAME_DATA_PATH: &str = "/liveclientdata/allgamedata";

/// The server is on this machine and answers in single-digit milliseconds. A
/// request still outstanding after this is not slow, it is gone — usually
/// because the game just ended.
///
/// Visible to the crate because it is the worst case for *any* answer from
/// this client, including "no game". A test that waits for one has to wait
/// longer than this, and deriving that deadline from this constant is what
/// stops the two drifting apart — see the live watcher's tests.
pub(crate) const REQUEST_TIMEOUT_SECS: u64 = 3;

#[derive(Debug)]
pub struct LiveClient {
    http: reqwest::Client,
    base: String,
}

impl Default for LiveClient {
    fn default() -> Self {
        LiveClient::new()
    }
}

impl LiveClient {
    pub fn new() -> LiveClient {
        LiveClient::at(&format!("https://127.0.0.1:{LIVE_CLIENT_PORT}"))
    }

    /// Point the client at a specific base URL. Tests use this; nothing in
    /// the app does, and the default hardcodes loopback.
    pub fn at(base: &str) -> LiveClient {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            // Justified in the module docs: loopback, self-signed, no DNS.
            .danger_accept_invalid_certs(true)
            .user_agent(concat!("spellcheck/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();

        LiveClient {
            http,
            base: base.trim_end_matches('/').to_string(),
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// The whole game, right now.
    ///
    /// `Ok(None)` means there is no game: the port is closed and the
    /// connection was refused, which is what this returns for the vast
    /// majority of the time the app is open. It is not an error and produces
    /// no log line. A game that has started but not yet spawned anyone
    /// answers 404 on this path, which is the same answer.
    pub async fn game(&self) -> Result<Option<GameSnapshot>, LiveError> {
        let url = format!("{}{}", self.base, ALL_GAME_DATA_PATH);

        let response = match self.http.get(&url).send().await {
            Ok(response) => response,
            // Refused, reset, timed out: all of them mean the same thing here.
            // There is no game, and there is nothing to report.
            Err(_) => return Ok(None),
        };

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(LiveError::Http {
                status: response.status().as_u16(),
                path: ALL_GAME_DATA_PATH.to_string(),
            });
        }

        let body = response
            .json::<Value>()
            .await
            .map_err(|error| LiveError::unexpected("a JSON body", error))?;

        GameSnapshot::from_json(&body).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_client_can_only_reach_this_machine() {
        let client = LiveClient::new();
        assert_eq!(client.base(), "https://127.0.0.1:2999");
        assert!(
            client.base().starts_with("https://127.0.0.1:"),
            "the live client must never be pointed off the loopback interface"
        );
    }

    #[tokio::test]
    async fn a_closed_port_is_no_game_rather_than_an_error() {
        // Nothing is listening here, which is the state this app spends
        // almost all of its life in.
        let client = LiveClient::at("https://127.0.0.1:1");
        let found = client.game().await;
        assert!(
            matches!(found, Ok(None)),
            "a refused connection was reported as a failure: {found:?}"
        );
    }
}
