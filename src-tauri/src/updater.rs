//! Noticing that a new version exists, and pointing at it.
//!
//! This is deliberately *not* a self-installing updater, even though the
//! machinery for one is present and tested. On macOS an unsigned app cannot
//! replace its own bundle inside `/Applications`: App Management protection
//! decides whether an app may modify an app there by looking at code
//! signatures, an unsigned bundle has no identity to check, and what actually
//! happens is that the new version lands beside the old one under a Finder
//! collision name and the original is deleted. The user is left with no app.
//!
//! So until the app is signed, the check runs and the install does not. The
//! bar reports the new version and opens the download; the user installs it
//! the ordinary way. Re-enabling the in-place install is a matter of wiring
//! [`install`] back to a command — see the note on that function.
//!
//! The whole of this lives in Rust rather than in the page. The frontend
//! calls our own commands, the same way it calls everything else, and never
//! touches the updater plugin directly — which is what keeps
//! `capabilities/default.json` able to say the webview reaches nothing but
//! our commands.

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

/// Where installable builds live. Shares a bucket with the update manifest
/// because they are published by the same job from the same artifacts, and a
/// second host would be a second thing that can be out of date.
pub const DOWNLOAD_BASE: &str = "https://pub-7d8d63aa0eec43f5a16598403866eed1.r2.dev";

/// What the page is told about a waiting version.
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

/// Open the installer for `version` in the user's browser.
///
/// The URL is built here rather than accepted from the page. The page already
/// knows the version, so passing the whole address would be more convenient
/// and would also mean the one thing in this app that launches an external
/// process takes its target from the webview. It does not: the host is a
/// constant, and `version` is refused unless it is digits and dots, so the
/// worst a compromised page can ask for is a file that does not exist.
pub fn open_download(version: &str) -> Result<(), String> {
    if version.is_empty()
        || !version
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.')
    {
        return Err(format!("refusing to open a download for {version:?}"));
    }

    // Each platform's installer, named as the release workflow uploads it.
    let file = if cfg!(target_os = "windows") {
        format!("spellcheck_{version}_x64-setup.exe")
    } else {
        format!("spellcheck_{version}_universal.dmg")
    };
    let url = format!("{DOWNLOAD_BASE}/{file}");

    // `open` and `start` rather than a plugin: this is the only thing in the
    // app that needs a browser, and a dependency for one call is not worth
    // the bytes. `start` needs the empty argument — it reads a lone quoted
    // first argument as a window title.
    let spawned = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .spawn()
    } else {
        std::process::Command::new("open").arg(&url).spawn()
    };

    spawned
        .map(|_| ())
        .map_err(|error| format!("could not open {url}: {error}"))
}

/// Download the waiting update, install it, and relaunch into it.
///
/// **Parked, and not reachable from the page.** This works — it was run end
/// to end against a real bucket — but only where the app is not inside
/// `/Applications`, which is where users put it. It is kept rather than
/// deleted because the day the app is code signed this becomes correct again,
/// and the way back is to expose it as a command and point the bar's button
/// at it instead of [`open_download`].
///
/// `Ok(false)` means there was nothing left to install by the time the button
/// was pressed, which is a no-op rather than a failure.
#[allow(dead_code)]
pub async fn install(app: &AppHandle) -> Result<bool, String> {
    let Some(update) = app
        .updater()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };

    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|error| error.to_string())?;

    // Windows never reaches this line: the NSIS installer needs the app it is
    // replacing to have exited, so the plugin closes it during install.
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::open_download;

    #[test]
    fn a_version_that_is_not_digits_and_dots_is_refused() {
        // The page supplies this, so the check is the boundary rather than a
        // formality. A traversal is the shape that would matter.
        for bad in ["", "../../etc/passwd", "0.1.2 ; rm -rf /", "latest"] {
            assert!(
                open_download(bad).is_err(),
                "{bad:?} should not have been opened"
            );
        }
    }
}
