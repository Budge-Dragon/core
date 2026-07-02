//! Stateless game rules: combat math, damage formulas, drop resolution,
//! leveling, and skill application.
//!
//! Services are pure functions over entities and components. They take the
//! current state plus an injected RNG and return updated state along with
//! [`crate::events`] — they never log, block, or touch the host.
