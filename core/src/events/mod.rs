//! Events returned by services instead of performing side effects.
//!
//! The core never logs, sends packets, or writes to storage. Every observable
//! outcome (damage dealt, item dropped, level gained) is returned as an event
//! value; the host decides whether to log it, persist it, or broadcast it.

pub mod combat;
pub mod kill;
pub mod loot;
pub mod monster_ai;
pub mod movement;
pub mod progression;
pub mod skills;
pub mod spawn;
