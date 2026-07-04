//! The outcome events of the timed-effect services, kind-tagged. [`EffectEvent`]
//! is the effect delivery vocabulary a host emits:
//! [`crate::services::effects::advance_effects`] returns the poison ticks, kills,
//! and expiries as it ticks a store forward, while the two application events
//! ([`EffectEvent::EffectApplied`], [`EffectEvent::Healed`]) are host-emitted —
//! wrapping a freshly resolved effect or heal — with no in-core producer, exactly
//! as spawn/placement events are. [`BuffCastOutcome`] is what a buff or heal cast
//! resolves to, reusing [`crate::events::skills::CastRejection`] so there is no
//! duplicate rejection vocabulary. One service, one outcome enum.

use serde::{Deserialize, Serialize};

use crate::components::active_effect::{ActiveEffect, EffectIdentity};
use crate::events::combat::Damage;
use crate::events::skills::CastRejection;

/// What advancing a timed-effect store produced, kind-tagged: an effect applied,
/// a poison tick (or the lethal one), or an effect that expired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectEvent {
    /// A timed effect was applied — carries the resolved effect. Host-emitted:
    /// the host wraps the [`ActiveEffect`] that
    /// [`crate::services::effects::apply_buff`] /
    /// [`crate::services::effects::apply_ailment`] resolves when it stores the
    /// effect on the receiver. No `advance_effects` path produces it.
    EffectApplied {
        /// The effect now active.
        effect: ActiveEffect,
    },
    /// A poison tick dealt `damage` and the target survived.
    PoisonTick {
        /// The tick's damage.
        damage: Damage,
    },
    /// A poison tick dealt `damage` and reduced the target to zero health.
    PoisonKilled {
        /// The lethal tick's damage.
        damage: Damage,
    },
    /// A timed effect expired and was removed.
    EffectExpired {
        /// Which effect expired.
        effect: EffectIdentity,
    },
    /// A heal restored `amount` health. Host-emitted: the host maps a
    /// [`BuffCastOutcome::Healed`] into this delivery event when it applies the
    /// restored health to the receiver. No `advance_effects` path produces it.
    Healed {
        /// Health actually restored (bounded by the maximum).
        amount: u32,
    },
}

/// What a buff or heal cast resolved to, kind-tagged: rejected before spending
/// anything, a buff applied, or a heal restored. Reuses
/// [`CastRejection`] — the shared cast-rejection vocabulary, not a duplicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BuffCastOutcome {
    /// The cast was rejected; no resource was spent.
    Rejected {
        /// Why the cast was rejected.
        reason: CastRejection,
    },
    /// A buff was applied to the receiver — carries the resolved effect the host
    /// stores on the receiver.
    Applied {
        /// The effect the receiver now carries.
        effect: ActiveEffect,
    },
    /// A heal restored `amount` health to the receiver.
    Healed {
        /// Health actually restored (bounded by the maximum).
        amount: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::active_effect::PoisonTicks;
    use crate::components::units::{Tick, Ticks};

    #[test]
    fn effect_applied_wire_pins() {
        let event = EffectEvent::EffectApplied {
            effect: ActiveEffect::Defense { expiry: Tick(80) },
        };
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"kind":"effect_applied","effect":{"kind":"defense","expiry":80}}"#
        );
    }

    #[test]
    fn poison_tick_and_kill_wire_pins() {
        assert_eq!(
            serde_json::to_string(&EffectEvent::PoisonTick { damage: Damage(9) }).unwrap(),
            r#"{"kind":"poison_tick","damage":9}"#
        );
        assert_eq!(
            serde_json::to_string(&EffectEvent::PoisonKilled { damage: Damage(9) }).unwrap(),
            r#"{"kind":"poison_killed","damage":9}"#
        );
    }

    #[test]
    fn expired_and_healed_wire_pins() {
        assert_eq!(
            serde_json::to_string(&EffectEvent::EffectExpired {
                effect: EffectIdentity::Iced,
            })
            .unwrap(),
            r#"{"kind":"effect_expired","effect":"iced"}"#
        );
        assert_eq!(
            serde_json::to_string(&EffectEvent::Healed { amount: 15 }).unwrap(),
            r#"{"kind":"healed","amount":15}"#
        );
    }

    #[test]
    fn buff_cast_outcome_wire_pins_and_round_trips() {
        let rejected = BuffCastOutcome::Rejected {
            reason: CastRejection::InsufficientMana,
        };
        assert_eq!(
            serde_json::to_string(&rejected).unwrap(),
            r#"{"kind":"rejected","reason":"insufficient_mana"}"#
        );
        let applied = BuffCastOutcome::Applied {
            effect: ActiveEffect::Poisoned {
                per_tick_damage: 12,
                remaining: PoisonTicks::INITIAL,
                next_tick: Tick(60),
                cadence: Ticks(60),
            },
        };
        let json = serde_json::to_string(&applied).unwrap();
        assert_eq!(
            serde_json::from_str::<BuffCastOutcome>(&json).unwrap(),
            applied
        );
        assert_eq!(
            serde_json::to_string(&BuffCastOutcome::Healed { amount: 20 }).unwrap(),
            r#"{"kind":"healed","amount":20}"#
        );
    }
}
