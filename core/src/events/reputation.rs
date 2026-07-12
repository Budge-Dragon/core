//! What a player-kill sanction, a flag, or a reputation decay produced. The
//! killer-side twin of [`crate::events::death`]: the flag path returns one
//! [`PkEvent`] per resolved kill, the decay path one per peel or accelerated
//! deadline. Outcome data only — the transitions live in
//! [`crate::services::reputation`]; nothing here decides.

use serde::{Deserialize, Serialize};

use crate::components::reputation::{PkStage, PlayerKillCount, Standing};
use crate::components::units::{Tick, Ticks};

/// One observable outcome of a reputation transition, kind-tagged. A flagged
/// kill emits [`PkEvent::Flagged`]; a free kill emits [`PkEvent::Sanctioned`];
/// a decay peel emits [`PkEvent::Decayed`]; a monster-kill acceleration that
/// pulls the deadline earlier without peeling emits [`PkEvent::DecayAccelerated`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PkEvent {
    /// An unsanctioned kill flagged the killer onto (or up) the murderer ladder.
    Flagged {
        /// The rung the killer now sits on.
        stage: PkStage,
        /// The absolute online-time tick the next rung peels at.
        decays_at: Tick,
        /// The killer's lifetime player-kill tally after this kill.
        lifetime_kills: PlayerKillCount,
    },
    /// The kill was free — no flag, no state change — for the stated reason.
    Sanctioned {
        /// Why the kill carried no sanction.
        reason: SanctionReason,
    },
    /// Time decay peeled the killer down one rung (or off the ladder to clean).
    Decayed {
        /// The standing after the peel.
        standing: Standing,
    },
    /// A monster kill pulled the decay deadline earlier without peeling a rung.
    DecayAccelerated {
        /// The deadline after the pull.
        decays_at: Tick,
        /// How far the deadline was pulled earlier.
        reduced_by: Ticks,
    },
}

/// Why a kill was free — the sanctioned circumstance. `VictimWasMurderer` is
/// core-decided from the victim's authoritative reputation; the rest are host
/// circumstances the host attests through the sanction's context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SanctionReason {
    /// The victim was already a hunted murderer — killing one is always free.
    VictimWasMurderer,
    /// The victim struck first — a self-defense kill.
    SelfDefense,
    /// The victim belonged to a rival guild.
    RivalGuild,
    /// The kill happened inside a sanctioned duel.
    Duel,
    /// The kill happened inside a player-versus-player mini-game.
    MiniGamePvp,
}
