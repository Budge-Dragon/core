//! Timed-effect resolution: applying a buff or ailment (resolving its magnitude
//! and absolute expiry once, from the caster where it scales off one), advancing
//! a store one instant forward (expiring due slots and running the poison
//! damage-over-time), and deriving the transient contributions an effect feeds
//! into combat and movement. Pure and deterministic: application and advance
//! draw ZERO randomness and read no clock — the tick comes in, all millisecond
//! durations are converted to absolute ticks at application, and the advance
//! only compares stored ticks against `now`.

use crate::components::active_effect::{
    ActiveEffect, ActiveEffects, EffectIdentity, IceStatus, PoisonDot, PoisonTicks,
};
use crate::components::bonus::CombatBonus;
use crate::components::movement::{Mobility, SlowRatio};
use crate::components::pool::Pool;
use crate::components::units::{DurationMs, Percent, Tick, TickDuration};
use crate::data::effects::Ailment;
use crate::events::combat::Damage;
use crate::events::effect::EffectEvent;
use crate::services::ratio::{nonzero, scale_ratio};

// W-SRC: status-effect durations (OpenMU SubType timers, MagicEffectDefinition
// initializers). Iced 10s and Frozen 5s are the Ice-Arrow / freeze timers;
// Frozen (Ice Arrow) is an S6 backport and DefenseReduction has no identified
// pre-S3 inflicting skill — both are deliberate design inclusions, not
// era-authentic pre-S3 mechanics.
/// Iced (movement-slow) duration — 10s.
const ICED_MS: DurationMs = DurationMs(10_000);
/// Frozen (movement-stop) duration — 5s (S6 Ice-Arrow form).
const FROZEN_MS: DurationMs = DurationMs(5_000);
/// Defense-reduction ailment duration.
const DEFENSE_REDUCTION_MS: DurationMs = DurationMs(10_000);
/// DK Defense buff duration.
const DEFENSE_MS: DurationMs = DurationMs(4_000);
/// Greater Damage / Greater Defense buff duration.
const GREATER_MS: DurationMs = DurationMs(60_000);
/// Poison cadence: one damage tick every three seconds.
const POISON_CADENCE_MS: DurationMs = DurationMs(3_000);

// W-SRC: energy-scaled buff magnitudes, source-verified against OpenMU's Elf
// buff initializers (GreaterDamageEffectInitializer / GreaterDefenseEffect-
// Initializer): Greater Damage +(3 + Energy/7), Greater Defense +(2 + Energy/8),
// each 60s and StopByDeath. The DK Defense ×1/2 incoming-damage form is OpenMU's
// S6-era model, adopted deliberately (unconfirmed pre-S3).
/// Greater Damage flat base, before the energy term: `3 + Energy/7`.
const GREATER_DAMAGE_BASE: u32 = 3;
/// Greater Damage energy divisor: `+ Energy / 7`.
const GREATER_DAMAGE_ENERGY_DEN: u32 = 7;
/// Greater Defense flat base, before the energy term: `2 + Energy/8`.
const GREATER_DEFENSE_BASE: u32 = 2;
/// Greater Defense energy divisor: `+ Energy / 8`.
const GREATER_DEFENSE_ENERGY_DEN: u32 = 8;
/// DK Defense buff: incoming damage reduced by this percentage (×1/2). S6-era
/// form, adopted deliberately.
const DEFENSE_INCOMING_REDUCTION_POINTS: u8 = 50;

// Poison per-tick damage is CALIBRATED against direct wizardry DPS. It scales off
// the CASTER's energy (`base + Energy × num/den`), NOT the target's HP, so a weak
// caster's poison is weak and a strong caster's can kill — a deliberate modern
// deviation from the authentic 3%-of-current-HP model (self-limiting, never
// kills).
//
// Calibration: a direct wizardry cast deals [Energy/9, Energy/4] (the wizard
// wizardry span). Poison per tick = `1 + Energy/9` ≈ ONE direct cast's MINIMUM.
// A poison session is 6 ticks over ~20s (3s cadence) ≈ 6·Energy/9 = 2·Energy/3
// total — about 8/3 of a single max cast, spread over 20s. Sustained direct
// casting lands far more than 6 hits in 20s, so poison is meaningful supplementary
// pressure yet well below direct throughput. These numbers are calibrated, not
// playtested — further tuning is normal live-balance work.
/// Poison per-tick flat base — kept ≥ 1 so every tick deals at least one damage.
const POISON_BASE: u32 = 1;
/// Poison energy-term numerator.
const POISON_ENERGY_NUM: u32 = 1;
/// Poison energy-term divisor (`+ Energy / 9`, ≈ a direct cast's minimum).
const POISON_ENERGY_DEN: u32 = 9;

/// The buffs this wave resolves to a timed effect — the applicable subset of
/// [`crate::data::effects::Buff`]. The rest (Soul Barrier, Swell Life, …) are
/// deferred, so they are unrepresentable here. [`apply_buff`] is exhaustive over
/// these three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicableBuff {
    /// DK Defense — incoming damage halved (no energy magnitude).
    Defense,
    /// Elf Greater Damage — energy-scaled physical-damage bonus.
    GreaterDamage,
    /// Elf Greater Defense — energy-scaled defense bonus.
    GreaterDefense,
}

/// Applies a buff to a store, resolving its magnitude from the caster's energy
/// (where it scales) and its absolute expiry from the fixed duration, then
/// slot-assigning it (replace-don't-stack). Returns the updated store and the
/// resolved [`ActiveEffect`] — the `EffectApplied` payload. `caster_energy` is
/// read only by the energy-scaled arms; the Defense buff ignores it.
#[must_use]
pub fn apply_buff(
    buff: ApplicableBuff,
    caster_energy: u16,
    existing: ActiveEffects,
    now: Tick,
    tick: TickDuration,
) -> (ActiveEffects, ActiveEffect) {
    let effect = match buff {
        ApplicableBuff::Defense => ActiveEffect::Defense {
            expiry: now + DEFENSE_MS.in_ticks(tick),
        },
        ApplicableBuff::GreaterDamage => ActiveEffect::GreaterDamage {
            amount: greater_damage_magnitude(caster_energy),
            expiry: now + GREATER_MS.in_ticks(tick),
        },
        ApplicableBuff::GreaterDefense => ActiveEffect::GreaterDefense {
            amount: greater_defense_magnitude(caster_energy),
            expiry: now + GREATER_MS.in_ticks(tick),
        },
    };
    (existing.with(effect), effect)
}

/// Applies an ailment to a store, mapping the data [`Ailment`] to the
/// component-level effect. Poison resolves its per-tick damage from the caster's
/// energy (`1 + Energy/9`, caster-scaled) and starts its six-tick counter; the
/// status ailments resolve an absolute expiry from their fixed durations and
/// ignore the energy. Iced and Frozen share the one ice slot, so applying one
/// clears the other. Returns the updated store and the resolved [`ActiveEffect`].
#[must_use]
pub fn apply_ailment(
    ailment: Ailment,
    caster_energy: u16,
    existing: ActiveEffects,
    now: Tick,
    tick: TickDuration,
) -> (ActiveEffects, ActiveEffect) {
    let effect = match ailment {
        Ailment::Poisoned => {
            let cadence = POISON_CADENCE_MS.in_ticks(tick);
            ActiveEffect::Poisoned {
                per_tick_damage: poison_per_tick(caster_energy),
                remaining: PoisonTicks::INITIAL,
                next_tick: now + cadence,
                cadence,
            }
        }
        Ailment::Iced => ActiveEffect::Iced {
            expiry: now + ICED_MS.in_ticks(tick),
        },
        Ailment::Frozen => ActiveEffect::Frozen {
            expiry: now + FROZEN_MS.in_ticks(tick),
        },
        Ailment::DefenseReduction => ActiveEffect::DefenseReduction {
            expiry: now + DEFENSE_REDUCTION_MS.in_ticks(tick),
        },
    };
    (existing.with(effect), effect)
}

/// Advances a store to `now`: expires every timed slot whose expiry has been
/// reached (emitting `EffectExpired`), then runs the poison damage-over-time —
/// firing every tick due since the last advance, bounded by the poison counter
/// (never the now-gap). A poison tick that reaches zero health returns the
/// cleared store and zeroed pool with `PoisonKilled`; the final counter tick
/// emits `EffectExpired` alongside its `PoisonTick`. Draws no randomness and
/// reads no clock. Slot order is fixed (defense, defense-reduction,
/// greater-damage, greater-defense, ice, poison) so events are deterministically
/// ordered.
#[must_use]
pub fn advance_effects(
    effects: ActiveEffects,
    health: Pool,
    now: Tick,
) -> (ActiveEffects, Pool, Vec<EffectEvent>) {
    let mut events = Vec::new();
    let mut result = effects;
    result = expire_timed(
        result,
        effects.defense(),
        EffectIdentity::Defense,
        now,
        &mut events,
    );
    result = expire_timed(
        result,
        effects.defense_reduction(),
        EffectIdentity::DefenseReduction,
        now,
        &mut events,
    );
    result = expire_timed(
        result,
        effects.greater_damage().map(|bonus| bonus.expiry),
        EffectIdentity::GreaterDamage,
        now,
        &mut events,
    );
    result = expire_timed(
        result,
        effects.greater_defense().map(|bonus| bonus.expiry),
        EffectIdentity::GreaterDefense,
        now,
        &mut events,
    );
    result = match effects.ice() {
        None => result,
        Some(ice) => {
            if ice.expiry().reached(now) {
                events.push(EffectEvent::EffectExpired {
                    effect: ice.identity(),
                });
                result.without(ice.identity())
            } else {
                result
            }
        }
    };
    match effects.poison() {
        None => (result, health, events),
        Some(poison) => advance_poison(result, poison, health, now, events),
    }
}

/// Expires one timed slot: when its `expiry` is reached, clears the slot and
/// emits `EffectExpired`; otherwise leaves the store untouched.
fn expire_timed(
    result: ActiveEffects,
    expiry: Option<Tick>,
    identity: EffectIdentity,
    now: Tick,
    events: &mut Vec<EffectEvent>,
) -> ActiveEffects {
    match expiry {
        None => result,
        Some(expiry) => {
            if expiry.reached(now) {
                events.push(EffectEvent::EffectExpired { effect: identity });
                result.without(identity)
            } else {
                result
            }
        }
    }
}

/// Runs the poison damage-over-time forward to `now`. Fires each due tick in
/// turn — reducing health, then either killing (clearing every effect, no
/// further ticks), self-terminating on the last counter tick, or advancing the
/// counter and the next-tick schedule by the stored cadence. The loop is bounded
/// by the counter reaching zero, so a large `now`-gap never fires a seventh
/// tick.
fn advance_poison(
    result: ActiveEffects,
    poison: PoisonDot,
    health: Pool,
    now: Tick,
    mut events: Vec<EffectEvent>,
) -> (ActiveEffects, Pool, Vec<EffectEvent>) {
    let mut remaining = poison.remaining;
    let mut next_tick = poison.next_tick;
    let mut current = health;
    let damage = Damage(poison.per_tick_damage);
    while next_tick.reached(now) {
        current = current.reduced(poison.per_tick_damage);
        if current.current() == 0 {
            events.push(EffectEvent::PoisonKilled { damage });
            // Death clears every timed effect — the empty store is that value.
            return (ActiveEffects::EMPTY, current, events);
        }
        events.push(EffectEvent::PoisonTick { damage });
        match remaining.decrement() {
            None => {
                events.push(EffectEvent::EffectExpired {
                    effect: EffectIdentity::Poisoned,
                });
                return (result.without(EffectIdentity::Poisoned), current, events);
            }
            Some(less) => {
                remaining = less;
                next_tick = next_tick + poison.cadence;
            }
        }
    }
    let updated = result.with(ActiveEffect::Poisoned {
        per_tick_damage: poison.per_tick_damage,
        remaining,
        next_tick,
        cadence: poison.cadence,
    });
    (updated, current, events)
}

/// The transient combat contribution one active effect folds into a profile, or
/// `None` for effects that are movement, derivation, or damage-over-time rather
/// than additive profile bonuses.
#[must_use]
pub(crate) fn effect_bonus(effect: &ActiveEffect) -> Option<CombatBonus> {
    match effect {
        // W-SRC: Greater Damage (GreaterDamageEffectInitializer.cs) is a flat add
        // applied to outgoing damage AFTER defense subtraction — CombatBonus::Damage,
        // never a physical-span raise (which crit/excellent would then amplify).
        ActiveEffect::GreaterDamage { amount, .. } => Some(CombatBonus::Damage {
            amount: u32::from(*amount),
        }),
        ActiveEffect::GreaterDefense { amount, .. } => Some(CombatBonus::Defense {
            amount: u32::from(*amount),
        }),
        ActiveEffect::Defense { .. } => Some(CombatBonus::IncomingDamagePct {
            percent: Percent::clamped(u64::from(DEFENSE_INCOMING_REDUCTION_POINTS)),
        }),
        ActiveEffect::Poisoned { .. }
        | ActiveEffect::Iced { .. }
        | ActiveEffect::Frozen { .. }
        | ActiveEffect::DefenseReduction { .. } => None,
    }
}

/// The movement capability a store confers: Frozen stops movement, Iced confers
/// the half-speed slow ratio, and everything else leaves it free. The slow is a
/// ratio, not a resolved speed — the consuming movement service scales its own
/// base step speed by it, so this stays unaware of any base magnitude.
#[must_use]
pub fn mobility(effects: &ActiveEffects) -> Mobility {
    match effects.ice() {
        Some(IceStatus::Frozen { .. }) => Mobility::Immobilized,
        // W-SRC: Iced slows movement to ×1/2 of the base step speed.
        Some(IceStatus::Iced { .. }) => Mobility::Slowed {
            ratio: SlowRatio::HALVED,
        },
        None => Mobility::Free,
    }
}

fn greater_damage_magnitude(energy: u16) -> u16 {
    saturating_u16(GREATER_DAMAGE_BASE.saturating_add(scale_ratio(
        u32::from(energy),
        1,
        nonzero(GREATER_DAMAGE_ENERGY_DEN),
    )))
}

fn greater_defense_magnitude(energy: u16) -> u16 {
    saturating_u16(GREATER_DEFENSE_BASE.saturating_add(scale_ratio(
        u32::from(energy),
        1,
        nonzero(GREATER_DEFENSE_ENERGY_DEN),
    )))
}

/// Per-tick poison damage from the caster's energy — `base + Energy × num/den`
/// (`1 + Energy/9`, calibrated against direct wizardry DPS). Base is kept ≥ 1 so
/// every tick deals at least one damage.
fn poison_per_tick(energy: u16) -> u32 {
    POISON_BASE.saturating_add(scale_ratio(
        u32::from(energy),
        POISON_ENERGY_NUM,
        nonzero(POISON_ENERGY_DEN),
    ))
}

/// Saturating narrow of a resolved buff magnitude to the `u16` it is stored as.
fn saturating_u16(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::units::Ticks;

    fn tick50() -> TickDuration {
        TickDuration::new(50).unwrap()
    }

    #[test]
    fn greater_damage_scales_with_energy_and_carries_a_sixty_second_expiry() {
        let (store, effect) = apply_buff(
            ApplicableBuff::GreaterDamage,
            70,
            ActiveEffects::EMPTY,
            Tick(0),
            tick50(),
        );
        // 3 + 70/7 = 13; 60_000ms / 50ms = 1200 ticks.
        assert_eq!(
            effect,
            ActiveEffect::GreaterDamage {
                amount: 13,
                expiry: Tick(1200),
            }
        );
        assert_eq!(store.greater_damage().map(|bonus| bonus.amount), Some(13));
    }

    #[test]
    fn defense_buff_has_no_magnitude_and_a_four_second_expiry() {
        let (_, effect) = apply_buff(
            ApplicableBuff::Defense,
            999,
            ActiveEffects::EMPTY,
            Tick(0),
            tick50(),
        );
        // 4_000ms / 50ms = 80 ticks; energy is ignored for Defense.
        assert_eq!(effect, ActiveEffect::Defense { expiry: Tick(80) });
        assert_eq!(
            effect_bonus(&effect),
            Some(CombatBonus::IncomingDamagePct {
                percent: Percent::clamped(50),
            })
        );
    }

    #[test]
    fn poison_per_tick_scales_off_the_caster_energy_not_the_target() {
        // ★POISON★: higher caster energy ⇒ strictly higher per-tick damage.
        let weak = poison_per_tick(10);
        let strong = poison_per_tick(300);
        assert!(strong > weak, "strong {strong} must exceed weak {weak}");
        // Base keeps a zero-energy caster's poison at ≥ 1.
        assert!(poison_per_tick(0) >= 1);
    }

    #[test]
    fn poison_apply_seeds_six_ticks_at_the_cadence() {
        let (store, effect) = apply_ailment(
            Ailment::Poisoned,
            70,
            ActiveEffects::EMPTY,
            Tick(0),
            tick50(),
        );
        // Cadence 3_000ms / 50ms = 60 ticks; first tick at now + 60.
        match effect {
            ActiveEffect::Poisoned {
                remaining,
                next_tick,
                cadence,
                ..
            } => {
                assert_eq!(remaining, PoisonTicks::INITIAL);
                assert_eq!(next_tick, Tick(60));
                assert_eq!(cadence, Ticks(60));
            }
            ActiveEffect::Defense { .. }
            | ActiveEffect::GreaterDamage { .. }
            | ActiveEffect::GreaterDefense { .. }
            | ActiveEffect::Iced { .. }
            | ActiveEffect::Frozen { .. }
            | ActiveEffect::DefenseReduction { .. } => panic!("expected poison"),
        }
        assert!(store.poison().is_some());
    }

    #[test]
    fn poison_deals_exactly_six_ticks_totaling_six_times_per_tick() {
        let energy = 70u16;
        let per_tick = poison_per_tick(energy);
        let big_pool = Pool::full(1_000_000);
        let (store, _) = apply_ailment(
            Ailment::Poisoned,
            energy,
            ActiveEffects::EMPTY,
            Tick(0),
            tick50(),
        );
        // A single advance far past every scheduled tick fires the whole stream.
        let (after, health, events) = advance_effects(store, big_pool, Tick(10_000));
        let ticks = events
            .iter()
            .filter(|event| matches!(event, EffectEvent::PoisonTick { .. }))
            .count();
        assert_eq!(ticks, 6, "exactly six ticks fire, never a seventh");
        assert_eq!(
            big_pool.current() - health.current(),
            per_tick.saturating_mul(6)
        );
        assert!(after.poison().is_none(), "poison self-terminates");
        assert!(events.iter().any(|event| matches!(
            event,
            EffectEvent::EffectExpired {
                effect: EffectIdentity::Poisoned
            }
        )));
    }

    #[test]
    fn a_strong_casters_poison_can_kill_a_low_hp_target() {
        let (store, _) = apply_ailment(
            Ailment::Poisoned,
            500,
            ActiveEffects::EMPTY,
            Tick(0),
            tick50(),
        );
        let per_tick = poison_per_tick(500);
        // A pool small enough that the first tick alone is lethal.
        let frail = Pool::full(per_tick - 1);
        let (after, health, events) = advance_effects(store, frail, Tick(10_000));
        assert_eq!(health.current(), 0);
        assert_eq!(after, ActiveEffects::EMPTY, "death clears every effect");
        assert!(
            events
                .iter()
                .any(|event| matches!(event, EffectEvent::PoisonKilled { .. }))
        );
        // No seventh-or-later tick: death stops the stream.
        let ticks = events
            .iter()
            .filter(|event| matches!(event, EffectEvent::PoisonTick { .. }))
            .count();
        assert_eq!(
            ticks, 0,
            "the lethal tick reports PoisonKilled, not PoisonTick"
        );
    }

    #[test]
    fn a_poison_death_clears_a_coexisting_non_poison_buff() {
        // Seed a still-active Greater Damage buff (expiry Tick(1200)) alongside a
        // lethal poison, then kill before that expiry (Tick(100) < 1200): only the
        // death-clear — not the buff's own expiry — can empty the store, which the
        // poison-only kill test cannot distinguish.
        let (buffed, _) = apply_buff(
            ApplicableBuff::GreaterDamage,
            70,
            ActiveEffects::EMPTY,
            Tick(0),
            tick50(),
        );
        let (store, _) = apply_ailment(Ailment::Poisoned, 500, buffed, Tick(0), tick50());
        assert!(
            store.greater_damage().is_some(),
            "the buff coexists with the poison before the lethal tick"
        );
        let frail = Pool::full(poison_per_tick(500) - 1);
        let (after, health, events) = advance_effects(store, frail, Tick(100));
        assert_eq!(health.current(), 0);
        assert_eq!(
            after,
            ActiveEffects::EMPTY,
            "a poison death clears every effect, the buff included"
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, EffectEvent::PoisonKilled { .. }))
        );
    }

    #[test]
    fn a_weak_casters_poison_does_strictly_less_total_than_a_strong_casters() {
        let big_pool = Pool::full(10_000_000);
        let total = |energy: u16| {
            let (store, _) = apply_ailment(
                Ailment::Poisoned,
                energy,
                ActiveEffects::EMPTY,
                Tick(0),
                tick50(),
            );
            let (_, health, _) = advance_effects(store, big_pool, Tick(10_000));
            big_pool.current() - health.current()
        };
        assert!(total(300) > total(10));
    }

    #[test]
    fn reapplying_a_buff_refreshes_rather_than_stacks() {
        let (store, _) = apply_buff(
            ApplicableBuff::GreaterDamage,
            70,
            ActiveEffects::EMPTY,
            Tick(0),
            tick50(),
        );
        let (store, _) = apply_buff(
            ApplicableBuff::GreaterDamage,
            70,
            store,
            Tick(100),
            tick50(),
        );
        // One slot, refreshed to the later expiry — never two stacked buffs.
        assert_eq!(store.active().len(), 1);
        assert_eq!(
            store.greater_damage().map(|bonus| bonus.expiry),
            Some(Tick(1300))
        );
    }

    #[test]
    fn timed_slots_expire_when_their_tick_is_reached() {
        let (store, _) = apply_buff(
            ApplicableBuff::Defense,
            0,
            ActiveEffects::EMPTY,
            Tick(0),
            tick50(),
        );
        // Expiry is Tick(80); one tick before, it survives.
        let (before, _, events) = advance_effects(store, Pool::full(10), Tick(79));
        assert!(before.defense().is_some());
        assert!(events.is_empty());
        // At the expiry tick it is removed with an EffectExpired.
        let (after, _, events) = advance_effects(store, Pool::full(10), Tick(80));
        assert!(after.defense().is_none());
        assert_eq!(
            events,
            vec![EffectEvent::EffectExpired {
                effect: EffectIdentity::Defense
            }]
        );
    }

    #[test]
    fn mobility_reads_the_ice_slot() {
        assert_eq!(mobility(&ActiveEffects::EMPTY), Mobility::Free);
        let iced = ActiveEffects::EMPTY.with(ActiveEffect::Iced { expiry: Tick(9) });
        assert_eq!(
            mobility(&iced),
            Mobility::Slowed {
                ratio: SlowRatio::HALVED
            }
        );
        let frozen = ActiveEffects::EMPTY.with(ActiveEffect::Frozen { expiry: Tick(9) });
        assert_eq!(mobility(&frozen), Mobility::Immobilized);
    }

    #[test]
    fn ailments_that_are_not_profile_bonuses_fold_to_nothing() {
        for effect in [
            ActiveEffect::Iced { expiry: Tick(1) },
            ActiveEffect::Frozen { expiry: Tick(1) },
            ActiveEffect::DefenseReduction { expiry: Tick(1) },
            ActiveEffect::Poisoned {
                per_tick_damage: 5,
                remaining: PoisonTicks::INITIAL,
                next_tick: Tick(1),
                cadence: Ticks(60),
            },
        ] {
            assert_eq!(effect_bonus(&effect), None);
        }
    }
}
