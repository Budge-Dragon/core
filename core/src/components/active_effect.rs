//! The live timed-effect store an entity carries: the total [`ActiveEffects`]
//! structure with one named slot per effect identity, plus the flat
//! kind-tagged [`ActiveEffect`] element it iterates and serializes to. Data
//! only — the magnitude/duration resolution and the tick advance live in
//! [`crate::services::effects`]; nothing here decides or rolls.
//!
//! Replace-don't-stack is unrepresentable-by-construction: one slot per
//! identity means applying an effect overwrites its slot, never appends. Ice is
//! a single slot holding [`IceStatus::Iced`] xor [`IceStatus::Frozen`], so the
//! two are mutually exclusive by shape. This mirrors the
//! [`crate::components::class::ClassSet`] total-membership ↔ `Vec` pattern: the
//! wire form is an array of [`ActiveEffect`], a duplicated identity is a parse
//! error, and the empty array is the legal [`ActiveEffects::EMPTY`] set.

use core::num::NonZeroU8;

use serde::{Deserialize, Serialize};

use crate::components::units::{Tick, Ticks};

/// The seven timed-effect identities — the discriminator every slot, event, and
/// dedup check keys off. Serialized as a bare `snake_case` string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectIdentity {
    /// DK Defense buff — incoming damage halved.
    Defense,
    /// Elf Greater Damage buff — energy-scaled physical-damage bonus.
    GreaterDamage,
    /// Elf Greater Defense buff — energy-scaled defense bonus.
    GreaterDefense,
    /// Poison damage-over-time.
    Poisoned,
    /// Iced — movement slowed.
    Iced,
    /// Frozen — movement stopped.
    Frozen,
    /// Defense reduced.
    DefenseReduction,
}

/// A buff magnitude paired with its absolute expiry tick — the stored form of an
/// energy-scaled Greater Damage / Greater Defense buff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedBonus {
    /// The resolved magnitude added to the profile.
    pub amount: u16,
    /// The tick the buff expires at.
    pub expiry: Tick,
}

/// The single ice slot's content: iced xor frozen. One slot means applying one
/// clears the other — the classic SubType-254 mutual exclusion, unrepresentable
/// as a stacked pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IceStatus {
    /// Iced: movement slowed until `expiry`.
    Iced {
        /// The tick the ice expires at.
        expiry: Tick,
    },
    /// Frozen: movement stopped until `expiry`.
    Frozen {
        /// The tick the freeze expires at.
        expiry: Tick,
    },
}

impl IceStatus {
    /// The tick this ice status expires at.
    #[must_use]
    pub fn expiry(self) -> Tick {
        match self {
            IceStatus::Iced { expiry } | IceStatus::Frozen { expiry } => expiry,
        }
    }

    /// Which identity this ice status is — Iced or Frozen.
    #[must_use]
    pub fn identity(self) -> EffectIdentity {
        match self {
            IceStatus::Iced { .. } => EffectIdentity::Iced,
            IceStatus::Frozen { .. } => EffectIdentity::Frozen,
        }
    }
}

/// The remaining poison tick counter: a positive count that governs
/// self-termination. Zero is unrepresentable — a poison with no ticks left is
/// removed, not held — so the counter can never gate an eighth tick. Serialized
/// as a bare integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct PoisonTicks(NonZeroU8);

impl PoisonTicks {
    /// The poison tick count at application: seven ticks, three seconds apart.
    // W-SRC: classic poison lasts ~20s at a 3s cadence — 7 damage ticks.
    pub const INITIAL: Self = match NonZeroU8::new(7) {
        Some(count) => Self(count),
        None => Self(NonZeroU8::MIN),
    };

    /// Builds a tick counter; zero is rejected.
    ///
    /// # Errors
    /// Returns [`EffectCountZero`] when `count` is zero.
    pub fn new(count: u8) -> Result<Self, EffectCountZero> {
        NonZeroU8::new(count).map(Self).ok_or(EffectCountZero)
    }

    /// The remaining tick count.
    #[must_use]
    pub fn get(self) -> u8 {
        self.0.get()
    }

    /// This counter after one tick fires: the next lower count, or `None` when
    /// the fired tick was the last (the counter reaches zero and the poison
    /// self-terminates).
    #[must_use]
    pub fn decrement(self) -> Option<Self> {
        NonZeroU8::new(self.0.get() - 1).map(Self)
    }
}

impl TryFrom<u8> for PoisonTicks {
    type Error = EffectCountZero;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<PoisonTicks> for u8 {
    fn from(ticks: PoisonTicks) -> Self {
        ticks.0.get()
    }
}

/// Poison damage-over-time state, governed by its remaining counter rather than
/// an expiry tick: a per-tick magnitude resolved once at application from the
/// caster, the ticks still to fire, the next tick they fire at, and the cadence
/// gap between ticks (stored so the advance never needs a tick length).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoisonDot {
    /// Damage each tick deals — resolved once from the caster's energy.
    pub per_tick_damage: u32,
    /// Ticks still to fire.
    pub remaining: PoisonTicks,
    /// The tick the next damage fires at.
    pub next_tick: Tick,
    /// The cadence gap between consecutive ticks, in whole ticks.
    pub cadence: Ticks,
}

/// One active timed effect as a flat kind-tagged value — the wire element, the
/// iteration element, and the `EffectApplied` payload. Reconstructed from the
/// [`ActiveEffects`] slots and mapped back into them, mirroring
/// [`crate::components::class::ClassSet`]'s `Vec<CharacterClass>` element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActiveEffect {
    /// DK Defense buff active until `expiry`.
    Defense {
        /// The tick the buff expires at.
        expiry: Tick,
    },
    /// Greater Damage buff of `amount` active until `expiry`.
    GreaterDamage {
        /// The resolved magnitude.
        amount: u16,
        /// The tick the buff expires at.
        expiry: Tick,
    },
    /// Greater Defense buff of `amount` active until `expiry`.
    GreaterDefense {
        /// The resolved magnitude.
        amount: u16,
        /// The tick the buff expires at.
        expiry: Tick,
    },
    /// Poison damage-over-time.
    Poisoned {
        /// Damage each tick deals.
        per_tick_damage: u32,
        /// Ticks still to fire.
        remaining: PoisonTicks,
        /// The tick the next damage fires at.
        next_tick: Tick,
        /// The cadence gap between consecutive ticks, in whole ticks.
        cadence: Ticks,
    },
    /// Iced — movement slowed until `expiry`.
    Iced {
        /// The tick the ice expires at.
        expiry: Tick,
    },
    /// Frozen — movement stopped until `expiry`.
    Frozen {
        /// The tick the freeze expires at.
        expiry: Tick,
    },
    /// Defense reduced until `expiry`.
    DefenseReduction {
        /// The tick the reduction expires at.
        expiry: Tick,
    },
}

impl ActiveEffect {
    /// The identity this effect occupies a slot under.
    #[must_use]
    pub fn identity(self) -> EffectIdentity {
        match self {
            ActiveEffect::Defense { .. } => EffectIdentity::Defense,
            ActiveEffect::GreaterDamage { .. } => EffectIdentity::GreaterDamage,
            ActiveEffect::GreaterDefense { .. } => EffectIdentity::GreaterDefense,
            ActiveEffect::Poisoned { .. } => EffectIdentity::Poisoned,
            ActiveEffect::Iced { .. } => EffectIdentity::Iced,
            ActiveEffect::Frozen { .. } => EffectIdentity::Frozen,
            ActiveEffect::DefenseReduction { .. } => EffectIdentity::DefenseReduction,
        }
    }
}

/// The live timed-effect store: one named optional slot per identity, so an
/// effect is always present at most once and applying it replaces rather than
/// stacks. Ice is a single slot holding iced xor frozen. Wire form: an array of
/// [`ActiveEffect`]; a duplicated identity is a parse error; the empty array is
/// [`Self::EMPTY`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<ActiveEffect>", into = "Vec<ActiveEffect>")]
pub struct ActiveEffects {
    defense: Option<Tick>,
    defense_reduction: Option<Tick>,
    greater_damage: Option<TimedBonus>,
    greater_defense: Option<TimedBonus>,
    ice: Option<IceStatus>,
    poison: Option<PoisonDot>,
}

impl ActiveEffects {
    /// The empty store — no effect active. The seed a fresh entity carries and
    /// the clear-on-death value. A real domain value (an entity with no timed
    /// effects), not a fabricated default.
    pub const EMPTY: Self = Self {
        defense: None,
        defense_reduction: None,
        greater_damage: None,
        greater_defense: None,
        ice: None,
        poison: None,
    };

    /// [`Self::EMPTY`] as a named constructor — the serde `default` seed for a
    /// legacy or freshly created entity whose record omits the effect array.
    #[must_use]
    pub const fn empty() -> Self {
        Self::EMPTY
    }

    /// The DK Defense buff slot's expiry, if active.
    #[must_use]
    pub fn defense(self) -> Option<Tick> {
        self.defense
    }

    /// The Defense-reduction ailment slot's expiry, if active.
    #[must_use]
    pub fn defense_reduction(self) -> Option<Tick> {
        self.defense_reduction
    }

    /// The Greater Damage buff slot, if active.
    #[must_use]
    pub fn greater_damage(self) -> Option<TimedBonus> {
        self.greater_damage
    }

    /// The Greater Defense buff slot, if active.
    #[must_use]
    pub fn greater_defense(self) -> Option<TimedBonus> {
        self.greater_defense
    }

    /// The ice slot (iced xor frozen), if active.
    #[must_use]
    pub fn ice(self) -> Option<IceStatus> {
        self.ice
    }

    /// The poison slot, if active.
    #[must_use]
    pub fn poison(self) -> Option<PoisonDot> {
        self.poison
    }

    /// This store with `effect` applied to its slot — replace-don't-stack: the
    /// identity's slot is overwritten, never appended. Applying an ice status
    /// clears whichever ice status was there (they share one slot). Total over
    /// the seven-variant [`ActiveEffect`].
    #[must_use]
    pub fn with(self, effect: ActiveEffect) -> Self {
        match effect {
            ActiveEffect::Defense { expiry } => Self {
                defense: Some(expiry),
                ..self
            },
            ActiveEffect::GreaterDamage { amount, expiry } => Self {
                greater_damage: Some(TimedBonus { amount, expiry }),
                ..self
            },
            ActiveEffect::GreaterDefense { amount, expiry } => Self {
                greater_defense: Some(TimedBonus { amount, expiry }),
                ..self
            },
            ActiveEffect::Poisoned {
                per_tick_damage,
                remaining,
                next_tick,
                cadence,
            } => Self {
                poison: Some(PoisonDot {
                    per_tick_damage,
                    remaining,
                    next_tick,
                    cadence,
                }),
                ..self
            },
            ActiveEffect::Iced { expiry } => Self {
                ice: Some(IceStatus::Iced { expiry }),
                ..self
            },
            ActiveEffect::Frozen { expiry } => Self {
                ice: Some(IceStatus::Frozen { expiry }),
                ..self
            },
            ActiveEffect::DefenseReduction { expiry } => Self {
                defense_reduction: Some(expiry),
                ..self
            },
        }
    }

    /// This store with `identity`'s slot cleared. Clearing either ice identity
    /// empties the shared ice slot. Total over the seven identities.
    #[must_use]
    pub fn without(self, identity: EffectIdentity) -> Self {
        match identity {
            EffectIdentity::Defense => Self {
                defense: None,
                ..self
            },
            EffectIdentity::GreaterDamage => Self {
                greater_damage: None,
                ..self
            },
            EffectIdentity::GreaterDefense => Self {
                greater_defense: None,
                ..self
            },
            EffectIdentity::Poisoned => Self {
                poison: None,
                ..self
            },
            EffectIdentity::Iced | EffectIdentity::Frozen => Self { ice: None, ..self },
            EffectIdentity::DefenseReduction => Self {
                defense_reduction: None,
                ..self
            },
        }
    }

    /// The active effects as a flat list, in slot-declaration order — the wire
    /// element sequence and the fold-iteration order.
    #[must_use]
    pub fn active(self) -> Vec<ActiveEffect> {
        let mut effects = Vec::new();
        if let Some(expiry) = self.defense {
            effects.push(ActiveEffect::Defense { expiry });
        }
        if let Some(expiry) = self.defense_reduction {
            effects.push(ActiveEffect::DefenseReduction { expiry });
        }
        if let Some(bonus) = self.greater_damage {
            effects.push(ActiveEffect::GreaterDamage {
                amount: bonus.amount,
                expiry: bonus.expiry,
            });
        }
        if let Some(bonus) = self.greater_defense {
            effects.push(ActiveEffect::GreaterDefense {
                amount: bonus.amount,
                expiry: bonus.expiry,
            });
        }
        if let Some(ice) = self.ice {
            effects.push(match ice {
                IceStatus::Iced { expiry } => ActiveEffect::Iced { expiry },
                IceStatus::Frozen { expiry } => ActiveEffect::Frozen { expiry },
            });
        }
        if let Some(poison) = self.poison {
            effects.push(ActiveEffect::Poisoned {
                per_tick_damage: poison.per_tick_damage,
                remaining: poison.remaining,
                next_tick: poison.next_tick,
                cadence: poison.cadence,
            });
        }
        effects
    }
}

impl TryFrom<Vec<ActiveEffect>> for ActiveEffects {
    type Error = DuplicateEffect;

    fn try_from(effects: Vec<ActiveEffect>) -> Result<Self, Self::Error> {
        let mut store = Self::EMPTY;
        for effect in effects {
            let identity = effect.identity();
            if store.occupies(identity) {
                return Err(DuplicateEffect(identity));
            }
            store = store.with(effect);
        }
        Ok(store)
    }
}

impl ActiveEffects {
    /// Whether the slot `identity` maps to is already filled — the dedup guard
    /// the wire parse keys off. Iced and Frozen share the one ice slot, so
    /// either occupies it.
    fn occupies(self, identity: EffectIdentity) -> bool {
        match identity {
            EffectIdentity::Defense => self.defense.is_some(),
            EffectIdentity::GreaterDamage => self.greater_damage.is_some(),
            EffectIdentity::GreaterDefense => self.greater_defense.is_some(),
            EffectIdentity::Poisoned => self.poison.is_some(),
            EffectIdentity::Iced | EffectIdentity::Frozen => self.ice.is_some(),
            EffectIdentity::DefenseReduction => self.defense_reduction.is_some(),
        }
    }
}

impl From<ActiveEffects> for Vec<ActiveEffect> {
    fn from(effects: ActiveEffects) -> Self {
        effects.active()
    }
}

/// Parse failure: an identity's slot listed more than once in an effect array
/// (both ice identities count as the one ice slot).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicateEffect(
    /// The identity that appeared more than once.
    pub EffectIdentity,
);

impl core::fmt::Display for DuplicateEffect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "effect identity listed more than once: {:?}", self.0)
    }
}

impl core::error::Error for DuplicateEffect {}

/// Rejection of a zero timed-effect count at the data-load boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectCountZero;

impl core::fmt::Display for EffectCountZero {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "timed-effect tick count must be at least 1")
    }
}

impl core::error::Error for EffectCountZero {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_has_no_slots_filled() {
        let empty = ActiveEffects::EMPTY;
        assert_eq!(empty.defense(), None);
        assert_eq!(empty.greater_damage(), None);
        assert_eq!(empty.ice(), None);
        assert_eq!(empty.poison(), None);
        assert_eq!(empty.active(), Vec::new());
        assert_eq!(ActiveEffects::empty(), empty);
    }

    #[test]
    fn poison_ticks_initial_is_seven_and_self_terminates() {
        let mut ticks = PoisonTicks::INITIAL;
        assert_eq!(ticks.get(), 7);
        let mut fired = 0;
        loop {
            fired += 1;
            match ticks.decrement() {
                Some(less) => ticks = less,
                None => break,
            }
        }
        assert_eq!(fired, 7, "exactly seven ticks fire");
    }

    #[test]
    fn poison_ticks_rejects_zero_on_the_wire() {
        assert!(PoisonTicks::new(0).is_err());
        assert!(serde_json::from_str::<PoisonTicks>("0").is_err());
        assert_eq!(serde_json::from_str::<PoisonTicks>("7").unwrap().get(), 7);
    }

    #[test]
    fn with_replaces_a_slot_rather_than_stacking() {
        let store = ActiveEffects::EMPTY
            .with(ActiveEffect::GreaterDamage {
                amount: 5,
                expiry: Tick(100),
            })
            .with(ActiveEffect::GreaterDamage {
                amount: 9,
                expiry: Tick(200),
            });
        assert_eq!(
            store.greater_damage(),
            Some(TimedBonus {
                amount: 9,
                expiry: Tick(200),
            })
        );
        // Exactly one Greater Damage effect — replace, never append.
        assert_eq!(store.active().len(), 1);
    }

    #[test]
    fn ice_slot_is_mutually_exclusive() {
        let iced = ActiveEffects::EMPTY.with(ActiveEffect::Iced { expiry: Tick(50) });
        assert_eq!(iced.ice(), Some(IceStatus::Iced { expiry: Tick(50) }));
        // Applying Frozen clears the Iced status — one shared slot.
        let frozen = iced.with(ActiveEffect::Frozen { expiry: Tick(80) });
        assert_eq!(frozen.ice(), Some(IceStatus::Frozen { expiry: Tick(80) }));
        assert_eq!(frozen.active().len(), 1);
    }

    #[test]
    fn without_clears_the_slot() {
        let store = ActiveEffects::EMPTY.with(ActiveEffect::Defense { expiry: Tick(40) });
        assert_eq!(store.defense(), Some(Tick(40)));
        let cleared = store.without(EffectIdentity::Defense);
        assert_eq!(cleared.defense(), None);
        // Clearing either ice identity empties the shared slot.
        let icy = ActiveEffects::EMPTY.with(ActiveEffect::Frozen { expiry: Tick(9) });
        assert_eq!(icy.without(EffectIdentity::Iced).ice(), None);
    }

    #[test]
    fn wire_round_trips_as_an_effect_array() {
        let store = ActiveEffects::EMPTY
            .with(ActiveEffect::Defense { expiry: Tick(40) })
            .with(ActiveEffect::Poisoned {
                per_tick_damage: 12,
                remaining: PoisonTicks::INITIAL,
                next_tick: Tick(60),
                cadence: Ticks(60),
            });
        let json = serde_json::to_string(&store).unwrap();
        assert_eq!(
            json,
            r#"[{"kind":"defense","expiry":40},{"kind":"poisoned","per_tick_damage":12,"remaining":7,"next_tick":60,"cadence":60}]"#
        );
        assert_eq!(serde_json::from_str::<ActiveEffects>(&json).unwrap(), store);
    }

    #[test]
    fn empty_wire_is_the_empty_array() {
        assert_eq!(serde_json::to_string(&ActiveEffects::EMPTY).unwrap(), "[]");
        assert_eq!(
            serde_json::from_str::<ActiveEffects>("[]").unwrap(),
            ActiveEffects::EMPTY
        );
    }

    #[test]
    fn wire_rejects_a_duplicate_identity() {
        let json = r#"[{"kind":"defense","expiry":40},{"kind":"defense","expiry":90}]"#;
        assert!(serde_json::from_str::<ActiveEffects>(json).is_err());
    }

    #[test]
    fn wire_rejects_two_ice_statuses_as_a_duplicate_slot() {
        let json = r#"[{"kind":"iced","expiry":40},{"kind":"frozen","expiry":90}]"#;
        assert!(serde_json::from_str::<ActiveEffects>(json).is_err());
        assert_eq!(
            ActiveEffects::try_from(vec![
                ActiveEffect::Iced { expiry: Tick(40) },
                ActiveEffect::Frozen { expiry: Tick(90) },
            ]),
            Err(DuplicateEffect(EffectIdentity::Frozen))
        );
    }

    #[test]
    fn effect_identity_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&EffectIdentity::DefenseReduction).unwrap(),
            r#""defense_reduction""#
        );
    }
}
