//! The player-kill reputation a character carries: clean, or a flagged
//! murderer on a decaying three-rung ladder, alongside a lifetime kill tally.
//! A small serde value the character composes like
//! [`crate::components::life::LifeState`] — data only. The transitions that
//! flag, decay, and clear it live in [`crate::services`]; nothing here decides.

use serde::{Deserialize, Serialize};

use crate::components::units::Tick;

/// The three guilty rungs, ordered lightest-first so every consequence gate is
/// a `>=` compare — declaration order is ladder order. Clean is not a rung; it
/// is the absence of one (see [`Standing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PkStage {
    /// Flagged, but below the hunted threshold: heavier experience loss only.
    Warning,
    /// Hunted by guards, free-to-kill, barred from event entry.
    FirstStage,
    /// The worst rung (cap): all first-stage consequences plus the harshest
    /// experience loss.
    SecondStage,
}

/// The result of dropping one rung down the ladder: either off the ladder
/// entirely (back to clean) or onto a lower rung.
pub(crate) enum StageDrop {
    /// The rung dropped was the lightest — the character is now clean.
    ToClean,
    /// The character fell to a lower guilty rung.
    To(PkStage),
}

impl PkStage {
    /// One rung up, saturating at the cap — an unsanctioned kill climbs the
    /// ladder and can never overflow past the worst rung.
    pub(crate) const fn climbed(self) -> PkStage {
        match self {
            Self::Warning => Self::FirstStage,
            Self::FirstStage | Self::SecondStage => Self::SecondStage,
        }
    }

    /// One rung down — the lightest rung falls off the ladder to clean, every
    /// other rung to the rung beneath it.
    pub(crate) const fn dropped(self) -> StageDrop {
        match self {
            Self::Warning => StageDrop::ToClean,
            Self::FirstStage => StageDrop::To(Self::Warning),
            Self::SecondStage => StageDrop::To(Self::FirstStage),
        }
    }
}

/// Clean or a flagged murderer. The decay deadline lives only on the guilty
/// variant — mirrors [`crate::components::life::LifeState`]'s `Alive`/`Dead`,
/// so "clean with a stray deadline" is unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Standing {
    /// No standing — not a murderer, no decay deadline.
    Clean,
    /// A flagged murderer at `stage`, whose reputation decays at `decays_at`.
    Flagged {
        /// The rung on the ladder.
        stage: PkStage,
        /// The absolute online-time tick the next rung peels at.
        decays_at: Tick,
    },
}

impl Standing {
    /// The one definition of "at least first stage": guards hunt, the kill is
    /// free, event entry is refused. Every gate asks here — none re-derives it.
    #[must_use]
    pub fn is_hunted(self) -> bool {
        matches!(self, Self::Flagged { stage, .. } if stage >= PkStage::FirstStage)
    }
}

/// Lifetime player-kill tally — kept forever, never decays, nothing branches on
/// it (a display stat).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PlayerKillCount(
    /// The number of player kills ever recorded.
    pub u32,
);

/// A character's player-kill reputation: the decaying standing plus the
/// lifetime tally. The count sits beside — not within — the standing precisely
/// so it survives decay back to clean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reputation {
    standing: Standing,
    kills: PlayerKillCount,
}

impl Reputation {
    /// The clean reputation — no standing, zero kills. The serde `default` seed
    /// for a freshly created or legacy character, mirroring
    /// [`crate::components::life::LifeState::alive`].
    #[must_use]
    pub const fn clean() -> Self {
        Self {
            standing: Standing::Clean,
            kills: PlayerKillCount(0),
        }
    }

    /// The current standing (clean or flagged).
    #[must_use]
    pub fn standing(self) -> Standing {
        self.standing
    }

    /// The lifetime player-kill tally.
    #[must_use]
    pub fn kills(self) -> PlayerKillCount {
        self.kills
    }

    /// This reputation with a replaced standing, tally untouched.
    pub(crate) fn with_standing(self, standing: Standing) -> Self {
        Self { standing, ..self }
    }

    /// This reputation with one more player kill recorded, standing untouched;
    /// the tally saturates rather than wrapping.
    pub(crate) fn with_recorded_kill(self) -> Self {
        Self {
            kills: PlayerKillCount(self.kills.0.saturating_add(1)),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::units::Ticks;

    #[test]
    fn ordering_and_is_hunted_key_off_the_ladder() {
        assert!(
            PkStage::Warning < PkStage::FirstStage && PkStage::FirstStage < PkStage::SecondStage
        );
        assert!(!Standing::Clean.is_hunted());
        assert!(
            !Standing::Flagged {
                stage: PkStage::Warning,
                decays_at: Tick(1)
            }
            .is_hunted()
        );
        assert!(
            Standing::Flagged {
                stage: PkStage::FirstStage,
                decays_at: Tick(1)
            }
            .is_hunted()
        );
        assert!(
            Standing::Flagged {
                stage: PkStage::SecondStage,
                decays_at: Tick(1)
            }
            .is_hunted()
        );
    }

    #[test]
    fn climbed_saturates_and_dropped_falls_to_clean() {
        assert_eq!(PkStage::Warning.climbed(), PkStage::FirstStage);
        assert_eq!(PkStage::SecondStage.climbed(), PkStage::SecondStage);
        assert!(matches!(PkStage::Warning.dropped(), StageDrop::ToClean));
        assert!(matches!(
            PkStage::FirstStage.dropped(),
            StageDrop::To(PkStage::Warning)
        ));
    }

    #[test]
    fn clean_is_the_default_and_records_kills_beside_the_standing() {
        let r = Reputation::clean();
        assert_eq!(r.standing(), Standing::Clean);
        assert_eq!(r.kills(), PlayerKillCount(0));
        let r = r
            .with_standing(Standing::Flagged {
                stage: PkStage::Warning,
                decays_at: Tick(9),
            })
            .with_recorded_kill();
        assert_eq!(r.kills(), PlayerKillCount(1)); // count survives beside the standing
    }

    #[test]
    fn wire_round_trips() {
        let r = Reputation::clean()
            .with_standing(Standing::Flagged {
                stage: PkStage::FirstStage,
                decays_at: Tick(903),
            })
            .with_recorded_kill();
        let json = serde_json::to_string(&r).unwrap();
        assert!(
            json.contains(r#""kind":"flagged""#)
                && json.contains(r#""stage":"first_stage""#)
                && json.contains(r#""kills":1"#)
        );
        assert_eq!(serde_json::from_str::<Reputation>(&json).unwrap(), r);
    }

    #[test]
    fn tick_sub_ticks_saturates_at_zero() {
        assert_eq!(Tick(100) - Ticks(30), Tick(70));
        assert_eq!(Tick(10) - Ticks(999), Tick(0));
    }
}
