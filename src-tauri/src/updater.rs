//! Noticing that a new version exists, and — on Windows — installing it.
//!
//! Whether the app may replace itself is a per-platform answer, and for a
//! while it was answered globally with "no". That was right for the platform
//! it was reasoned about and wrong for the other one.
//!
//! On macOS an unsigned app cannot replace its own bundle inside
//! `/Applications`: App Management protection decides whether an app may
//! modify an app there by looking at code signatures, an unsigned bundle has
//! no identity to check, and what actually happens is that the new version
//! lands beside the old one under a Finder collision name and the original is
//! deleted. The user is left with no app. So macOS stays a notice: the bar
//! reports the new version and [`open_download`] opens its installer, and the
//! user installs it the ordinary way. Code signing is what changes this, and
//! nothing else will.
//!
//! Windows has none of that. The NSIS installer replaces the app the way any
//! installer replaces any program, and the plugin closes the running copy
//! first because the installer requires it. SmartScreen still fires — the
//! build is unsigned, and on Windows that means it fires on every update
//! rather than only on first install — but one "Run anyway" is a great deal
//! less than a manual download and reinstall, and Windows is where most of
//! this app's users are. So [`install`] is reachable there.
//!
//! The split is enforced here rather than in the page. A webview that decides
//! which platform it is running on is a webview that can be talked into
//! deciding wrong.
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
    /// Whether this platform can install the update in place. False on macOS
    /// while the app is unsigned, and the page uses it to decide what its
    /// button says — not whether it is allowed to install, which [`install`]
    /// decides for itself.
    pub can_install: bool,
}

/// Can this platform replace the app in place?
///
/// Windows only, and not because of anything about the update — because of
/// what macOS does to an unsigned bundle inside `/Applications`. See the
/// module documentation.
pub const fn can_install() -> bool {
    cfg!(target_os = "windows")
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
        can_install: can_install(),
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
/// Windows only. The guard is here rather than in the command or the page
/// because this is the function that would do the damage, and a check that
/// lives anywhere else is a check that a future caller can forget. On macOS
/// this is not a thing that fails cleanly — an unsigned bundle replacing
/// itself inside `/Applications` leaves the user with no app at all — so it
/// refuses before it reaches the plugin.
///
/// `Ok(false)` means there was nothing left to install by the time the button
/// was pressed, which is a no-op rather than a failure.
pub async fn install(app: &AppHandle) -> Result<bool, String> {
    if !can_install() {
        return Err("this platform installs updates by hand".into());
    }

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
    use super::{can_install, open_download, UpdateInfo};

    #[test]
    fn the_page_is_told_whether_this_platform_can_install() {
        let info = UpdateInfo {
            version: "0.1.3".into(),
            notes: String::new(),
            can_install: can_install(),
        };
        let json = serde_json::to_value(&info).expect("UpdateInfo should serialize");

        // The page reads this key to decide whether its button installs or
        // downloads. Renaming the field without the rename_all attribute would
        // leave the button silently reading undefined and always downloading,
        // which is a worse bug than a broken build.
        assert_eq!(json["canInstall"], serde_json::json!(cfg!(target_os = "windows")));
    }

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
