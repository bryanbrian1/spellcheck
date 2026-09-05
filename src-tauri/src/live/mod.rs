//! Watching the game that is actually being played.
//!
//! Where the [`lcu`](crate::lcu) layer listens to the *launcher*, this one
//! reads the game itself. Riot serves a small documented API on this machine
//! while a match is running — fixed port, no lockfile, no authentication —
//! and it carries every player's items, level and score. That is the input to
//! the third check.
//!
//! The layer is built bottom-up:
//!
//! 1. [`client`] — the one HTTP read, where a refused connection means "no
//!    game" rather than a failure.
//! 2. [`game`] — the payload, reduced to who we are and who is in our lane.
//! 3. [`watcher`] — the loop, and the timing discipline that makes it
//!    affordable.
//!
//! The discipline is the whole difficulty. Champ select arrives on a socket
//! and costs nothing to wait for; this has no event stream and must be
//! polled, during the one period when the machine is busy drawing frames. So
//! the loop does not run at all unless the launcher is open, waits on a
//! channel rather than a timer while it is closed, and looks rarely once a
//! game is underway. See [`watcher`] for what each interval is and why.

pub mod client;
pub mod error;
pub mod game;
pub mod watcher;

pub use client::{LiveClient, ALL_GAME_DATA_PATH, LIVE_CLIENT_PORT};
pub use error::LiveError;
pub use game::{GameSnapshot, Player, Team};
pub use watcher::{watch, GameEvent, LiveWatcherConfig};
