//! Checking for, and installing, a new version of the app.
//!
//! The whole of this lives in Rust rather than in the page. The frontend
//! calls two of our own commands, the same way it calls everything else, and
//! never touches the updater plugin directly — which is what keeps
//! `capabilities/default.json` able to say the webview reaches nothing but
//! our commands.
//!
//! Nothing here installs on its own. `check` reports; `install` acts, and is
//! only ever reached by a button press. That is the same rule
//! `CLAUDE.md` sets for item set and rune imports, applied for the same
//! reason: an app that rewrites itself while a game is in champ select is an
//! app that took the decision away from the player.

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

/// What the page is told about a waiting update.
///
/// Deliberately not the plugin's own type: that one carries the download
/// handle and a good deal else, none of which the page has any business
/// seeing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// The version on offer, not the version running.
    pub version: String,
    /// Release notes, as written into the manifest. May be empty.
    pub notes: String,
}

/// Is there a newer version?
///
/// `Ok(None)` is the ordinary answer and means the app is current. An error
/// here is not worth interrupting anybody over — the endpoint being
/// unreachable is indistinguishable from being offline, which for this app is
/// the normal state — so the caller is expected to swallow it quietly.
pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let update = app
        .updater()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())?;

    Ok(update.map(|update| UpdateInfo {
        version: update.version.clone(),
        notes: update.body.clone().unwrap_or_default(),
    }))
}

/// Download the waiting update, install it, and relaunch into it.
///
/// The check runs again here rather than the handle from [`check`] being held
/// in app state. That costs one more request to a few hundred bytes of
/// manifest, and buys not having to keep a live download handle alive across
/// the gap between a page rendering a bar and somebody clicking it — which
/// may be the length of a game.
///
/// The signature on the downloaded archive is verified against the public key
/// compiled into `tauri.conf.json` before anything is written. That check is
/// the plugin's, it cannot be turned off, and it is the only reason serving
/// these files from a bucket anybody can read is safe.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let update = app
        .updater()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "no update to install".to_string())?;

    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|error| error.to_string())?;

    // Windows never reaches this line: the NSIS installer needs the app it is
    // replacing to have exited, so the plugin closes it during install.
    app.restart();
}
