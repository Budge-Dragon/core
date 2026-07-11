//! Per-kill drop resolution: one category roll (money, item, jewel, excellent,
//! or nothing) plus every special drop the kill qualifies for. Pure and
//! deterministic — the category roll, the pool pick, and each special's chance
//! all draw through the injected RNG in a fixed order (category, then category
//! payload, then special drops in dataset order). The category rates partition a
//! `0..10000` space proven at load, so the "nothing" remainder is a real bucket,
//! never a fallback. [`roll_drop_group`] is the second declared entry point: a
//! uniform pick among a self-contained item group at a fixed plus level.

use rand_core::RngCore;

use crate::components::collections::OneOrMore;
use crate::components::interval::Interval;
use crate::components::item_quality::ItemRarity;
use crate::components::units::{ChancePer10000, Exp, ItemLevel, Level, Zen};
use crate::data::atlas::Atlas;
use crate::data::common::{ItemRef, MapNumber};
use crate::data::drop_config::DropConfig;
use crate::data::item_definitions::ItemDefinition;
use crate::data::special_drops::SpecialDrop;
use crate::entities::monster_instance::MonsterInstance;
use crate::events::loot::{Drop, DropResolution};
use crate::rng::uniform_below;
use crate::services::chance::{pick_one, roll_per_10000};
use crate::services::item_roll::is_excellent_capable;
use crate::services::ratio::nonzero;

// W-SRC: OpenMU drop constants hardcoded in the drop routine, not in
// game_config.json — the flat zen bonus a money drop adds to the kill's
// experience, and the width of the item-level drop window below the monster's
// level (the classic `DropLevel > monsterLevel - 12` band).
/// Flat zen added to a money drop, on top of the awarded experience.
const BASE_MONEY_DROP: u64 = 7;
/// Item-level drop window width below the monster's level.
const DROP_LEVEL_WINDOW_GAP: u16 = 11;

/// Which non-empty category a kill's single roll landed in — money, an item from
/// the level pool, a jewel, an excellent item, or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropCategory {
    Money,
    Item,
    Jewel,
    Excellent,
    Nothing,
}

/// Resolves everything a kill drops: the single category roll's payload plus
/// every special drop the victim qualifies for, in dataset order. `victim_level`
/// is the resolved monster level (looked up once by the kill orchestrator, so
/// this service stays free of the monster-definition lookup).
#[must_use]
pub fn resolve_kill_drops(
    victim: &MonsterInstance,
    victim_level: Level,
    awarded_exp: Exp,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> DropResolution {
    let map = victim.placement.map;
    let category = match category_roll(atlas.drop_config(), rng) {
        DropCategory::Money => Drop::Zen {
            amount: Zen(awarded_exp.0.saturating_add(BASE_MONEY_DROP)),
        },
        DropCategory::Item => item_drop(victim_level, atlas, ItemRarity::Normal, rng),
        DropCategory::Excellent => item_drop(victim_level, atlas, ItemRarity::Excellent, rng),
        DropCategory::Jewel => jewel_drop(atlas, rng),
        DropCategory::Nothing => Drop::Nothing,
    };
    let specials = special_drops(victim, victim_level, map, atlas, rng);
    DropResolution { category, specials }
}

/// Rolls a self-contained drop group: one uniform pick among the group's
/// items, at the fixed `item_level`, normal rarity — the same pick-from-a-set
/// grain as a monster-bound special drop, declared as its own entry point.
/// Draws exactly one random word.
#[must_use]
pub fn roll_drop_group(
    items: &OneOrMore<ItemRef>,
    item_level: ItemLevel,
    rng: &mut impl RngCore,
) -> Drop {
    Drop::Item {
        item: *pick_one(items, rng),
        level: item_level,
        rarity: ItemRarity::Normal,
    }
}

/// Walks the four category rates cumulatively over a `0..10000` draw; a roll past
/// them all lands in the "nothing" remainder — a real bucket held apart, so the
/// walk never needs a wildcard fallback.
fn category_roll(config: &DropConfig, rng: &mut impl RngCore) -> DropCategory {
    let roll = uniform_below(nonzero(u32::from(ChancePer10000::DENOMINATOR)), rng);
    let mut cumulative = 0u32;
    for (rate, category) in [
        (config.money_roll(), DropCategory::Money),
        (config.item_roll(), DropCategory::Item),
        (config.jewel_roll(), DropCategory::Jewel),
        (config.excellent_roll(), DropCategory::Excellent),
    ] {
        cumulative += u32::from(rate.numerator());
        if roll < cumulative {
            return category;
        }
    }
    DropCategory::Nothing
}

/// An item drop from the monster's level pool: the window `[level - 11, level]`,
/// a uniform pick among the droppable items in it, and a plus level of
/// `min((level - drop_level) / 3, max_item_level)`. An `Excellent` rarity gates
/// the pool to excellent-capable kinds so no excellent set is ever stamped on a
/// kind that has none; `Normal` and `Ancient` draw the unrestricted pool. An
/// empty window is a real "nothing" — matched before any table is built, never a
/// panic.
fn item_drop(level: Level, atlas: &Atlas, rarity: ItemRarity, rng: &mut impl RngCore) -> Drop {
    let monster_level = level.get();
    let window = Interval::spanning(
        narrow_u8(monster_level.saturating_sub(DROP_LEVEL_WINDOW_GAP)),
        narrow_u8(monster_level),
    );
    let candidates: Vec<&ItemDefinition> = atlas
        .drop_pool()
        .in_window(window)
        .filter_map(|id| atlas.item(id))
        .filter(|def| match rarity {
            ItemRarity::Excellent => is_excellent_capable(&def.kind),
            ItemRarity::Normal | ItemRarity::Ancient => true,
        })
        .collect();
    match OneOrMore::new(candidates) {
        Err(_) => Drop::Nothing,
        Ok(pool) => {
            let definition = *pick_one(&pool, rng);
            let above_base =
                u32::from(monster_level.saturating_sub(u16::from(definition.drop_level)));
            let plus = (above_base / 3).min(u32::from(definition.max_item_level.get()));
            Drop::Item {
                item: definition.id,
                level: ItemLevel::clamped(u64::from(plus)),
                rarity,
            }
        }
    }
}

/// A jewel drop: a uniform pick from the world jewel roster, at plus level zero.
fn jewel_drop(atlas: &Atlas, rng: &mut impl RngCore) -> Drop {
    let jewel = *pick_one(atlas.drop_config().jewel_drops(), rng);
    Drop::Item {
        item: jewel,
        level: ItemLevel::ZERO,
        rarity: ItemRarity::Normal,
    }
}

/// Every special drop the victim qualifies for, in dataset order. Level-banded
/// and map-bound drops gate on their own chance roll; monster-bound drops fall on
/// every matching kill (no roll).
fn special_drops(
    victim: &MonsterInstance,
    victim_level: Level,
    map: MapNumber,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> Vec<Drop> {
    let mut drops = Vec::new();
    for record in atlas.special_drops() {
        match &record.drop {
            SpecialDrop::LevelBanded {
                item,
                chance_per_10000,
                bands,
            } => {
                if let Some(item_level) = bands.item_level_for(victim_level) {
                    if roll_per_10000(*chance_per_10000, rng) {
                        drops.push(Drop::Item {
                            item: *item,
                            level: item_level,
                            rarity: ItemRarity::Normal,
                        });
                    }
                }
            }
            SpecialDrop::MonsterBound {
                monster,
                items,
                item_level,
            } => {
                if *monster == victim.number {
                    let item = *pick_one(items, rng);
                    drops.push(Drop::Item {
                        item,
                        level: *item_level,
                        rarity: ItemRarity::Normal,
                    });
                }
            }
            SpecialDrop::MapBound {
                map: bound_map,
                min_monster_level,
                item,
                item_level,
                chance_per_10000,
            } => {
                if *bound_map == map
                    && victim_level >= *min_monster_level
                    && roll_per_10000(*chance_per_10000, rng)
                {
                    drops.push(Drop::Item {
                        item: *item,
                        level: *item_level,
                        rarity: ItemRarity::Normal,
                    });
                }
            }
        }
    }
    drops
}

/// Saturating narrow of a monster level into the item pool's `u8` drop-level
/// key space; levels above 255 clamp to the top of the pool.
fn narrow_u8(value: u16) -> u8 {
    // Boundary saturation into the pool's key space — never a masked lookup.
    u8::try_from(value).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic `SplitMix64` for replayable tests; cast-free extraction of
    /// the low 32 bits keeps clippy's cast lints quiet in test code too.
    struct TestRng {
        state: u64,
    }

    impl TestRng {
        fn new(seed: u64) -> Self {
            Self { state: seed }
        }
    }

    impl RngCore for TestRng {
        fn next_u64(&mut self) -> u64 {
            self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        fn next_u32(&mut self) -> u32 {
            let [b0, b1, b2, b3, _, _, _, _] = self.next_u64().to_le_bytes();
            u32::from_le_bytes([b0, b1, b2, b3])
        }

        fn fill_bytes(&mut self, dst: &mut [u8]) {
            for chunk in dst.chunks_mut(8) {
                let bytes = self.next_u64().to_le_bytes();
                for (slot, byte) in chunk.iter_mut().zip(bytes.iter()) {
                    *slot = *byte;
                }
            }
        }
    }

    /// Wraps [`TestRng`] and counts the 64-bit words drawn.
    struct CountingRng {
        inner: TestRng,
        words: u64,
    }

    impl CountingRng {
        fn new(seed: u64) -> Self {
            Self {
                inner: TestRng::new(seed),
                words: 0,
            }
        }
    }

    impl RngCore for CountingRng {
        fn next_u64(&mut self) -> u64 {
            self.words += 1;
            self.inner.next_u64()
        }

        fn next_u32(&mut self) -> u32 {
            let [b0, b1, b2, b3, _, _, _, _] = self.next_u64().to_le_bytes();
            u32::from_le_bytes([b0, b1, b2, b3])
        }

        fn fill_bytes(&mut self, dst: &mut [u8]) {
            self.inner.fill_bytes(dst);
        }
    }

    fn item(group: u8, number: u16) -> ItemRef {
        ItemRef { group, number }
    }

    fn a_group() -> OneOrMore<ItemRef> {
        OneOrMore::with_head(item(12, 15), vec![item(13, 0), item(14, 13)])
    }

    #[test]
    fn narrow_u8_saturates_above_255() {
        assert_eq!(narrow_u8(0), 0);
        assert_eq!(narrow_u8(200), 200);
        assert_eq!(narrow_u8(300), 255);
    }

    #[test]
    fn roll_drop_group_picks_a_group_member_at_the_fixed_level() {
        let group = a_group();
        let level = ItemLevel::new(7).unwrap();
        for seed in 0..32 {
            let mut rng = TestRng::new(seed);
            match roll_drop_group(&group, level, &mut rng) {
                Drop::Item {
                    item: picked,
                    level: rolled_level,
                    rarity,
                } => {
                    assert!(group.iter().any(|member| *member == picked));
                    assert_eq!(rolled_level, level);
                    assert_eq!(rarity, ItemRarity::Normal);
                }
                Drop::Zen { .. } | Drop::Nothing => {
                    panic!("a group roll always produces an item")
                }
            }
        }
    }

    #[test]
    fn roll_drop_group_on_a_single_item_group_is_certain() {
        let group = OneOrMore::with_head(item(14, 13), Vec::new());
        let mut rng = TestRng::new(9);
        assert_eq!(
            roll_drop_group(&group, ItemLevel::ZERO, &mut rng),
            Drop::Item {
                item: item(14, 13),
                level: ItemLevel::ZERO,
                rarity: ItemRarity::Normal,
            }
        );
    }

    #[test]
    fn roll_drop_group_is_deterministic_for_a_seed() {
        let group = a_group();
        let level = ItemLevel::new(3).unwrap();
        let first = roll_drop_group(&group, level, &mut TestRng::new(42));
        let second = roll_drop_group(&group, level, &mut TestRng::new(42));
        assert_eq!(first, second);
    }

    #[test]
    fn roll_drop_group_draws_exactly_one_word() {
        let mut rng = CountingRng::new(7);
        let _ = roll_drop_group(&a_group(), ItemLevel::ZERO, &mut rng);
        assert_eq!(rng.words, 1);
    }
}
