//! The shared mini-game framework's behavior: the ticket-gated entry, the
//! tick-driven session lifecycle, the overlapping wave spawner, instanced
//! kill scoring, the win/lose reward algebra, and the death/leave roster
//! bookkeeping. Every service is a pure function over the caller-owned
//! [`crate::entities::minigame_session::MiniGameSession`] and a resolved
//! [`crate::data::atlas::MiniGameHandle`]; randomness enters only where a
//! landing or a spawn is sampled — the entry warp, wave spawns and respawns,
//! the alive warp-outs, and item-drop materialisation.

mod death;
mod entry;
mod lifecycle;
mod rewards;
mod scoring;
#[cfg(test)]
mod support;
mod waves;

pub use death::{report_death, report_leave};
pub use entry::{EnterOutcome, PkStanding, enter_mini_game};
pub use lifecycle::advance_mini_game;
pub use rewards::{
    FinisherAward, GrantDecision, ItemDropGrant, MoneyGrant, RewardOutcome, apply_item_drop_grant,
    apply_money_grant, finish_event, resolve_rewards,
};
pub use scoring::report_session_kill;
