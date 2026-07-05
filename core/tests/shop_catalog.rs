//! W-SHOP shelf-catalog contract, proven against the real regenerated `/data`.
//!
//! The Atlas resolves all eleven era merchants' shelves with every entry
//! joined to its item definition at parse, the 8×15 grid re-proven with zero
//! overlap, and the shop/merchant referential edge proven both ways. The
//! curated data facts ride here too: the two Hanzo fixes, the Vine mixed
//! levels, the two option-2 shields, the summon-orb level ladder, and the
//! per-family entry counts. The dataset rides in through the shared
//! `common/dataset` harness: the catalog proofs read the parsed
//! [`real_atlas`]; the negative tests take the un-parsed [`real_static_data`]
//! and corrupt a record before [`Atlas::parse`].

#[path = "common/dataset.rs"]
mod dataset;

use dataset::{real_atlas, real_static_data};

use std::collections::BTreeSet;
use std::num::NonZeroU8;

use mu_core::components::item_instance::{LuckRoll, RolledNormalOption, SkillRoll};
use mu_core::components::item_options::NormalOption;
use mu_core::components::levels::{EnhanceLevel, OptionLevel};
use mu_core::data::atlas::{Atlas, AtlasError};
use mu_core::data::common::{ItemRef, MonsterNumber};
use mu_core::data::monster_definitions::{MonsterRole, NpcWindow};
use mu_core::data::npc_shops::{ShelfEntry, ShelfSlot, ShelfStock};

/// The eleven era merchant NPC numbers.
const MERCHANTS: [u16; 11] = [242, 243, 244, 245, 246, 248, 250, 251, 253, 254, 255];

#[test]
fn atlas_resolves_all_eleven_merchant_catalogs() {
    let atlas = real_atlas();
    for number in MERCHANTS {
        let shop = atlas.shop(MonsterNumber(number)).unwrap();
        assert!(shop.entries().count() > 0, "merchant {number} is stocked");
    }
    // Non-merchant numbers are a genuine miss: a monster, a non-merchant
    // window NPC (Chaos Goblin, Baz the vault), and an open number.
    assert!(atlas.shop(MonsterNumber(0)).is_none());
    assert!(atlas.shop(MonsterNumber(238)).is_none());
    assert!(atlas.shop(MonsterNumber(240)).is_none());
    assert!(atlas.shop(MonsterNumber(9999)).is_none());
}

#[test]
fn the_shop_merchant_edge_holds_both_ways_over_the_real_roster() {
    let atlas = real_atlas();
    let mut merchant_numbers = Vec::new();
    for definition in atlas.monsters() {
        let is_merchant = matches!(
            definition.role,
            MonsterRole::Npc {
                window: Some(NpcWindow::Merchant),
            }
        );
        if is_merchant {
            merchant_numbers.push(definition.number.0);
            assert!(atlas.shop(definition.number).is_some());
        } else {
            assert!(atlas.shop(definition.number).is_none());
        }
    }
    assert_eq!(merchant_numbers, MERCHANTS);
}

#[test]
fn shelves_fit_the_grid_with_zero_overlap_and_pinned_family_counts() {
    let atlas = real_atlas();
    let mut total = 0usize;
    let (mut gear, mut stack, mut quiver, mut single) = (0usize, 0usize, 0usize, 0usize);
    for number in MERCHANTS {
        let shop = atlas.shop(MonsterNumber(number)).unwrap();
        let mut occupied: BTreeSet<(u16, u16)> = BTreeSet::new();
        for (slot, entry) in shop.entries() {
            total += 1;
            match entry.stock {
                ShelfStock::Gear { .. } => gear += 1,
                ShelfStock::Stack { .. } => stack += 1,
                ShelfStock::Quiver => quiver += 1,
                ShelfStock::Single => single += 1,
            }
            // Independent geometric re-proof through the public views: the
            // joined definition's footprint fits the 8×15 grid and claims
            // every covered cell exactly once.
            let row = u16::from(slot.row());
            let col = u16::from(slot.col());
            let height = u16::from(entry.def.height);
            let width = u16::from(entry.def.width);
            assert!(row + height <= u16::from(ShelfSlot::ROWS), "npc {number}");
            assert!(col + width <= u16::from(ShelfSlot::COLUMNS), "npc {number}");
            for covered_row in row..row + height {
                for covered_col in col..col + width {
                    assert!(
                        occupied.insert((covered_row, covered_col)),
                        "npc {number} overlaps at ({covered_row},{covered_col})"
                    );
                }
            }
        }
    }
    assert_eq!(total, 231);
    assert_eq!(gear, 153);
    assert_eq!(stack, 48);
    assert_eq!(quiver, 6);
    assert_eq!(single, 24);
}

#[test]
fn hanzo_ships_the_two_curated_fixes() {
    let atlas = real_atlas();
    let shop = atlas.shop(MonsterNumber(251)).unwrap();

    // D15: the +2 entry is an actual Mace (Scepters #0), no skill.
    let mace = shop.entry(ShelfSlot::new(50).unwrap()).unwrap();
    assert_eq!(
        mace.def.id,
        ItemRef {
            group: 2,
            number: 0
        }
    );
    assert_eq!(mace.level, EnhanceLevel::L2);
    assert_eq!(
        *mace.stock,
        ShelfStock::Gear {
            luck: LuckRoll::Lucky,
            skill: SkillRoll::NoSkill,
            option: Some(RolledNormalOption {
                option: NormalOption::PhysicalDamage,
                level: OptionLevel::L1,
            }),
        }
    );

    // Morning Star (Scepters #1) stays at +3 with skill.
    let morning_star = shop.entry(ShelfSlot::new(54).unwrap()).unwrap();
    assert_eq!(
        morning_star.def.id,
        ItemRef {
            group: 2,
            number: 1
        }
    );
    assert_eq!(morning_star.level, EnhanceLevel::L3);
    assert_eq!(
        *morning_star.stock,
        ShelfStock::Gear {
            luck: LuckRoll::Lucky,
            skill: SkillRoll::WithSkill,
            option: Some(RolledNormalOption {
                option: NormalOption::PhysicalDamage,
                level: OptionLevel::L1,
            }),
        }
    );

    // D14: the Falchion moved off Gladius's anchor to its own slot 76.
    let gladius = shop.entry(ShelfSlot::new(73).unwrap()).unwrap();
    assert_eq!(
        gladius.def.id,
        ItemRef {
            group: 0,
            number: 6
        }
    );
    let falchion = shop.entry(ShelfSlot::new(76).unwrap()).unwrap();
    assert_eq!(
        falchion.def.id,
        ItemRef {
            group: 0,
            number: 7
        }
    );
    assert_eq!(falchion.level, EnhanceLevel::L3);
}

#[test]
fn vine_set_ships_the_mixed_levels_verbatim() {
    let atlas = real_atlas();
    let shop = atlas.shop(MonsterNumber(242)).unwrap();
    // K4: +0 helm/armor/pants, +3 gloves/boots.
    let expected: [(u8, u8, u8); 5] =
        [(32, 7, 0), (34, 8, 0), (36, 9, 0), (38, 10, 3), (48, 11, 3)];
    for (slot, group, level) in expected {
        let entry = shop.entry(ShelfSlot::new(slot).unwrap()).unwrap();
        assert_eq!(entry.def.id, ItemRef { group, number: 10 }, "slot {slot}");
        assert_eq!(
            entry.level,
            EnhanceLevel::try_from(level).unwrap(),
            "slot {slot}"
        );
    }
}

#[test]
fn exactly_the_two_k6_shields_carry_an_option_two() {
    let atlas = real_atlas();

    // Elven Shield at Eo (243), Legendary Shield at Izabel (245).
    let elven = atlas
        .shop(MonsterNumber(243))
        .unwrap()
        .entry(ShelfSlot::new(78).unwrap())
        .unwrap();
    assert_eq!(
        elven.def.id,
        ItemRef {
            group: 6,
            number: 3
        }
    );
    let legendary = atlas
        .shop(MonsterNumber(245))
        .unwrap()
        .entry(ShelfSlot::new(52).unwrap())
        .unwrap();
    assert_eq!(
        legendary.def.id,
        ItemRef {
            group: 6,
            number: 14
        }
    );
    let option_two = Some(RolledNormalOption {
        option: NormalOption::DefenseRate,
        level: OptionLevel::L2,
    });
    for entry in [&elven, &legendary] {
        match entry.stock {
            ShelfStock::Gear { option, .. } => assert_eq!(*option, option_two),
            ShelfStock::Stack { .. } | ShelfStock::Quiver | ShelfStock::Single => {
                panic!("a shield is gear")
            }
        }
    }

    // Every other gear option across the whole catalog is level 1.
    let mut level_two = 0usize;
    for number in MERCHANTS {
        let shop = atlas.shop(MonsterNumber(number)).unwrap();
        for (_, entry) in shop.entries() {
            if let ShelfStock::Gear {
                option: Some(rolled),
                ..
            } = entry.stock
            {
                match rolled.level {
                    OptionLevel::L2 => level_two += 1,
                    OptionLevel::L1 => {}
                    OptionLevel::L3 | OptionLevel::L4 => {
                        panic!("no era store option exceeds level 2")
                    }
                }
            }
        }
    }
    assert_eq!(level_two, 2);
}

#[test]
fn summon_orbs_ship_levels_zero_through_four() {
    let atlas = real_atlas();
    let shop = atlas.shop(MonsterNumber(242)).unwrap();
    for (offset, slot) in (24u8..=28).enumerate() {
        let entry = shop.entry(ShelfSlot::new(slot).unwrap()).unwrap();
        assert_eq!(
            entry.def.id,
            ItemRef {
                group: 12,
                number: 11
            }
        );
        assert_eq!(
            entry.level,
            EnhanceLevel::try_from(u8::try_from(offset).unwrap()).unwrap()
        );
        assert_eq!(*entry.stock, ShelfStock::Single);
    }
}

#[test]
fn shelf_lookup_is_anchor_exact() {
    let atlas = real_atlas();
    let shop = atlas.shop(MonsterNumber(251)).unwrap();
    // The Falchion anchors at 76 and covers rows 9-11 of column 4; the covered
    // non-anchor cell 84 is a miss, as is the never-covered cell 77.
    assert!(shop.entry(ShelfSlot::new(76).unwrap()).is_some());
    assert!(shop.entry(ShelfSlot::new(84).unwrap()).is_none());
    assert!(shop.entry(ShelfSlot::new(77).unwrap()).is_none());
}

#[test]
fn a_three_pack_potion_stack_carries_its_pieces() {
    let atlas = real_atlas();
    let shop = atlas.shop(MonsterNumber(242)).unwrap();
    let pack = shop.entry(ShelfSlot::new(8).unwrap()).unwrap();
    assert_eq!(
        pack.def.id,
        ItemRef {
            group: 14,
            number: 0
        }
    );
    assert_eq!(
        *pack.stock,
        ShelfStock::Stack {
            pieces: NonZeroU8::new(3).unwrap(),
        }
    );
}

// --- Negative tests: parse proofs valid on-disk data cannot exercise.

#[test]
fn atlas_rejects_a_shop_with_a_dangling_item_ref() {
    let mut data = real_static_data();
    let record = data.shops.records.first_mut().unwrap();
    let entry = record.shelf.first_mut().unwrap();
    entry.item = ItemRef {
        group: 200,
        number: 0,
    };
    let err = Atlas::parse(data).unwrap_err();
    assert!(matches!(err, AtlasError::UnknownItemRef { .. }));
}

#[test]
fn atlas_rejects_a_shop_for_a_non_merchant_and_for_an_unknown_npc() {
    let mut data = real_static_data();
    data.shops.records.first_mut().unwrap().npc = MonsterNumber(0);
    let err = Atlas::parse(data).unwrap_err();
    assert_eq!(
        err,
        AtlasError::ShopForNonMerchant {
            npc: MonsterNumber(0)
        }
    );

    let mut data = real_static_data();
    data.shops.records.first_mut().unwrap().npc = MonsterNumber(9999);
    let err = Atlas::parse(data).unwrap_err();
    assert_eq!(
        err,
        AtlasError::UnknownMonsterRef {
            monster: MonsterNumber(9999)
        }
    );
}

#[test]
fn atlas_rejects_an_unstocked_merchant_and_a_duplicate_record() {
    let mut data = real_static_data();
    data.shops.records.pop();
    let err = Atlas::parse(data).unwrap_err();
    assert_eq!(
        err,
        AtlasError::MerchantWithoutShop {
            npc: MonsterNumber(255)
        }
    );

    let mut data = real_static_data();
    let clone = data.shops.records.first().unwrap().clone();
    data.shops.records.push(clone);
    let err = Atlas::parse(data).unwrap_err();
    assert_eq!(
        err,
        AtlasError::DuplicateShopRecord {
            npc: MonsterNumber(242)
        }
    );
}

/// Appends an entry to Hanzo's (251) shelf record. A macro, so its `unwrap`
/// expands at the `#[test]` call site where `clippy.toml` permits it
/// (`allow-unwrap-in-tests`).
macro_rules! with_extra_hanzo_entry {
    ($data:expr, $entry:expr $(,)?) => {{
        let record = $data
            .shops
            .records
            .iter_mut()
            .find(|record| record.npc == MonsterNumber(251))
            .unwrap();
        record.shelf.push($entry);
    }};
}

#[test]
fn atlas_rejects_an_overlapping_shelf_entry() {
    let mut data = real_static_data();
    // Cell 84 is covered by the Falchion anchored at 76.
    with_extra_hanzo_entry!(
        data,
        ShelfEntry {
            slot: ShelfSlot::new(84).unwrap(),
            item: ItemRef {
                group: 12,
                number: 8,
            },
            level: EnhanceLevel::L0,
            stock: ShelfStock::Single,
            review: None,
        },
    );
    let err = Atlas::parse(data).unwrap_err();
    assert_eq!(
        err,
        AtlasError::ShelfSlotOverlap {
            npc: MonsterNumber(251),
            slot: ShelfSlot::new(84).unwrap(),
        }
    );
}

#[test]
fn atlas_rejects_a_footprint_running_past_the_grid() {
    let mut data = real_static_data();
    // A 2×2 body armor anchored on the bottom row overruns row 15.
    with_extra_hanzo_entry!(
        data,
        ShelfEntry {
            slot: ShelfSlot::new(112).unwrap(),
            item: ItemRef {
                group: 8,
                number: 3,
            },
            level: EnhanceLevel::L0,
            stock: ShelfStock::Gear {
                luck: LuckRoll::Plain,
                skill: SkillRoll::NoSkill,
                option: None,
            },
            review: None,
        },
    );
    let err = Atlas::parse(data).unwrap_err();
    assert_eq!(
        err,
        AtlasError::ShelfFootprintOutOfGrid {
            npc: MonsterNumber(251),
            slot: ShelfSlot::new(112).unwrap(),
        }
    );
}

#[test]
fn atlas_rejects_a_stock_tag_disagreeing_with_the_kind() {
    let mut data = real_static_data();
    // A stack tag on a sword: the family discriminant is dual-sourced, so the
    // wire tag and the joined kind must agree.
    with_extra_hanzo_entry!(
        data,
        ShelfEntry {
            slot: ShelfSlot::new(96).unwrap(),
            item: ItemRef {
                group: 0,
                number: 7,
            },
            level: EnhanceLevel::L0,
            stock: ShelfStock::Stack {
                pieces: NonZeroU8::new(1).unwrap(),
            },
            review: None,
        },
    );
    let err = Atlas::parse(data).unwrap_err();
    assert_eq!(
        err,
        AtlasError::ShelfStockKindMismatch {
            npc: MonsterNumber(251),
            slot: ShelfSlot::new(96).unwrap(),
        }
    );
}

#[test]
fn atlas_rejects_a_stack_past_its_definition_cap() {
    let mut data = real_static_data();
    // Small healing potions cap at 3 pieces (the definition's durability).
    with_extra_hanzo_entry!(
        data,
        ShelfEntry {
            slot: ShelfSlot::new(96).unwrap(),
            item: ItemRef {
                group: 14,
                number: 0,
            },
            level: EnhanceLevel::L0,
            stock: ShelfStock::Stack {
                pieces: NonZeroU8::new(4).unwrap(),
            },
            review: None,
        },
    );
    let err = Atlas::parse(data).unwrap_err();
    assert_eq!(
        err,
        AtlasError::ShelfStackOverCap {
            npc: MonsterNumber(251),
            slot: ShelfSlot::new(96).unwrap(),
        }
    );
}
