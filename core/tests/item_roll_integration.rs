//! Item-instance drop rolls end-to-end over the real regenerated `/data`.
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

use mu_core::components::active_effect::ActiveEffects;
use mu_core::components::class::CharacterClass;
use mu_core::components::equipment::Equipment;
use mu_core::components::interval::Interval;
use mu_core::components::inventory::{Cell, Footprint, Inventory};
use mu_core::components::item_instance::{
    AugmentSlot, CraftedAugment, ExcellentArmorSet, ExcellentCat, ExcellentOptions, ItemInstance,
    ItemInstanceError, RarityRoll, SkillRoll,
};
use mu_core::components::item_options::{
    AncientBonusLevel, ExcellentArmorOption, ExcellentCategory, SecondWingBonus,
};
use mu_core::components::item_quality::ItemRarity;
use mu_core::components::levels::OptionLevel;
use mu_core::components::movement::Movement;
use mu_core::components::placement::Placement;
use mu_core::components::pool::Pool;
use mu_core::components::spatial::Facing;
use mu_core::components::stats::Stats;
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
use mu_core::data::item_definitions::{ItemDefinition, ItemKind, WeaponHandling};
use mu_core::data::map_definitions::MapDefinition;
use mu_core::data::monster_definitions::MonsterDefinition;
use mu_core::data::npc_shops::MerchantShop;
use mu_core::data::option_roll::OptionRollPolicy;
use mu_core::data::skills::Skill;
use mu_core::data::spawns::Spawn;
use mu_core::data::special_drops::SpecialDropRecord;
use mu_core::data::terrain::{MapTerrain, TerrainBytes};
use mu_core::entities::monster_instance::MonsterInstance;
use mu_core::events::inventory::{EquipOutcome, EquipRejection};
use mu_core::events::loot::Drop;
use mu_core::services::inventory::{
    EquipmentConflict, InventoryConflict, PlaceIntent, Wearer, equip, place_item,
    reconcile_equipment, reconcile_inventory,
};
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
        shops: load::<MerchantShop>("npc_shops"),
        classes: load::<ClassRecord>("classes"),
        exp_tables: load::<ExpTable>("exp_tables"),
        game_config: load::<GameConfig>("game_config"),
        mini_games: DataFile {
            records: Vec::new(),
        },
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

/// A generator whose first `next_u32` is a fixed word, then a real `SplitMix`
/// stream. The loot category roll is the first draw and uses `next_u32`, so this
/// steers exactly that roll into a chosen category while every later draw (the
/// item pick, the specials) stays a genuine deterministic stream — the excellent
/// item is therefore picked from the gated pool as it is in production.
struct ForcedCategoryRng {
    first_u32: Option<u32>,
    inner: TestRng,
}

impl RngCore for ForcedCategoryRng {
    fn next_u64(&mut self) -> u64 {
        self.inner.next_u64()
    }

    fn next_u32(&mut self) -> u32 {
        match self.first_u32.take() {
            Some(word) => word,
            None => self.inner.next_u32(),
        }
    }

    fn fill_bytes(&mut self, dst: &mut [u8]) {
        self.inner.fill_bytes(dst);
    }
}

/// The mid-bucket random word that steers `uniform_below(DENOMINATOR)` to
/// `target`. The rng seam's documented Lemire mapping is
/// `uniform_below(d) == (word * d) >> 32`; the mid-bucket choice keeps the draw
/// off the rejection path, so the forced category costs exactly one `next_u32`.
fn category_word_for(target: u32) -> u32 {
    let denom = u64::from(ChancePer10000::DENOMINATOR);
    let word = (u64::from(2 * target + 1) << 32) / (2 * denom);
    or_abort(u32::try_from(word))
}

/// A level-100 victim on Lorencia standing in for a real kill; only its number,
/// map, and level drive drop resolution.
fn drop_victim(atlas: &Atlas) -> MonsterInstance {
    MonsterInstance {
        number: or_abort(atlas.monsters().next().ok_or("the dataset ships monsters")).number,
        placement: Placement {
            position: TileCoord::new(10, 10).to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: MapNumber(0),
        },
        health: Pool::full(1),
        anchor: TileCoord::new(10, 10).to_world(),
        next_action: Tick(0),
        active_effects: ActiveEffects::EMPTY,
    }
}

// --- Injected policy extremes (never the review-flagged defaults). ---

fn always() -> OptionRollPolicy {
    OptionRollPolicy {
        item_option_roll_per_10000: ChancePer10000::ALWAYS,
        luck_roll_per_10000: ChancePer10000::ALWAYS,
        extra_excellent_option_roll_per_10000: ChancePer10000::ALWAYS,
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
        // Ammunition rides a hand slot beside its bow/crossbow (W-EQUIP).
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Staff { .. }
        | ItemKind::Shield { .. }
        | ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. } => Some(EquipmentSlot::LeftHand),
        ItemKind::Helm { .. } => Some(EquipmentSlot::Helm),
        ItemKind::BodyArmor { .. } => Some(EquipmentSlot::Armor),
        ItemKind::Pants { .. } => Some(EquipmentSlot::Pants),
        ItemKind::Gloves { .. } => Some(EquipmentSlot::Gloves),
        ItemKind::Boots { .. } => Some(EquipmentSlot::Boots),
        ItemKind::Wings { .. } => Some(EquipmentSlot::Wings),
        ItemKind::Pet { .. } => Some(EquipmentSlot::Pet),
        ItemKind::Pendant { .. } => Some(EquipmentSlot::Pendant),
        ItemKind::Ring { .. } | ItemKind::TransformationRing { .. } => Some(EquipmentSlot::Ring1),
        ItemKind::Orb { .. }
        | ItemKind::SkillScroll { .. }
        | ItemKind::Jewel { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => None,
    }
}

/// The eight-class roster in declaration order — the sweep an eligible wearer
/// is picked from.
const ROSTER: [CharacterClass; 8] = [
    CharacterClass::DarkWizard,
    CharacterClass::SoulMaster,
    CharacterClass::DarkKnight,
    CharacterClass::BladeKnight,
    CharacterClass::FairyElf,
    CharacterClass::MuseElf,
    CharacterClass::MagicGladiator,
    CharacterClass::DarkLord,
];

/// A maxed-out wearer of `class` — level cap, every stat at the u16 ceiling —
/// so only geometry or the class list can refuse.
fn maxed(class: CharacterClass) -> Wearer {
    Wearer {
        class,
        level: or_abort(Level::new(400)),
        stats: Stats::Standard {
            strength: u16::MAX,
            agility: u16::MAX,
            vitality: u16::MAX,
            energy: u16::MAX,
        },
    }
}

/// The most-qualified wearer for `def`: its first admitted class, maxed.
/// `None` when the qualified list is empty (the Red Wing Summoner backports —
/// wearable by no pre-S3 class) or the kind is non-wearable.
fn an_eligible_wearer(def: &ItemDefinition) -> Option<Wearer> {
    let classes = def.kind.classes()?;
    ROSTER
        .into_iter()
        .find(|&class| classes.allows(class))
        .map(maxed)
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
                    assert_eq!(
                        instance.reconcile(Some(expected), AugmentSlot::None),
                        Ok(())
                    );
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
        active_effects: ActiveEffects::EMPTY,
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
        let Some(wearer) = an_eligible_wearer(def) else {
            // An empty qualified-class list (the Red Wing Summoner backports)
            // is wearable by NO pre-S3 class — the live gate refuses every
            // wearer with ClassMismatch, an authentic data fact.
            let (_, outcome) = equip(
                Equipment::empty(),
                instance,
                def,
                valid,
                &atlas,
                &maxed(CharacterClass::DarkKnight),
            );
            assert!(
                matches!(
                    outcome,
                    EquipOutcome::Rejected {
                        reason: EquipRejection::ClassMismatch,
                        ..
                    }
                ),
                "empty-class item {:?} is wearable by no one",
                def.id
            );
            continue;
        };
        let (equipment, outcome) = equip(
            Equipment::empty(),
            instance.clone(),
            def,
            valid,
            &atlas,
            &wearer,
        );
        assert!(
            matches!(
                outcome,
                mu_core::events::inventory::EquipOutcome::Equipped { .. }
            ),
            "item {:?} equips into its slot for an eligible wearer",
            def.id
        );
        // The item now occupies the translated slot.
        let worn = matches!(valid, EquipmentSlot::LeftHand | EquipmentSlot::RightHand);
        if worn {
            assert!(equipment.get(EquipmentSlot::LeftHand).is_some());
        }
        // An incompatible slot is rejected — capability outranks eligibility,
        // so the reason is IncompatibleSlot even for the eligible wearer.
        let wrong = if matches!(def.kind, ItemKind::Helm { .. }) {
            EquipmentSlot::LeftHand
        } else {
            EquipmentSlot::Helm
        };
        let (_, outcome) = equip(Equipment::empty(), instance, def, wrong, &atlas, &wearer);
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

#[test]
fn an_excellent_drop_only_carries_excellent_capable_kinds() {
    // I3: the loot pool gates the excellent category on excellent-capability, so
    // an excellent `Drop::Item` can only ever reference a kind that has an
    // excellent set. Force the category into the excellent bucket (rate 1/10000
    // naturally), then let the pick draw many different items across seeds; every
    // one must be excellent-capable.
    let atlas = real_atlas();
    let config = atlas.drop_config();
    let excellent_bucket_start = u32::from(config.money_roll().numerator())
        + u32::from(config.item_roll().numerator())
        + u32::from(config.jewel_roll().numerator());
    let forced = category_word_for(excellent_bucket_start);
    let victim = drop_victim(&atlas);
    let victim_level = Level::new(100).unwrap();

    let mut excellent_items = 0u32;
    for seed in 0u64..256 {
        let mut rng = ForcedCategoryRng {
            first_u32: Some(forced),
            inner: TestRng::new(seed),
        };
        let resolution = resolve_kill_drops(&victim, victim_level, Exp(0), &atlas, &mut rng);
        // Only the category slot can carry an excellent item; specials and the
        // jewel roster are always Normal.
        if let Drop::Item {
            item,
            rarity: ItemRarity::Excellent,
            ..
        } = resolution.category
        {
            let def = atlas
                .item(item)
                .expect("an excellent drop resolves in the atlas");
            assert!(
                expected_excellent(&def.kind).is_some(),
                "excellent drop {:?} must be an excellent-capable kind",
                def.id
            );
            excellent_items += 1;
        }
    }
    assert!(
        excellent_items > 0,
        "the forced excellent category yields excellent item drops over the real pool"
    );
}

#[test]
fn the_normal_drop_pool_still_carries_excellent_incapable_kinds() {
    // I3 gates only the excellent category: the underlying level pool a Normal
    // item drop draws from is unrestricted, so it still contains kinds with no
    // excellent set (wings, pets, ammunition, jewels, consumables, ...). This is
    // what the excellent gate deliberately excludes — proving the gate is not a
    // no-op over the shipped data.
    let atlas = real_atlas();
    let mut saw_incapable_droppable = false;
    for low in 0u8..=120 {
        let window = Interval::spanning(low, low.saturating_add(11));
        for id in atlas.drop_pool().in_window(window) {
            let def = atlas.item(id).expect("a pooled drop resolves in the atlas");
            if def.drops_from_monsters && expected_excellent(&def.kind).is_none() {
                saw_incapable_droppable = true;
            }
        }
    }
    assert!(
        saw_incapable_droppable,
        "the unfiltered level pool carries excellent-incapable droppable items"
    );
}

// --- I1: two-handed dual-hand occupancy over the real dataset. ---

/// The first shipped definition whose kind matches `want`.
fn find_kind(atlas: &Atlas, want: impl Fn(&ItemKind) -> bool) -> &ItemDefinition {
    or_abort(
        atlas
            .items()
            .find(|def| want(&def.kind))
            .ok_or("the dataset ships the kind under test"),
    )
}

/// A rolled instance of `def` (identity is all the equip rules read).
fn instance_of(def: &ItemDefinition, seed: u64) -> ItemInstance {
    roll_dropped_item(
        def,
        ItemLevel::ZERO,
        ItemRarity::Normal,
        &always(),
        &mut TestRng::new(seed),
    )
}

fn is_two_handed_kind(kind: &ItemKind) -> bool {
    matches!(
        kind,
        ItemKind::Weapon {
            handling: WeaponHandling::TwoHanded,
            ..
        } | ItemKind::Bow { .. }
            | ItemKind::Crossbow { .. }
    )
}

fn is_one_handed_weapon(kind: &ItemKind) -> bool {
    matches!(
        kind,
        ItemKind::Weapon {
            handling: WeaponHandling::OneHanded,
            ..
        }
    )
}

#[test]
fn a_two_handed_weapon_requires_a_free_paired_hand() {
    let atlas = real_atlas();
    let two_handed = find_kind(&atlas, is_two_handed_kind);
    let one_handed = find_kind(&atlas, is_one_handed_weapon);

    let (equipment, occupied) = equip(
        Equipment::empty(),
        instance_of(one_handed, 1),
        one_handed,
        EquipmentSlot::RightHand,
        &atlas,
        &or_abort(an_eligible_wearer(one_handed).ok_or("one-hander has a class")),
    );
    assert!(matches!(occupied, EquipOutcome::Equipped { .. }));

    let (equipment, outcome) = equip(
        equipment,
        instance_of(two_handed, 2),
        two_handed,
        EquipmentSlot::LeftHand,
        &atlas,
        &or_abort(an_eligible_wearer(two_handed).ok_or("two-hander has a class")),
    );
    assert!(
        matches!(
            outcome,
            EquipOutcome::Rejected {
                reason: EquipRejection::TwoHandedConflict,
                ..
            }
        ),
        "a two-handed weapon needs its paired hand free"
    );
    assert!(equipment.get(EquipmentSlot::LeftHand).is_none());
}

#[test]
fn a_two_handed_weapon_equips_into_a_fully_empty_pair() {
    let atlas = real_atlas();
    let two_handed = find_kind(&atlas, is_two_handed_kind);
    let (equipment, outcome) = equip(
        Equipment::empty(),
        instance_of(two_handed, 3),
        two_handed,
        EquipmentSlot::LeftHand,
        &atlas,
        &or_abort(an_eligible_wearer(two_handed).ok_or("two-hander has a class")),
    );
    assert!(matches!(
        outcome,
        EquipOutcome::Equipped {
            slot: EquipmentSlot::LeftHand
        }
    ));
    assert!(equipment.get(EquipmentSlot::LeftHand).is_some());
}

#[test]
fn an_offhand_cannot_join_a_hand_paired_with_a_two_hander() {
    let atlas = real_atlas();
    let two_handed = find_kind(&atlas, is_two_handed_kind);
    let shield = find_kind(&atlas, |kind| matches!(kind, ItemKind::Shield { .. }));

    let (equipment, _) = equip(
        Equipment::empty(),
        instance_of(two_handed, 4),
        two_handed,
        EquipmentSlot::LeftHand,
        &atlas,
        &or_abort(an_eligible_wearer(two_handed).ok_or("two-hander has a class")),
    );
    let (equipment, outcome) = equip(
        equipment,
        instance_of(shield, 5),
        shield,
        EquipmentSlot::RightHand,
        &atlas,
        &or_abort(an_eligible_wearer(shield).ok_or("shield has a class")),
    );
    assert!(
        matches!(
            outcome,
            EquipOutcome::Rejected {
                reason: EquipRejection::TwoHandedConflict,
                ..
            }
        ),
        "no item may share a hand pair with a worn two-handed weapon"
    );
    assert!(equipment.get(EquipmentSlot::RightHand).is_none());
}

#[test]
fn a_bow_admits_ammunition_beside_it_but_never_a_shield() {
    // The SANCTIONED ammo geometry: a bow is two-handed EXCEPT that its
    // paired hand carries the quiver — bow+ammo equips (either order), while
    // bow+shield still conflicts and a sword never sits beside arrows.
    let atlas = real_atlas();
    let bow = find_kind(&atlas, |kind| matches!(kind, ItemKind::Bow { .. }));
    let arrows = find_kind(&atlas, |kind| matches!(kind, ItemKind::Arrows { .. }));
    let shield = find_kind(&atlas, |kind| matches!(kind, ItemKind::Shield { .. }));
    let one_handed = find_kind(&atlas, is_one_handed_weapon);
    let elf = or_abort(an_eligible_wearer(bow).ok_or("bow has a class"));

    // Bow first, then ammunition into the paired hand.
    let (equipment, worn_bow) = equip(
        Equipment::empty(),
        instance_of(bow, 20),
        bow,
        EquipmentSlot::RightHand,
        &atlas,
        &elf,
    );
    assert!(matches!(worn_bow, EquipOutcome::Equipped { .. }));
    let (equipment, worn_ammo) = equip(
        equipment,
        instance_of(arrows, 21),
        arrows,
        EquipmentSlot::LeftHand,
        &atlas,
        &elf,
    );
    assert!(
        matches!(worn_ammo, EquipOutcome::Equipped { .. }),
        "ammunition rides the hand paired with a bow"
    );
    assert!(equipment.get(EquipmentSlot::LeftHand).is_some());
    assert_eq!(reconcile_equipment(&equipment, &atlas), Ok(()));

    // Ammunition first, then the bow into the paired hand.
    let (equipment, worn_ammo) = equip(
        Equipment::empty(),
        instance_of(arrows, 22),
        arrows,
        EquipmentSlot::LeftHand,
        &atlas,
        &elf,
    );
    assert!(matches!(worn_ammo, EquipOutcome::Equipped { .. }));
    let (_, worn_bow) = equip(
        equipment,
        instance_of(bow, 23),
        bow,
        EquipmentSlot::RightHand,
        &atlas,
        &elf,
    );
    assert!(
        matches!(worn_bow, EquipOutcome::Equipped { .. }),
        "a bow accepts the hand paired with its ammunition"
    );

    // A shield beside the bow is still the two-handed conflict.
    let (with_bow, _) = equip(
        Equipment::empty(),
        instance_of(bow, 24),
        bow,
        EquipmentSlot::RightHand,
        &atlas,
        &elf,
    );
    let (_, refused) = equip(
        with_bow,
        instance_of(shield, 25),
        shield,
        EquipmentSlot::LeftHand,
        &atlas,
        &or_abort(an_eligible_wearer(shield).ok_or("shield has a class")),
    );
    assert!(matches!(
        refused,
        EquipOutcome::Rejected {
            reason: EquipRejection::TwoHandedConflict,
            ..
        }
    ));

    // A sword cannot sit beside arrows either.
    let (with_ammo, _) = equip(
        Equipment::empty(),
        instance_of(arrows, 26),
        arrows,
        EquipmentSlot::LeftHand,
        &atlas,
        &elf,
    );
    let (_, refused) = equip(
        with_ammo,
        instance_of(one_handed, 27),
        one_handed,
        EquipmentSlot::RightHand,
        &atlas,
        &or_abort(an_eligible_wearer(one_handed).ok_or("one-hander has a class")),
    );
    assert!(matches!(
        refused,
        EquipOutcome::Rejected {
            reason: EquipRejection::TwoHandedConflict,
            ..
        }
    ));
}

#[test]
fn two_one_handed_items_fill_both_hands() {
    let atlas = real_atlas();
    let one_handed = find_kind(&atlas, is_one_handed_weapon);
    let shield = find_kind(&atlas, |kind| matches!(kind, ItemKind::Shield { .. }));

    let (equipment, first) = equip(
        Equipment::empty(),
        instance_of(one_handed, 6),
        one_handed,
        EquipmentSlot::LeftHand,
        &atlas,
        &or_abort(an_eligible_wearer(one_handed).ok_or("one-hander has a class")),
    );
    assert!(matches!(first, EquipOutcome::Equipped { .. }));
    let (equipment, second) = equip(
        equipment,
        instance_of(shield, 7),
        shield,
        EquipmentSlot::RightHand,
        &atlas,
        &or_abort(an_eligible_wearer(shield).ok_or("shield has a class")),
    );
    assert!(
        matches!(second, EquipOutcome::Equipped { .. }),
        "two one-handed items fill both hands"
    );
    assert!(equipment.get(EquipmentSlot::LeftHand).is_some());
    assert!(equipment.get(EquipmentSlot::RightHand).is_some());
}

#[test]
fn equipping_an_occupied_slot_is_rejected() {
    let atlas = real_atlas();
    let helm = find_kind(&atlas, |kind| matches!(kind, ItemKind::Helm { .. }));
    let wearer = or_abort(an_eligible_wearer(helm).ok_or("helm has a class"));

    let (equipment, first) = equip(
        Equipment::empty(),
        instance_of(helm, 8),
        helm,
        EquipmentSlot::Helm,
        &atlas,
        &wearer,
    );
    assert!(matches!(
        first,
        EquipOutcome::Equipped {
            slot: EquipmentSlot::Helm
        }
    ));
    let (equipment, second) = equip(
        equipment,
        instance_of(helm, 9),
        helm,
        EquipmentSlot::Helm,
        &atlas,
        &wearer,
    );
    assert!(matches!(
        second,
        EquipOutcome::Rejected {
            reason: EquipRejection::SlotOccupied,
            ..
        }
    ));
    assert!(equipment.get(EquipmentSlot::Helm).is_some());
}

#[test]
fn reconcile_equipment_accepts_legal_and_rejects_two_handed_with_offhand() {
    let atlas = real_atlas();
    let two_handed = find_kind(&atlas, is_two_handed_kind);
    let one_handed = find_kind(&atlas, is_one_handed_weapon);
    let shield = find_kind(&atlas, |kind| matches!(kind, ItemKind::Shield { .. }));

    // A legal sword+shield set reconciles cleanly.
    let legal = Equipment::empty()
        .with(EquipmentSlot::LeftHand, instance_of(one_handed, 10))
        .with(EquipmentSlot::RightHand, instance_of(shield, 11));
    assert_eq!(reconcile_equipment(&legal, &atlas), Ok(()));

    // A hand-crafted two-handed + offhand set is rejected at reload.
    let illegal = Equipment::empty()
        .with(EquipmentSlot::LeftHand, instance_of(two_handed, 12))
        .with(EquipmentSlot::RightHand, instance_of(shield, 13));
    assert_eq!(
        reconcile_equipment(&illegal, &atlas),
        Err(EquipmentConflict::TwoHandedWithOffhand)
    );

    // A lone two-handed weapon with its paired hand empty reconciles cleanly.
    let lone = Equipment::empty().with(EquipmentSlot::LeftHand, instance_of(two_handed, 14));
    assert_eq!(reconcile_equipment(&lone, &atlas), Ok(()));
}

#[test]
fn reconcile_equipment_rejects_a_worn_item_in_a_slot_its_kind_forbids() {
    let atlas = real_atlas();
    let helm = find_kind(&atlas, |kind| matches!(kind, ItemKind::Helm { .. }));
    // A helm forced into the Pet slot: geometry the Equipment component permits,
    // but the shared kind→slot rule (INV-1) forbids at reload — the check the
    // hand-pair-only reconcile previously skipped.
    let forged = Equipment::empty().with(EquipmentSlot::Pet, instance_of(helm, 1));
    assert_eq!(
        reconcile_equipment(&forged, &atlas),
        Err(EquipmentConflict::WrongSlot)
    );
}

#[test]
fn reconcile_equipment_rejects_a_worn_item_whose_options_contradict_its_definition() {
    let atlas = real_atlas();
    let weapon = find_kind(&atlas, is_one_handed_weapon);
    // A weapon forged with an ARMOR excellent set: its definition rolls the
    // WEAPON category, so ItemInstance::reconcile (INV-5) rejects it. This is the
    // production caller that check previously lacked — exercised over real /data.
    let mut forged_instance = instance_of(weapon, 1);
    forged_instance.roll = RarityRoll::Excellent {
        options: ExcellentOptions::Armor {
            options: or_abort(ExcellentArmorSet::from_options([
                ExcellentArmorOption::MaxHealth,
            ])),
        },
    };
    let worn = Equipment::empty().with(EquipmentSlot::RightHand, forged_instance);
    assert_eq!(
        reconcile_equipment(&worn, &atlas),
        Err(EquipmentConflict::ItemInvariant(
            ItemInstanceError::ExcellentSetCategoryMismatch
        ))
    );
}

#[test]
fn reconcile_equipment_rejects_a_worn_wing_whose_augment_contradicts_its_definition() {
    let atlas = real_atlas();
    // A first wing carries augment slot None — its definition permits no crafted
    // augment. Forging a wing bonus onto it is the crafted-augment forgery the
    // reload boundary must refuse: worn_item_ok cross-references the instance's
    // augment against the definition's own augment_slot (INV-6), over real /data.
    let first_wing = find_kind(&atlas, |kind| {
        matches!(kind, ItemKind::Wings { .. }) && kind.augment_slot() == AugmentSlot::None
    });
    let mut forged = instance_of(first_wing, 1);
    forged.augment = CraftedAugment::WingBonus {
        bonus: SecondWingBonus::MaxHealth,
    };
    let worn = Equipment::empty().with(EquipmentSlot::Wings, forged);
    assert_eq!(
        reconcile_equipment(&worn, &atlas),
        Err(EquipmentConflict::ItemInvariant(
            ItemInstanceError::AugmentSlotMismatch
        ))
    );

    // The mirror (FIX 1's cross-check): a second wing carries augment slot
    // WingBonus, so a matching wing bonus — exactly what the chaos-machine mint
    // now derives from that same slot — reconciles clean. A legitimately crafted
    // item is never false-rejected at reload.
    let second_wing = find_kind(&atlas, |kind| {
        matches!(kind, ItemKind::Wings { .. }) && kind.augment_slot() == AugmentSlot::WingBonus
    });
    let mut legit = instance_of(second_wing, 2);
    legit.augment = CraftedAugment::WingBonus {
        bonus: SecondWingBonus::MaxHealth,
    };
    let worn = Equipment::empty().with(EquipmentSlot::Wings, legit);
    assert_eq!(reconcile_equipment(&worn, &atlas), Ok(()));
}

#[test]
fn live_equip_rejects_a_malformed_item_through_the_shared_proof() {
    let atlas = real_atlas();
    let weapon = find_kind(&atlas, is_one_handed_weapon);
    // The same forged weapon live equip runs the shared `worn_item_ok` proof
    // over — the reconcile check runs before class/requirement, so a corrupt
    // instance is refused with `MalformedItem`, not silently worn.
    let mut forged = instance_of(weapon, 1);
    forged.roll = RarityRoll::Excellent {
        options: ExcellentOptions::Armor {
            options: or_abort(ExcellentArmorSet::from_options([
                ExcellentArmorOption::MaxHealth,
            ])),
        },
    };
    let (_, outcome) = equip(
        Equipment::empty(),
        forged,
        weapon,
        EquipmentSlot::RightHand,
        &atlas,
        &maxed(CharacterClass::DarkKnight),
    );
    assert!(
        matches!(
            outcome,
            EquipOutcome::Rejected {
                reason: EquipRejection::MalformedItem,
                ..
            }
        ),
        "a malformed worn item is refused by the shared live gate"
    );
}

#[test]
fn reconcile_inventory_rejects_a_tampered_footprint_and_passes_a_faithful_one() {
    let atlas = real_atlas();
    let weapon = find_kind(&atlas, is_one_handed_weapon);
    let anchor = Cell { row: 0, col: 0 };
    let faithful = or_abort(Footprint::new(weapon.width, weapon.height));
    // A footprint that cannot equal the real dimensions — the shrink-to-pack-more
    // cheat.
    let (tampered_w, tampered_h) = if weapon.width != 1 || weapon.height != 1 {
        (1, 1)
    } else {
        (2, 1)
    };
    let tampered = or_abort(Footprint::new(tampered_w, tampered_h));

    let (faithful_inv, _) = place_item(
        Inventory::empty(15, 8),
        PlaceIntent {
            anchor,
            footprint: faithful,
            item: instance_of(weapon, 1),
        },
    );
    assert_eq!(reconcile_inventory(&faithful_inv, &atlas), Ok(()));

    let (tampered_inv, _) = place_item(
        Inventory::empty(15, 8),
        PlaceIntent {
            anchor,
            footprint: tampered,
            item: instance_of(weapon, 2),
        },
    );
    assert_eq!(
        reconcile_inventory(&tampered_inv, &atlas),
        Err(InventoryConflict::FootprintMismatch { at: anchor })
    );
}
