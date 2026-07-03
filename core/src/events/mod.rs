//! Events returned by services instead of performing side effects.
//!
//! The core never logs, sends packets, or writes to storage. Every observable
//! outcome (damage dealt, item dropped, level gained) is returned as an event
//! value; the host decides whether to log it, persist it, or broadcast it.

pub mod monster_ai;
pub mod movement;
pub mod spawn;
