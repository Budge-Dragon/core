//! Game entities: the aggregate objects the simulation operates on.
//!
//! Characters, monsters, NPCs, and world items live here as plain data types
//! composed from [`crate::components`]. Entities carry no host concerns — no
//! engine handles, no database rows, no network state.

pub mod character;
pub mod monster_instance;
pub mod spawned;
pub mod trade_session;
pub mod world_item;
pub mod world_zen;
