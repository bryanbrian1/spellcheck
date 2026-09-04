//! Talking to the running League client.
//!
//! The launcher runs a small local web server so its own screens can talk to
//! it, and it lets other programs on the machine listen in. That is where the
//! champion you locked and the role you were assigned come from.
//!
//! The layer is built bottom-up:
//!
//! 1. [`lockfile`] — the port and password, rewritten every launch.
//! 2. [`client`] — the REST API, for the champion-id-to-key mapping and for
//!    the one read that catches us up if we connect mid-champ-select.
//! 3. [`session`] — the champ select payload, reduced to champion and role.
//! 4. [`ws`] — the event socket the client pushes changes down.
//! 5. [`watcher`] — the loop over all of it, whose output is a champion key
//!    and a position: exactly what
//!    [`BuildService::build_for`](crate::BuildService::build_for) takes.
//!
//! Two rules shape everything here. The client is usually *not* running, so
//! that is a state and not an error; and champ select arrives over a
//! WebSocket rather than a polling loop, because a loop wakes the CPU while
//! the game is trying to draw frames.

pub mod client;
pub mod error;
pub mod lockfile;
pub mod session;
pub mod watcher;
pub mod ws;

pub use client::LcuClient;
pub use error::LcuError;
pub use lockfile::{Lockfile, DEFAULT_LOCKFILE_PATH, LCU_USERNAME};
pub use session::{ChampSelectSession, Selection, CHAMP_SELECT_SESSION_URI};
pub use watcher::{watch, ChampSelectEvent, LockedChampion, WatcherConfig};
pub use ws::{LcuEventStream, LcuJsonEvent, CHAMP_SELECT_EVENT};
