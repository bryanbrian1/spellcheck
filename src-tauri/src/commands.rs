//! The frontend's entire surface area.
//!
//! Two commands, both thin. Neither one names a provider: the UI asks for a
//! build and gets [`BuildLookup`] back, whether that came from OP.GG or from
//! our own crawl. Swapping sources is a config change, invisible from here.

use tauri::State;

use crate::{BuildLookup, BuildService};

/// Attribution string for whichever source is live. The UI renders it
/// verbatim in the footer and must not branch on the value.
#[tauri::command]
pub fn source_label(service: State<'_, BuildService>) -> String {
    service.source_label().to_string()
}

/// Look up one champion-role pair.
///
/// `role` arrives in the LCU's own vocabulary (`top`, `jungle`, `middle`,
/// `bottom`, `utility`) so that champ select can pass `assignedPosition`
/// straight through once the WebSocket listener exists.
///
/// Errors are stringified because they cross into JavaScript. "This source
/// has nothing for that pair" is *not* an error — it comes back as
/// [`BuildLookup::NoData`] so the UI can say so plainly.
#[tauri::command]
pub async fn fetch_build(
    service: State<'_, BuildService>,
    champion: String,
    role: String,
) -> Result<BuildLookup, String> {
    service
        .build_for(&champion, &role, None)
        .await
        .map_err(|error| error.to_string())
}
