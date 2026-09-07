//! The client's REST API, over loopback.
//!
//! Two things are read here and nothing is written. Writing — item sets, rune
//! pages — is a user-initiated button press by rule, and there is no button
//! yet, so there is deliberately no code here that could write by accident.
//!
//! Certificate verification is disabled for this client, which needs a
//! justification rather than a shrug. The LCU serves a self-signed
//! certificate for 127.0.0.1; there is no authority that could vouch for it
//! and no name resolution involved. The base URL is built by
//! [`Lockfile::http_base`], which hardcodes the loopback address, so this
//! client cannot be pointed at the network. Nothing else in the app relaxes
//! TLS — the OP.GG client verifies normally.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::Mutex;

use crate::build_data::validate_champion_key;

use super::error::LcuError;
use super::lockfile::{Lockfile, LCU_USERNAME};
use super::session::{ChampSelectSession, CHAMP_SELECT_SESSION_URI};

/// The client is on this machine. A request that has not answered in this
/// long is not slow, it is gone — usually because the player quit mid-call.
const REQUEST_TIMEOUT_SECS: u64 = 5;

/// Champion id to Data Dragon key. The client is the authority for this
/// mapping, so we ask it rather than shipping a table that goes stale every
/// time a champion is released.
const CHAMPION_ASSET_PATH: &str = "/lol-game-data/assets/v1/champions";

#[derive(Debug)]
pub struct LcuClient {
    http: reqwest::Client,
    base: String,
    password: String,
    /// Champion keys never change within a run of the client, and champ
    /// select asks for the same one repeatedly as the session updates. This
    /// is a handful of short strings, held in memory only.
    champion_keys: Mutex<HashMap<u32, String>>,
}

impl LcuClient {
    pub fn new(lockfile: &Lockfile) -> Result<LcuClient, LcuError> {
        let base = lockfile.http_base();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            // Justified in the module docs: loopback, self-signed, no DNS.
            .danger_accept_invalid_certs(true)
            .user_agent(concat!("spellcheck/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| LcuError::transport(&base, error))?;

        Ok(LcuClient {
            http,
            base,
            password: lockfile.password().to_string(),
            champion_keys: Mutex::new(HashMap::new()),
        })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// GET one resource.
    ///
    /// `Ok(None)` is a 404, which the LCU uses for "that thing is not
    /// happening right now" — there is no champ select session when nobody is
    /// in champ select. That is a state, not a failure.
    pub async fn get(&self, path: &str) -> Result<Option<Value>, LcuError> {
        let url = format!("{}{}", self.base, path);
        let response = self
            .http
            .get(&url)
            .basic_auth(LCU_USERNAME, Some(&self.password))
            .send()
            .await
            .map_err(|error| LcuError::transport(&self.base, error))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(LcuError::Http {
                status: response.status().as_u16(),
                path: path.to_string(),
            });
        }

        response
            .json::<Value>()
            .await
            .map(Some)
            .map_err(|error| LcuError::unexpected("a JSON body", error))
    }

    /// The champ select session as it stands right now.
    ///
    /// The WebSocket carries every change, but it only carries *changes* —
    /// connecting mid-champ-select would otherwise show nothing until the
    /// next update. This is the one read that fills that gap.
    pub async fn champ_select_session(&self) -> Result<Option<ChampSelectSession>, LcuError> {
        match self.get(CHAMP_SELECT_SESSION_URI).await? {
            Some(value) => ChampSelectSession::from_json(&value).map(Some),
            None => Ok(None),
        }
    }

    /// Resolve a champion id to the key our providers look builds up by.
    ///
    /// The id is what champ select sends; `Ahri` is what a build source
    /// wants. The key is validated before it leaves this function because it
    /// goes on to become a file path and a URL value.
    pub async fn champion_key(&self, champion_id: u32) -> Result<Option<String>, LcuError> {
        if let Some(key) = self.champion_keys.lock().await.get(&champion_id) {
            return Ok(Some(key.clone()));
        }

        let Some(asset) = self
            .get(&format!("{CHAMPION_ASSET_PATH}/{champion_id}.json"))
            .await?
        else {
            return Ok(None);
        };

        let key = champion_key_from_asset(&asset, champion_id)?;
        self.champion_keys
            .lock()
            .await
            .insert(champion_id, key.clone());
        Ok(Some(key))
    }
}

/// Pull the Data Dragon key out of a champion asset and check it.
///
/// Split out from the request so the shape of the client's answer is testable
/// without a running client.
pub fn champion_key_from_asset(asset: &Value, champion_id: u32) -> Result<String, LcuError> {
    let alias = asset
        .get("alias")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|alias| !alias.is_empty())
        .ok_or_else(|| {
            LcuError::unexpected(
                "a champion key",
                format!("champion {champion_id} has no alias"),
            )
        })?;

    validate_champion_key(alias)
        .map(str::to_string)
        .map_err(|error| LcuError::unexpected("a champion key", error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn lockfile() -> Lockfile {
        Lockfile::parse(
            "LeagueClient:4242:52519:pw:https",
            std::path::Path::new("/tmp/lockfile"),
        )
        .unwrap()
    }

    #[test]
    fn is_built_against_loopback_only() {
        let client = LcuClient::new(&lockfile()).unwrap();
        assert_eq!(client.base(), "https://127.0.0.1:52519");
    }

    #[test]
    fn reads_the_champion_key_out_of_an_asset() {
        let asset = json!({ "id": 62, "name": "Wukong", "alias": "MonkeyKing" });
        assert_eq!(champion_key_from_asset(&asset, 62).unwrap(), "MonkeyKing");
    }

    #[test]
    fn refuses_an_alias_that_could_become_a_path() {
        let asset = json!({ "id": 1, "alias": "../../etc/passwd" });
        assert!(champion_key_from_asset(&asset, 1).is_err());
    }

    #[test]
    fn a_missing_alias_is_reported_rather_than_guessed() {
        let error = champion_key_from_asset(&json!({ "id": 1 }), 1).unwrap_err();
        assert!(error.to_string().contains("champion key"), "{error}");
    }
}
