//! The frontend's entire surface area.
//!
//! Five commands, all thin. None of them names a provider: the UI asks for a
//! build and gets [`BuildLookup`] back, whether that came from OP.GG or from
//! our own crawl. Swapping sources is a config change, invisible from here.

use std::sync::Arc;

use tauri::State;

use crate::ddragon::{DataDragon, DataDragonState};
use crate::updater::UpdateInfo;
use crate::{BuildLookup, BuildService};

/// Attribution string for whichever source is live. The UI renders it
/// verbatim in the footer and must not branch on the value.
#[tauri::command]
pub fn source_label(service: State<'_, Arc<BuildService>>) -> String {
    service.source_label().to_string()
}

/// Look up one champion-role pair.
///
/// `role` arrives in the LCU's own vocabulary (`top`, `jungle`, `middle`,
/// `bottom`, `utility`), which is what champ select passes through untouched
/// on the other route into [`BuildService::build_for`](crate::BuildService).
///
/// Errors are stringified because they cross into JavaScript. "This source
/// has nothing for that pair" is *not* an error — it comes back as
/// [`BuildLookup::NoData`] so the UI can say so plainly.
/// `opponent` is the Data Dragon key of a lane opponent to build against, or
/// empty for the ordinary question. Naming one is only a request: whether the
/// build that comes back is really filtered to them is answered by
/// `matchup` on the build, which the screen reads rather than assuming.
#[tauri::command]
pub async fn fetch_build(
    service: State<'_, Arc<BuildService>>,
    champion: String,
    role: String,
    opponent: Option<String>,
) -> Result<BuildLookup, String> {
    // An empty box and an absent one mean the same thing, and the page sends
    // whichever is easier — so neither reaches the provider as an opponent.
    let opponent = opponent.filter(|key| !key.trim().is_empty());

    // The search box always names a role, so there is never a lane to work
    // out here and the flag that comes back is always false.
    service
        .build_for(&champion, &role, None, opponent.as_deref())
        .await
        .map(|resolved| resolved.lookup)
        .map_err(|error| error.to_string())
}

/// Where icon art lives.
///
/// The page may display images from Data Dragon but may not call it — the
/// window's content security policy is `default-src 'self'` — so the two
/// lookup tables it cannot derive from an id come through here instead.
///
/// An error is ordinary rather than fatal: the UI falls back to the text
/// labels it has always drawn, and the next call tries again.
#[tauri::command]
pub async fn data_dragon(ddragon: State<'_, Arc<DataDragonState>>) -> Result<DataDragon, String> {
    ddragon
        .get()
        .await
        .cloned()
        .map_err(|error| error.to_string())
}

/// Is a newer version waiting?
///
/// `None` means the app is current. A failure to reach the manifest comes
/// back as an error the page is expected to drop on the floor: this app
/// spends most of its life next to a League client that is not running, and
/// being offline is not news worth putting on screen.
#[tauri::command]
pub async fn check_update(app: tauri::AppHandle) -> Result<Option<UpdateInfo>, String> {
    crate::updater::check(&app).await
}

/// Open the installer for a newer version in the browser.
///
/// Only ever called from a button. The app does not install anything itself
/// while it is unsigned — see `updater` for what goes wrong when it tries.
#[tauri::command]
pub fn open_download(version: String) -> Result<(), String> {
    crate::updater::open_download(&version)
}
