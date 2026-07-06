//! The outcome events of the consume service ([`crate::services::consume`]):
//! a recovery, a poison cure, or a typed refusal. One kind-tagged flat enum —
//! the [`crate::events::shop`] peer-enum grain — carrying the authoritative
//! result the server decided, never a client-claimed heal number. The
//! [`PoolKind`] discriminator and the [`ConsumeRejection`] roster live here
//! beside the event they tag; neither imports a service.

use serde::{Deserialize, Serialize};

/// Which recovery pool a consumable restored — a two-variant discriminator, not
/// a truncated three: ability (AG) is never a recovery target, so only health
/// and mana can be recovered. Serialized as a bare `snake_case` string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolKind {
    /// The health pool.
    Health,
    /// The mana pool.
    Mana,
}

/// Why a consume was refused. Every case leaves the stack whole (nothing
/// consumed) and the character untouched. `NoEffect` is the one unified reason
/// for "consuming would change nothing" — a full pool, a zero-magnitude heal, or
/// an antidote with no poison to clear. Serialized as a bare `snake_case`
/// string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumeRejection {
    /// The character is dead and cannot consume.
    NotAlive,
    /// No item covers the addressed cell.
    NoItem,
    /// The item at the cell is not a consumable.
    NotConsumable,
    /// The consumable is not a recovery or cure item — this service applies only
    /// the recovery/cure family (an alcohol buff or a town-portal warp has its
    /// own owning service).
    NotRecoverable,
    /// Consuming would produce no observable change (a full pool, a
    /// zero-magnitude heal, or an antidote with no active poison).
    NoEffect,
}

/// What a consume produced, kind-tagged: a pool recovered, a poison cured, or a
/// typed refusal. A `Recovered` carries the actual post-cap delta applied — the
/// truthful, non-recomputable observable, never the pre-cap computed amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConsumeEvent {
    /// A pool was recovered by `restored` points — the actual gain applied after
    /// the cap-at-max clamp, not the pre-cap computed amount.
    Recovered {
        /// Which pool was recovered.
        pool: PoolKind,
        /// The points actually restored (the post-cap delta).
        restored: u32,
    },
    /// The character's active poison was cured.
    PoisonCured,
    /// The consume was refused; the stack stays whole and the character is
    /// unchanged.
    Rejected {
        /// Why the consume was refused.
        reason: ConsumeRejection,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trips(event: ConsumeEvent, json: &str) {
        assert_eq!(serde_json::to_string(&event).unwrap(), json);
        assert_eq!(serde_json::from_str::<ConsumeEvent>(json).unwrap(), event);
    }

    #[test]
    fn recovered_round_trips_for_each_pool() {
        round_trips(
            ConsumeEvent::Recovered {
                pool: PoolKind::Health,
                restored: 110,
            },
            r#"{"kind":"recovered","pool":"health","restored":110}"#,
        );
        round_trips(
            ConsumeEvent::Recovered {
                pool: PoolKind::Mana,
                restored: 90,
            },
            r#"{"kind":"recovered","pool":"mana","restored":90}"#,
        );
    }

    #[test]
    fn poison_cured_round_trips() {
        round_trips(ConsumeEvent::PoisonCured, r#"{"kind":"poison_cured"}"#);
    }

    #[test]
    fn rejected_round_trips_for_every_reason() {
        round_trips(
            ConsumeEvent::Rejected {
                reason: ConsumeRejection::NotAlive,
            },
            r#"{"kind":"rejected","reason":"not_alive"}"#,
        );
        round_trips(
            ConsumeEvent::Rejected {
                reason: ConsumeRejection::NoItem,
            },
            r#"{"kind":"rejected","reason":"no_item"}"#,
        );
        round_trips(
            ConsumeEvent::Rejected {
                reason: ConsumeRejection::NotConsumable,
            },
            r#"{"kind":"rejected","reason":"not_consumable"}"#,
        );
        round_trips(
            ConsumeEvent::Rejected {
                reason: ConsumeRejection::NotRecoverable,
            },
            r#"{"kind":"rejected","reason":"not_recoverable"}"#,
        );
        round_trips(
            ConsumeEvent::Rejected {
                reason: ConsumeRejection::NoEffect,
            },
            r#"{"kind":"rejected","reason":"no_effect"}"#,
        );
    }
}
