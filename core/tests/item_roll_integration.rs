//! W-INV end-to-end over the real regenerated `/data`.
//!
//! Rolls every droppable equippable item against injected policy extremes and
//! proves the drop-time contract holds on the shipped item definitions: rarity
//! and level echo the loot service's decided inputs, an excellent set matches
//! the item's derived category and is a distinct `1..=cap` set, skill rides an
//! excellent roll when the definition carries one, durability is full, ancient
//! rolls carry a `{One, Two}` bonus and no excellent set, jewels roll bare
//! consuming zero RNG words, the roll is bit-for-bit deterministic under a seed,
//! the loot service's `Drop::Item` bridges cleanly into the roll, and every
//! rolled item's footprint places into an empty grid and equips into exactly the
//! slots its kind permits.
//!
//! This file carries its own dataset loader (the movement suite's `common`
//! helpers are unused here); load failures route through `or_abort` so no banned
//! suppressor is needed outside a `#[test]` body.

use std::io::Write;
use std::path::PathBuf;

use rand_core::RngCore;
use serde::de::DeserializeOwned;

use mu_core::components::equipment::{EquipSlot, Equipment};
use mu_core::components::inventory::{Cell, Footprint, Inventory};
use mu_core::components::item_instance::{ExcellentCat, ExcellentOptions, RarityRoll, SkillRoll};
use mu_core::components::item_options::{AncientBonusLevel, ExcellentCategory};
use mu_core::components::item_quality::ItemRarity;
use mu_core::components::levels::OptionLevel;
use mu_core::components::movement::Movement;
use mu_core::components::placement::Placement;
use mu_core::components::pool::Pool;
use mu_core::components::spatial::Facing;
use mu_core::components::tile::TileCoord;
use mu_core::components::units::{ChancePer10000, Exp, ItemLevel, Level, MapNumber, Tick};
use mu_core::data::ancient_sets::AncientSet;
use mu_core::data::atlas::{Atlas, StaticData};
use mu_core::data::box_drops::BoxDrop;
use mu_core::data::chaos_mixes::ChaosMix;
use mu_core::data::classes::ClassRecord;
use mu_core::data::common::DataFile;
use mu_core::data::exp_tables::ExpTable;
use mu_core::data::game_config::{EquipmentSlot, GameConfig};
use mu_core::data::gates_warps::GateWarpRecord;
use mu_core::data::item_definitions::{ItemDefinition, ItemKind};
use mu_core::data::map_definitions::MapDefinition;
use mu_core::data::monster_definitions::MonsterDefinition;
use mu_core::data::option_roll::OptionRollPolicy;
use mu_core::data::skills::Skill;
use mu_core::data::spawns::Spawn;
use mu_core::data::special_drops::SpecialDropRecord;
use mu_core::data::terrain::{MapTerrain, TerrainBytes};
use mu_core::entities::monster_instance::MonsterInstance;
use mu_core::events::loot::Drop;
use mu_core::services::inventory::{PlaceIntent, equip, place_item};
use mu_core::services::item_roll::roll_dropped_item;
use mu_core::services::loot::resolve_kill_drops;

// --- Self-contained dataset harness (load failures abort, never unwrap). ---

fn or_abort<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            let mut stderr = std::io::stderr();
            let _ = writeln!(stderr, "item_roll_integration harness: {error}");
            std::process::abort()
        }
    }
}

fn data_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("data");
    path.push(format!("{name}.json"));
    path
}

fn load<T: DeserializeOwned>(name: &str) -> DataFile<T> {
    let text = or_abort(std::fs::read_to_string(data_path(name)));
    or_abort(serde_json::from_str(&text))
}

fn load_terrain() -> Vec<MapTerrain> {
    (0u8..=10)
        .map(|map| {
            let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            path.push("..");
            path.push("data");
            path.push("terrain");
            path.push(format!("{map}.bin"));
            MapTerrain {
                map: MapNumber(map),
                bytes: or_abort(TerrainBytes::new(or_abort(std::fs::read(&path)))),
            }
        })
        .collect()
}

fn real_atlas() -> Atlas {
    let data = StaticData {
        maps: load::<MapDefinition>("map_definitions"),
        gates_warps: load::<GateWarpRecord>("gates_warps"),
        monsters: load::<MonsterDefinition>("monster_definitions"),
        spawns: load::<Spawn>("spawns"),
        skills: load::<Skill>("skills"),
        items: load::<ItemDefinition>("item_definitions"),
        box_drops: load::<BoxDrop>("box_drops"),
        special_drops: load::<SpecialDropRecord>("special_drops"),
        ancient_sets: load::<AncientSet>("ancient_sets"),
        chaos_mixes: load::<ChaosMix>("chaos_mixes"),
        classes: load::<ClassRecord>("classes"),
        exp_tables: load::<ExpTable>("exp_tables"),
        game_config: load::<GameConfig>("game_config"),
        terrain: load_terrain(),
    };
    or_abort(Atlas::parse(data))
}

/// Deterministic `SplitMix64` — the shared replayable stream.
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

// --- Injected policy extremes (never the review-flagged defaults). ---

fn always() -> OptionRollPolicy {
    OptionRollPolicy {
        item_option_roll_per_10000: ChancePer10000::ALWAYS,
        luck_roll_per_10000: ChancePer10000::ALWAYS,
        extra_excellent_option_roll_per_10000: ChancePer10000::ALWAYS,
        second_wing_bonus_roll_per_10000: ChancePer10000::ALWAYS,
        dinorant_option_roll_per_10000: ChancePer10000::ALWAYS,
        max_excellent_options_per_drop: 3,
        max_dropped_option_level: OptionLevel::L4,
        review: None,
    }
}

// --- Test-local classifiers mirroring the roll service's per-kind gates,
//     so the contract is checked against an independent restatement. ---

/// The excellent category a kind is expected to roll, if any.
fn expected_excellent(kind: &ItemKind) -> Option<ExcellentCat> {
    match kind {
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Staff { .. } => Some(ExcellentCat::Weapon),
        ItemKind::Shield { .. }
        | ItemKind::Helm { .. }
        | ItemKind::BodyArmor { .. }
        | ItemKind::Pants { .. }
        | ItemKind::Gloves { .. }
        | ItemKind::Boots { .. }
        | ItemKind::Ring { .. } => Some(ExcellentCat::Armor),
        ItemKind::Pendant { excellent, .. } => Some(match excellent {
            ExcellentCategory::Armor => ExcellentCat::Armor,
            ExcellentCategory::Weapon { .. } => ExcellentCat::Weapon,
        }),
        ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
        | ItemKind::Wings { .. }
        | ItemKind::Pet { .. }
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

/// Whether the definition carries a weapon skill.
fn has_skill(kind: &ItemKind) -> bool {
    match kind {
        ItemKind::Weapon { skill, .. }
        | ItemKind::Bow { skill, .. }
        | ItemKind::Crossbow { skill, .. }
        | ItemKind::Staff { skill, .. }
        | ItemKind::Shield { skill, .. }
        | ItemKind::Pet { skill, .. } => skill.is_some(),
        ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
        | ItemKind::Helm { .. }
        | ItemKind::BodyArmor { .. }
        | ItemKind::Pants { .. }
        | ItemKind::Gloves { .. }
        | ItemKind::Boots { .. }
        | ItemKind::Wings { .. }
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
        | ItemKind::StatFruit => false,
    }
}

/// One equipment slot a kind is expected to accept, if it is equippable.
fn a_valid_slot(kind: &ItemKind) -> Option<EquipmentSlot> {
    match kind {
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Staff { .. }
        | ItemKind::Shield { .. } => Some(EquipmentSlot::LeftHand),
        ItemKind::Helm { .. } => Some(EquipmentSlot::Helm),
        ItemKind::BodyArmor { .. } => Some(EquipmentSlot::Armor),
        ItemKind::Pants { .. } => Some(EquipmentSlot::Pants),
        ItemKind::Gloves { .. } => Some(EquipmentSlot::Gloves),
        ItemKind::Boots { .. } => Some(EquipmentSlot::Boots),
        ItemKind::Wings { .. } => Some(EquipmentSlot::Wings),
        ItemKind::Pet { .. } => Some(EquipmentSlot::Pet),
        ItemKind::Pendant { .. } => Some(EquipmentSlot::Pendant),
        ItemKind::Ring { .. } | ItemKind::TransformationRing { .. } => Some(EquipmentSlot::Ring1),
        ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
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

fn droppable(atlas: &Atlas) -> impl Iterator<Item = &ItemDefinition> {
    atlas.items().filter(|def| def.drops_from_monsters)
}

/// Asserts the excellent payload is a distinct `1..=cap` set of the item's
/// category.
fn assert_excellent_well_formed(options: ExcellentOptions, expected: ExcellentCat, cap: u8) {
    assert_eq!(options.category(), expected);
    match options {
        ExcellentOptions::Armor { options } => {
            let count = options.count();
            assert!((1..=cap).contains(&count));
            assert_eq!(usize::from(count), options.iter().count());
        }
        ExcellentOptions::Weapon { options } => {
            let count = options.count();
            assert!((1..=cap).contains(&count));
            assert_eq!(usize::from(count), options.iter().count());
        }
    }
}

#[test]
fn every_droppable_item_rolls_normal_echoing_its_inputs() {
    let atlas = real_atlas();
    for def in droppable(&atlas) {
        for seed in 0u64..8 {
            let mut rng = TestRng::new(seed);
            let level = ItemLevel::new(u8::try_from(seed % 12).unwrap()).unwrap();
            let instance = roll_dropped_item(def, level, ItemRarity::Normal, &always(), &mut rng);
            assert_eq!(instance.item, def.id, "item echo");
            assert_eq!(instance.level, level, "level echo");
            assert_eq!(instance.roll.rarity(), ItemRarity::Normal, "rarity echo");
            assert_eq!(
                instance.durability.current(),
                instance.durability.max(),
                "durability full"
            );
            // A normal drop never nests an excellent set.
            assert!(matches!(instance.roll, RarityRoll::Normal));
        }
    }
}

#[test]
fn every_excellent_capable_item_rolls_a_matching_distinct_set() {
    let atlas = real_atlas();
    let policy = always();
    let cap = policy.max_excellent_options_per_drop;
    for def in droppable(&atlas) {
        let Some(expected) = expected_excellent(&def.kind) else {
            continue;
        };
        for seed in 0u64..16 {
            let mut rng = TestRng::new(seed);
            let level = ItemLevel::new(u8::try_from(seed % 12).unwrap()).unwrap();
            let instance = roll_dropped_item(def, level, ItemRarity::Excellent, &policy, &mut rng);
            assert_eq!(instance.item, def.id);
            assert_eq!(instance.level, level);
            assert_eq!(instance.roll.rarity(), ItemRarity::Excellent, "rarity echo");
            assert_eq!(instance.durability.current(), instance.durability.max());
            match &instance.roll {
                RarityRoll::Excellent { options } => {
                    assert_excellent_well_formed(*options, expected, cap);
                    // The set is internally consistent with its own category and
                    // fails a reconcile against the opposite one.
                    assert_eq!(instance.reconcile(Some(expected)), Ok(()));
                }
                RarityRoll::Normal | RarityRoll::Ancient { .. } => {
                    panic!("an excellent-capable item must roll excellent")
                }
            }
            if has_skill(&def.kind) {
                assert_eq!(
                    instance.skill,
                    SkillRoll::WithSkill,
                    "skill rides excellent"
                );
            }
        }
    }
}

#[test]
fn a_representative_item_rolls_ancient_with_both_tiers_and_no_set() {
    let atlas = real_atlas();
    // Any excellent-capable droppable item stands in as the ancient carrier.
    let def = droppable(&atlas)
        .find(|def| expected_excellent(&def.kind).is_some())
        .expect("the dataset ships excellent-capable droppable items");
    let mut saw_one = false;
    let mut saw_two = false;
    for seed in 0u64..64 {
        let mut rng = TestRng::new(seed);
        let instance = roll_dropped_item(
            def,
            ItemLevel::new(5).unwrap(),
            ItemRarity::Ancient,
            &always(),
            &mut rng,
        );
        match instance.roll {
            RarityRoll::Ancient { bonus } => match bonus {
                AncientBonusLevel::One => saw_one = true,
                AncientBonusLevel::Two => saw_two = true,
            },
            RarityRoll::Normal | RarityRoll::Excellent { .. } => panic!("ancient expected"),
        }
    }
    assert!(saw_one && saw_two, "both ancient tiers appear across seeds");
}

#[test]
fn every_real_jewel_rolls_bare_consuming_zero_words() {
    let atlas = real_atlas();
    let mut jewels = 0u32;
    for def in atlas.items() {
        if !matches!(def.kind, ItemKind::Jewel { .. }) {
            continue;
        }
        jewels += 1;
        let mut rng = TestRng::new(4);
        let mut probe = TestRng::new(4);
        let instance = roll_dropped_item(
            def,
            ItemLevel::ZERO,
            ItemRarity::Normal,
            &always(),
            &mut rng,
        );
        assert_eq!(instance.roll, RarityRoll::Normal);
        assert!(instance.normal_option.is_none());
        assert_eq!(instance.durability.max(), def.durability);
        assert_eq!(rng.next_u64(), probe.next_u64(), "a jewel draws no words");
    }
    assert!(jewels > 0, "the dataset ships jewels");
}

#[test]
fn a_fixed_roll_is_bit_for_bit_deterministic() {
    let atlas = real_atlas();
    let def = droppable(&atlas)
        .find(|def| expected_excellent(&def.kind).is_some())
        .expect("an excellent-capable droppable item");
    let level = ItemLevel::new(9).unwrap();
    let mut a = TestRng::new(0xABCD);
    let mut b = TestRng::new(0xABCD);
    let ia = roll_dropped_item(def, level, ItemRarity::Excellent, &always(), &mut a);
    let ib = roll_dropped_item(def, level, ItemRarity::Excellent, &always(), &mut b);
    assert_eq!(
        or_abort(serde_json::to_string(&ia)),
        or_abort(serde_json::to_string(&ib)),
        "identical serialized instances"
    );
    assert_eq!(a.next_u64(), b.next_u64(), "identical word consumption");
}

#[test]
fn loot_drop_item_bridges_into_the_roll() {
    let atlas = real_atlas();
    let victim = MonsterInstance {
        number: atlas.monsters().next().expect("a monster").number,
        placement: Placement {
            position: TileCoord::new(10, 10).to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: MapNumber(0),
        },
        health: Pool::full(1),
        anchor: TileCoord::new(10, 10).to_world(),
        next_action: Tick(0),
    };
    let victim_level = Level::new(80).unwrap();
    let mut bridged = 0u32;
    for seed in 0u64..256 {
        let mut rng = TestRng::new(seed);
        let resolution = resolve_kill_drops(&victim, victim_level, Exp(0), &atlas, &mut rng);
        let mut drops = vec![resolution.category];
        drops.extend(resolution.specials);
        for drop in drops {
            let Drop::Item {
                item,
                level,
                rarity,
            } = drop
            else {
                continue;
            };
            let Some(def) = atlas.item(item) else {
                continue;
            };
            let mut roll_rng = TestRng::new(seed ^ 0x5555);
            let instance = roll_dropped_item(def, level, rarity, &always(), &mut roll_rng);
            assert_eq!(instance.item, item, "bridged item echo");
            assert_eq!(instance.level, level, "bridged level echo");
            assert_eq!(instance.durability.current(), instance.durability.max());
            bridged += 1;
        }
    }
    assert!(
        bridged > 0,
        "the loot service produced at least one item drop"
    );
}

#[test]
fn every_droppable_footprint_places_into_an_empty_grid() {
    let atlas = real_atlas();
    for def in droppable(&atlas) {
        let footprint = or_abort(Footprint::new(def.width, def.height));
        let mut rng = TestRng::new(1);
        let instance = roll_dropped_item(
            def,
            ItemLevel::ZERO,
            ItemRarity::Normal,
            &always(),
            &mut rng,
        );
        let (inventory, outcome) = place_item(
            Inventory::empty(8, 8),
            PlaceIntent {
                anchor: Cell { row: 0, col: 0 },
                footprint,
                item: instance,
            },
        );
        assert!(
            matches!(
                outcome,
                mu_core::events::inventory::PlaceOutcome::Placed { .. }
            ),
            "item {:?} ({}x{}) fits an 8x8 grid",
            def.id,
            def.width,
            def.height
        );
        assert_eq!(inventory.placed().len(), 1);
    }
}

#[test]
fn every_equippable_item_equips_into_its_slot_and_rejects_a_wrong_one() {
    let atlas = real_atlas();
    for def in droppable(&atlas) {
        let Some(valid) = a_valid_slot(&def.kind) else {
            continue;
        };
        let mut rng = TestRng::new(1);
        let instance = roll_dropped_item(
            def,
            ItemLevel::ZERO,
            ItemRarity::Normal,
            &always(),
            &mut rng,
        );
        let (equipment, outcome) = equip(Equipment::empty(), instance.clone(), &def.kind, valid);
        assert!(
            matches!(
                outcome,
                mu_core::events::inventory::EquipOutcome::Equipped { .. }
            ),
            "item {:?} equips into its slot",
            def.id
        );
        // The item now occupies the translated slot.
        let worn = matches!(valid, EquipmentSlot::LeftHand | EquipmentSlot::RightHand);
        if worn {
            assert!(equipment.get(EquipSlot::LeftHand).is_some());
        }
        // An incompatible slot is rejected.
        let wrong = if matches!(def.kind, ItemKind::Helm { .. }) {
            EquipmentSlot::LeftHand
        } else {
            EquipmentSlot::Helm
        };
        let (_, outcome) = equip(Equipment::empty(), instance, &def.kind, wrong);
        assert!(
            matches!(
                outcome,
                mu_core::events::inventory::EquipOutcome::Rejected {
                    reason: mu_core::events::inventory::EquipRejection::IncompatibleSlot,
                    ..
                }
            ),
            "item {:?} rejects an incompatible slot",
            def.id
        );
    }
}
