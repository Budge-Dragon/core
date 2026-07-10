//! Combat durability wear: the pure service that grinds worn equipment down
//! after a strike. Composed AFTER [`resolve_attack`] so the strike's own RNG
//! draw count is untouched; [`resolve_strike_with_wear`] is the single
//! composition that owns the strike→wear RNG order as a core contract — a
//! host calls it, never the two steps separately, so no host can diverge the
//! stream. Wear amounts ride each item's persisted
//! [`crate::components::item_instance::WearLedger`], so a sub-divisor hit
//! never floors to nothing and the carried remainder survives the persist
//! seam. A miss or a fully-reduced (0-damage) hit wears only ammunition;
//! poison, reflect, and every non-strike pathway never reach this module.

use core::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use rand_core::RngCore;

use crate::components::collections::OneOrMore;
use crate::components::combat_profile::CombatProfile;
use crate::components::equipment::{Equipment, EquipmentSlot};
use crate::components::item_instance::{Durability, ItemInstance};
use crate::components::pool::Pool;
use crate::data::atlas::Atlas;
use crate::data::item_definitions::ItemKind;
use crate::events::combat::AttackOutcome;
use crate::services::chance::pick_one;
use crate::services::combat::{StrikeBasis, resolve_attack};
use crate::services::ratio::nonzero;

// W-SRC: the three wear divisors are OpenMU's era-uniform tuning knobs
// (GameConfigurationInitializerBase.cs:61-63), inherited unchanged by 0.75,
// 0.95d, and S6 — adopted, ledgered under CMB-CONST. The defensive pool wears
// damage-proportionally (HealthDamage / 2000, Player.cs:2739-2743 +
// DataModel/ItemExtensions.cs:172-177), the offensive pool a FLAT 1/10000 per
// landed damaging hit (Player.cs:2792), the pet additionally at
// HealthDamage / 100000 outside the random pool (Player.cs:2745-2747,2771).
/// Defensive-pool wear: health-damage units per durability point.
const DEFENSIVE_WEAR_DIVISOR: u32 = 2000;
/// Offensive-pool wear: landed damaging hits per durability point.
const OFFENSIVE_WEAR_DIVISOR: u32 = 10_000;
/// Pet wear: health-damage units per durability point (50× slower than
/// armor).
const PET_WEAR_DIVISOR: u32 = 100_000;
/// The offensive pool's flat per-hit ledger advance.
const ONE_HIT: u32 = 1;

/// One durability-wear outcome of a strike — a plain domain value the host
/// routes outward (log/packet/table). Kind-tagged, minimum payload; events
/// stay attributed by side in [`StrikeWear`], so no side field exists here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WearEvent {
    /// A worn item accumulated wear and is still functional; `durability` is
    /// the new gauge (its ledger may have advanced without crossing a point).
    Worn {
        /// The slot whose occupant wore.
        slot: EquipmentSlot,
        /// The occupant's gauge after the wear.
        durability: Durability,
    },
    /// A gear item (armor/weapon) ground to durability 0 — contribution off,
    /// STILL worn, repairable. Not removed from its slot.
    Broken {
        /// The slot whose occupant broke.
        slot: EquipmentSlot,
    },
    /// Ammunition emptied, or a non-trainable pet at 0 — removed from the
    /// slot.
    Destroyed {
        /// The slot that was emptied.
        slot: EquipmentSlot,
    },
}

/// Both fighters' worn sets and wear events after a strike — the wear
/// composition's output (a return value bundling the two sides, not a
/// parameter bundle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrikeWear {
    /// The attacker's worn set after ammunition consumption and weapon wear.
    pub attacker_worn: Equipment,
    /// The defender's worn set after defensive and pet wear.
    pub defender_worn: Equipment,
    /// The attacker-side wear events, in occurrence order.
    pub attacker_events: Vec<WearEvent>,
    /// The defender-side wear events, in occurrence order.
    pub defender_events: Vec<WearEvent>,
}

/// One authoritative strike WITH durability wear, in the fixed RNG order the
/// replay contract pins: the strike's own draws (unchanged from the shipped
/// resolver), then — only on a landed damaging hit — the defender pool pick,
/// the pet (no RNG), and the attacker pool pick. Calls [`resolve_attack`]
/// (which never sees Equipment), then wears both sets. A monster side passes
/// [`Equipment::empty`] — empty pools draw zero words. The host calls THIS,
/// never the two steps separately, so no host can diverge the stream.
#[must_use]
pub fn resolve_strike_with_wear(
    attacker: &CombatProfile,
    attacker_worn: Equipment,
    target: &CombatProfile,
    target_worn: Equipment,
    target_health: Pool,
    basis: &StrikeBasis,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> (Pool, AttackOutcome, StrikeWear) {
    let (health, outcome) = resolve_attack(attacker, target, target_health, basis, rng);
    let wear = wear_from_strike(&outcome, attacker_worn, target_worn, atlas, rng);
    (health, outcome, wear)
}

/// Wears both sets from a resolved strike. Ammunition is spent first (no RNG,
/// every swing hit-or-miss); then — only when the outcome carries real
/// health damage — the defender's random defensive pick, the pet (no RNG), and
/// the attacker's random offensive pick, in that fixed order. A miss or a
/// fully-reduced (0-damage) hit wears only ammo; the `NonZeroU32` fold makes
/// that structural, never a guard.
#[must_use]
pub fn wear_from_strike(
    outcome: &AttackOutcome,
    attacker_worn: Equipment,
    defender_worn: Equipment,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> StrikeWear {
    let health_damage = match outcome {
        AttackOutcome::Missed => 0,
        AttackOutcome::Landed { hit } | AttackOutcome::Killed { hit } => hit.damage.0,
    };
    // W-SRC: every swing consumes one round, hit or miss
    // (AttackableExtensions.cs:495-507).
    let (attacker_worn, mut attacker_events) = ammo_consume(attacker_worn, atlas);
    let (defender_events, defender_worn, attacker_worn) = match NonZeroU32::new(health_damage) {
        None => (Vec::new(), defender_worn, attacker_worn),
        Some(damage) => {
            let (defender_worn, mut defender_events) =
                defender_wear(defender_worn, damage.get(), atlas, rng);
            let (defender_worn, pet_events) = pet_wear(defender_worn, damage.get());
            defender_events.extend(pet_events);
            let (attacker_worn, weapon_events) = attacker_wear(attacker_worn, atlas, rng);
            attacker_events.extend(weapon_events);
            (defender_events, defender_worn, attacker_worn)
        }
    };
    StrikeWear {
        attacker_worn,
        defender_worn,
        attacker_events,
        defender_events,
    }
}

/// Which wear pool a slot's occupant belongs to, resolved through the Atlas
/// for the two hand slots. `None` = neither pool (empty slot, broken item,
/// pet, or an occupant outside both pools).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WearPool {
    /// The defensive random pool: armor pieces, wings, rings, shields.
    Defensive,
    /// The offensive random pool: hand weapons and the pendant.
    Offensive,
}

/// The pool membership of `slot`'s occupant, when it has one. Broken
/// (durability-0) items are in NO pool — OUR-pin; OpenMU keeps broken
/// defensive items in the pool as wasted wear (Player.cs:2739), a quirk not
/// ported. The pendant wears on the OFFENSIVE path (ItemExtensions.cs:213-255);
/// the pet wears outside both pools; ammunition only ever consumes.
fn pool_of(worn: &Equipment, slot: EquipmentSlot, atlas: &Atlas) -> Option<WearPool> {
    let occupant = worn.get(slot)?;
    if occupant.durability.current() == 0 {
        return None;
    }
    match slot {
        EquipmentSlot::Helm
        | EquipmentSlot::Armor
        | EquipmentSlot::Pants
        | EquipmentSlot::Gloves
        | EquipmentSlot::Boots
        | EquipmentSlot::Wings
        | EquipmentSlot::Ring1
        | EquipmentSlot::Ring2 => Some(WearPool::Defensive),
        EquipmentSlot::Pendant => Some(WearPool::Offensive),
        EquipmentSlot::Pet => None,
        EquipmentSlot::LeftHand | EquipmentSlot::RightHand => {
            let def = atlas.item(occupant.item)?;
            match &def.kind {
                ItemKind::Shield { .. } => Some(WearPool::Defensive),
                ItemKind::Weapon { .. }
                | ItemKind::Bow { .. }
                | ItemKind::Crossbow { .. }
                | ItemKind::Staff { .. } => Some(WearPool::Offensive),
                ItemKind::Arrows { .. }
                | ItemKind::Bolts { .. }
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
                | ItemKind::Consumable { .. }
                | ItemKind::LuckyBox
                | ItemKind::EventTicket { .. }
                | ItemKind::MixMaterial
                | ItemKind::StatFruit => None,
            }
        }
    }
}

/// The occupied, non-broken slots of `pool`, in the fixed [`EquipmentSlot::ALL`]
/// order.
fn pool_slots(worn: &Equipment, pool: WearPool, atlas: &Atlas) -> Vec<EquipmentSlot> {
    EquipmentSlot::ALL
        .into_iter()
        .filter(|&slot| pool_of(worn, slot, atlas) == Some(pool))
        .collect()
}

/// One random defensive-pool pick worn by `health_damage / 2000`. Draws one
/// word iff the pool is non-empty; a naked or all-broken defender draws ZERO
/// words — deterministic, since every host holds the same worn set.
fn defender_wear(
    worn: Equipment,
    health_damage: u32,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> (Equipment, Vec<WearEvent>) {
    let pool = pool_slots(&worn, WearPool::Defensive, atlas);
    wear_one(
        worn,
        pool,
        health_damage,
        nonzero(DEFENSIVE_WEAR_DIVISOR),
        rng,
    )
}

/// One random offensive-pool pick worn a FLAT one hit per 10000. Draws one
/// word iff the pool is non-empty; a bare-handed attacker draws ZERO words.
fn attacker_wear(
    worn: Equipment,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> (Equipment, Vec<WearEvent>) {
    let pool = pool_slots(&worn, WearPool::Offensive, atlas);
    wear_one(worn, pool, ONE_HIT, nonzero(OFFENSIVE_WEAR_DIVISOR), rng)
}

/// Picks one slot uniformly from the pool's fixed-order enumeration and wears
/// its occupant toward broken. An empty pool folds to no wear and NO draw —
/// [`OneOrMore`] is the non-emptiness proof, so a 0-bound draw is
/// unrepresentable.
fn wear_one(
    worn: Equipment,
    pool: Vec<EquipmentSlot>,
    amount: u32,
    divisor: NonZeroU32,
    rng: &mut impl RngCore,
) -> (Equipment, Vec<WearEvent>) {
    match OneOrMore::new(pool) {
        Err(_) => (worn, Vec::new()),
        Ok(slots) => {
            let picked = *pick_one(&slots, rng);
            wear_slot(worn, picked, amount, divisor)
        }
    }
}

/// Wears the occupant of `picked` by `amount / divisor` via its persisted
/// ledger, keeping a ground-to-0 item worn ([`Durability::worn`], the
/// broken-keep seam). The pool proved the slot occupied; the `None` arm is the
/// total fold of the component's optional slot, not a guard.
fn wear_slot(
    worn: Equipment,
    picked: EquipmentSlot,
    amount: u32,
    divisor: NonZeroU32,
) -> (Equipment, Vec<WearEvent>) {
    let (worn, taken) = worn.without(picked);
    match taken {
        None => (worn, Vec::new()),
        Some(item) => {
            let durability = item.durability.worn(amount, divisor);
            let event = if durability.current() == 0 {
                WearEvent::Broken { slot: picked }
            } else {
                WearEvent::Worn {
                    slot: picked,
                    durability,
                }
            };
            let updated = ItemInstance { durability, ..item };
            (worn.with(picked, updated), vec![event])
        }
    }
}

/// The pet's additional wear at `health_damage / 100000`, outside the random
/// pool, no RNG (a single fixed slot). A non-trainable pet ground to 0 is
/// destroyed — removed from the slot — never left broken; every pre-S3 pet
/// (Angel, Imp, Uniria, Dinorant) is non-trainable
/// (W-SRC: Player.cs:2745-2758; the trainable-pet exp rule is S3+/DL, out of
/// scope). A broken-absent or empty pet slot folds to no wear.
fn pet_wear(worn: Equipment, health_damage: u32) -> (Equipment, Vec<WearEvent>) {
    let (worn, taken) = worn.without(EquipmentSlot::Pet);
    match taken {
        None => (worn, Vec::new()),
        Some(item) => {
            if item.durability.current() == 0 {
                return (worn.with(EquipmentSlot::Pet, item), Vec::new());
            }
            let durability = item
                .durability
                .worn(health_damage, nonzero(PET_WEAR_DIVISOR));
            if durability.current() == 0 {
                return (
                    worn,
                    vec![WearEvent::Destroyed {
                        slot: EquipmentSlot::Pet,
                    }],
                );
            }
            let updated = ItemInstance { durability, ..item };
            (
                worn.with(EquipmentSlot::Pet, updated),
                vec![WearEvent::Worn {
                    slot: EquipmentSlot::Pet,
                    durability,
                }],
            )
        }
    }
}

/// Consumes one round from a worn quiver — the hand slot whose occupant the
/// Atlas classifies as ammunition. No RNG (a fixed scan of the two hands);
/// durability IS the round count, so the last round destroys the quiver via
/// [`Durability::decremented`] (the removal seam, distinct from the
/// broken-keep [`Durability::worn`]). No ammunition worn = no consumption.
fn ammo_consume(worn: Equipment, atlas: &Atlas) -> (Equipment, Vec<WearEvent>) {
    let ammo_slot = [EquipmentSlot::LeftHand, EquipmentSlot::RightHand]
        .into_iter()
        .find(|&slot| is_ammunition(&worn, slot, atlas));
    let Some(slot) = ammo_slot else {
        return (worn, Vec::new());
    };
    let (worn, taken) = worn.without(slot);
    match taken {
        None => (worn, Vec::new()),
        Some(item) => match item.durability.decremented() {
            None => (worn, vec![WearEvent::Destroyed { slot }]),
            Some(durability) => {
                let updated = ItemInstance { durability, ..item };
                (
                    worn.with(slot, updated),
                    vec![WearEvent::Worn { slot, durability }],
                )
            }
        },
    }
}

/// Whether `slot`'s occupant is ammunition, resolved through the Atlas. An
/// empty slot or an unknown identity is not ammunition — a total fold.
fn is_ammunition(worn: &Equipment, slot: EquipmentSlot, atlas: &Atlas) -> bool {
    let Some(occupant) = worn.get(slot) else {
        return false;
    };
    match atlas.item(occupant.item) {
        Some(def) => matches!(def.kind, ItemKind::Arrows { .. } | ItemKind::Bolts { .. }),
        None => false,
    }
}
