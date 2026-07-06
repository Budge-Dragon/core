//! Using a consumable: drink a potion, eat an apple, cure a poison. One pure
//! transition — a value in, a value out, no RNG, no clock — mirroring the
//! [`crate::services::inventory`] fold-the-component-`Result`-into-an-outcome
//! pattern. The intent is "consume the stack at `cell`", never a claimed heal
//! number; the service computes the authoritative magnitude itself.
//!
//! Heal and cure are pure integer/state transitions, so [`use_consumable`]
//! draws no randomness. The percent-of-max term scales through
//! [`crate::services::ratio`] (integer floor, no float); the flat term is a
//! saturating subtraction that decays to zero with level. A successful heal or
//! cure consumes exactly one piece; a refusal consumes nothing.

use crate::components::active_effect::EffectIdentity;
use crate::components::inventory::{Cell, Inventory};
use crate::components::life::LifeState;
use crate::components::units::Level;
use crate::components::vitals::Vitals;
use crate::data::atlas::Atlas;
use crate::data::item_definitions::{ConsumeEffect, HealingTier, ItemKind, ManaTier};
use crate::entities::character::Character;
use crate::events::consume::{ConsumeEvent, ConsumeRejection, PoolKind};
use crate::services::ratio::{nonzero, scale_ratio};

/// Consumes the stack covering `cell`, computing the authoritative outcome. A
/// dead character consumes nothing; an empty cell or a non-consumable is
/// refused; a recovery or cure that would change nothing is refused and
/// consumes nothing; an out-of-scope consumable (alcohol, town portal) is
/// refused. Otherwise the character is healed or cured, exactly one piece is
/// consumed, and the outcome is returned. The borrowed character is never
/// mutated — a fresh character value is returned.
#[must_use]
pub fn use_consumable(
    character: &Character,
    inventory: Inventory,
    cell: Cell,
    atlas: &Atlas,
) -> (Character, Inventory, Vec<ConsumeEvent>) {
    match character.life() {
        LifeState::Alive => {}
        LifeState::Dead { .. } => {
            return refuse(character, inventory, ConsumeRejection::NotAlive);
        }
    }
    let effect = match resolve_effect(&inventory, cell, atlas) {
        Ok(effect) => effect,
        Err(reason) => return refuse(character, inventory, reason),
    };
    match effect {
        ConsumeEffect::Healing { tier } => recover(
            character,
            inventory,
            cell,
            PoolKind::Health,
            healing_multiplier(tier),
        ),
        ConsumeEffect::Mana { tier } => recover(
            character,
            inventory,
            cell,
            PoolKind::Mana,
            mana_multiplier(tier),
        ),
        ConsumeEffect::Antidote => cure(character, inventory, cell),
        ConsumeEffect::Alcohol | ConsumeEffect::TownPortal => {
            refuse(character, inventory, ConsumeRejection::NotRecoverable)
        }
    }
}

/// The refusal outcome: the untouched inventory and a single `Rejected` event.
fn refuse(
    character: &Character,
    inventory: Inventory,
    reason: ConsumeRejection,
) -> (Character, Inventory, Vec<ConsumeEvent>) {
    (
        character.clone(),
        inventory,
        vec![ConsumeEvent::Rejected { reason }],
    )
}

/// The consume effect of the item covering `cell`, or the refusal a wrong cell
/// or a non-consumable earns. An occupant the atlas cannot identify is nothing
/// this service can drink — the equip/sell services' unknown-occupant fold.
fn resolve_effect(
    inventory: &Inventory,
    cell: Cell,
    atlas: &Atlas,
) -> Result<ConsumeEffect, ConsumeRejection> {
    let Some(occupant) = inventory.occupant(cell) else {
        return Err(ConsumeRejection::NoItem);
    };
    let Some(def) = atlas.item(occupant.item.item) else {
        return Err(ConsumeRejection::NotConsumable);
    };
    match &def.kind {
        ItemKind::Consumable { effect } => Ok(*effect),
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
        | ItemKind::Staff { .. }
        | ItemKind::Shield { .. }
        | ItemKind::Helm { .. }
        | ItemKind::BodyArmor { .. }
        | ItemKind::Pants { .. }
        | ItemKind::Gloves { .. }
        | ItemKind::Boots { .. }
        | ItemKind::Wings { .. }
        | ItemKind::Pet { .. }
        | ItemKind::Ring { .. }
        | ItemKind::Pendant { .. }
        | ItemKind::TransformationRing { .. }
        | ItemKind::Orb { .. }
        | ItemKind::SkillScroll { .. }
        | ItemKind::Jewel { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => Err(ConsumeRejection::NotConsumable),
    }
}

/// Recovers `pool` by the tier's percent-of-max plus its level-decaying flat
/// term, capped at the pool's own maximum. A zero post-cap delta (a full pool or
/// a zero-magnitude amount) refuses and consumes nothing; otherwise the reseated
/// character is committed and one piece consumed.
fn recover(
    character: &Character,
    inventory: Inventory,
    cell: Cell,
    pool: PoolKind,
    multiplier: u32,
) -> (Character, Inventory, Vec<ConsumeEvent>) {
    let vitals = character.vitals();
    let current = match pool {
        PoolKind::Health => vitals.health,
        PoolKind::Mana => vitals.mana,
    };
    let amount = recovery_amount(current.max(), character.level(), multiplier);
    let raised = current.restored(amount);
    let restored = raised.current().saturating_sub(current.current());
    if restored == 0 {
        return refuse(character, inventory, ConsumeRejection::NoEffect);
    }
    let vitals = match pool {
        PoolKind::Health => Vitals {
            health: raised,
            ..vitals
        },
        PoolKind::Mana => Vitals {
            mana: raised,
            ..vitals
        },
    };
    commit(
        character,
        character.with_vitals(vitals),
        inventory,
        cell,
        ConsumeEvent::Recovered { pool, restored },
    )
}

/// Cures an active poison. An antidote met by no poison would change nothing, so
/// it refuses and consumes nothing; otherwise the cured character is committed
/// and one antidote consumed.
fn cure(
    character: &Character,
    inventory: Inventory,
    cell: Cell,
) -> (Character, Inventory, Vec<ConsumeEvent>) {
    if character.active_effects().poison().is_none() {
        return refuse(character, inventory, ConsumeRejection::NoEffect);
    }
    let effects = character.active_effects().without(EffectIdentity::Poisoned);
    commit(
        character,
        character.with_effects(effects),
        inventory,
        cell,
        ConsumeEvent::PoisonCured,
    )
}

/// Consumes one piece for a successful heal or cure and returns the applied
/// character with its event. The single consume seam: `resolve_effect` already
/// proved the cell covered, so `consume_one`'s rejection arm is structurally
/// unreachable — folding it keeps the transition total without a panic (the
/// shop-sell fold).
fn commit(
    original: &Character,
    applied: Character,
    inventory: Inventory,
    cell: Cell,
    event: ConsumeEvent,
) -> (Character, Inventory, Vec<ConsumeEvent>) {
    match inventory.consume_one(cell) {
        Ok(inventory) => (applied, inventory, vec![event]),
        Err((inventory, _reason)) => refuse(original, inventory, ConsumeRejection::NoItem),
    }
}

/// The authentic recovery magnitude: `max * (multiplier * 10)% + max(0,
/// (multiplier + 1) * 50 - level)`, integer-only. The percent term floors
/// through [`scale_ratio`]; the flat term decays to zero once level reaches it.
fn recovery_amount(max: u32, level: Level, multiplier: u32) -> u32 {
    let base_percent = multiplier.saturating_mul(10);
    let additional_base = multiplier.saturating_add(1).saturating_mul(50);
    let scaled = scale_ratio(max, base_percent, nonzero(100));
    let flat = additional_base.saturating_sub(u32::from(level.get()));
    scaled.saturating_add(flat)
}

/// The recovery multiplier of a healing tier (Apple weakest). `base_percent =
/// multiplier * 10`, `additional_base = (multiplier + 1) * 50`.
fn healing_multiplier(tier: HealingTier) -> u32 {
    match tier {
        HealingTier::Apple => 0,
        HealingTier::Small => 1,
        HealingTier::Medium => 2,
        HealingTier::Large => 3,
    }
}

/// The recovery multiplier of a mana tier — the healing ladder without the
/// apple rung.
fn mana_multiplier(tier: ManaTier) -> u32 {
    match tier {
        ManaTier::Small => 1,
        ManaTier::Medium => 2,
        ManaTier::Large => 3,
    }
}
