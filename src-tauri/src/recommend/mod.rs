//! The recommendation engine.
//!
//! Everything this module produces is a **rule**: it reasons from tags —
//! "Aatrox and Soraka both heal" — rather than from a sample, and the app
//! renders it in amber to say so. The teal colour, and the numbers that come
//! with it, belong to the build data layer, which has game counts behind it.
//! Keeping those apart is a correctness requirement, not a styling choice.
//!
//! This commit lands the vocabulary and the two tag files the checks read.
//! The checks themselves follow.

pub mod tags;

pub use tags::{Answer, ChampionTags, DamageType, ItemTags, Tags};
