//! Talking to the running League client.
//!
//! The launcher runs a small local web server so its own screens can talk to
//! it, and it lets other programs on the machine listen in. That is where the
//! champion you locked and the role you were assigned come from.
//!
//! The layer is built bottom-up:
//!
//! 1. [`lockfile`] — the port and password, rewritten every launch.
//!
//! Two rules shape everything here. The client is usually *not* running, so
//! that is a state and not an error; and champ select arrives over a
//! WebSocket rather than a polling loop, because a loop wakes the CPU while
//! the game is trying to draw frames.

pub mod error;
pub mod lockfile;

pub use error::LcuError;
pub use lockfile::{Lockfile, DEFAULT_LOCKFILE_PATH, LCU_USERNAME};
