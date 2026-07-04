//! Chaos-machine mixes end-to-end over the real regenerated `/data`.
//!
//! One success and one failure path per catalog record (all 10), the recipe
//! inference precedence windows of the design's §4.1 (claim-all fall-throughs
//! included), casualty accounting (every input in exactly one place), zen
//! arithmetic (exact fees, exact-balance-to-zero, never-refunded), determinism
//! bit-equality under a fixed seed, and the classic price routing with one
//! priced instance per `ItemKind` row at exact pinned values.
//!
//! Branch pinning: under the `SplitMix64` stream, seed 24's first roll word is
//! below 3 (passes every rate ≥ 3) and seed 8's is at least 90 (fails every
//! rate ≤ 90) — the two universal seeds the branch-pinned scenarios ride.
//! Rate-100 windows succeed under ANY seed with no roll draw (D1).
//!
//! This file carries its own dataset loader (the movement suite's `common`
//! helpers are unused here); load failures route through `or_abort` so no
//! banned suppressor is needed outside a `#[test]` body.

use std::io::Write;
use std::path::PathBuf;

use rand_core::RngCore;
use serde::de::DeserializeOwned;

use mu_core::components::item_instance::{
    CraftedAugment, Durability, ExcellentArmorSet, ExcellentOptions, ItemInstance, LuckRoll,
    RarityRoll, RolledNormalOption, SkillRoll,
};
use mu_core::components::item_options::NormalOption;
use mu_core::components::item_quality::ItemRarity;
use mu_core::components::levels::OptionLevel;
use mu_core::components::units::{ItemLevel, MapNumber, Zen};
use mu_core::data::ancient_sets::AncientSet;
use mu_core::data::atlas::{Atlas, StaticData};
use mu_core::data::box_drops::BoxDrop;
use mu_core::data::chaos_mixes::ChaosMix;
use mu_core::data::classes::ClassRecord;
use mu_core::data::common::{DataFile, ItemRef};
use mu_core::data::exp_tables::ExpTable;
use mu_core::data::game_config::GameConfig;
use mu_core::data::gates_warps::GateWarpRecord;
use mu_core::data::item_definitions::ItemDefinition;
use mu_core::data::map_definitions::MapDefinition;
use mu_core::data::monster_definitions::MonsterDefinition;
use mu_core::data::skills::Skill;
use mu_core::data::spawns::Spawn;
use mu_core::data::special_drops::SpecialDropRecord;
use mu_core::data::terrain::{MapTerrain, TerrainBytes};
use mu_core::events::craft::{Casualty, MixOutcome, RejectReason};
use mu_core::services::craft::mix;
use mu_core::services::item_rules::max_durability;
use mu_core::services::price::{buying_price, old_buying_price};

// --- Self-contained dataset harness (load failures abort, never unwrap). ---

fn or_abort<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            let mut stderr = std::io::stderr();
            let _ = writeln!(stderr, "craft_integration harness: {error}");
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

/// The universal passing seed: its first roll word is below 3, so it passes
/// every rate this suite reaches (all ≥ 3).
const SEED_SUCCESS: u64 = 24;
/// The universal failing seed: its first roll word is at least 90, so it fails
/// every rate this suite reaches (all ≤ 90).
const SEED_FAILURE: u64 = 8;
/// A comfortable balance for every fee in the suite.
const BALANCE: Zen = Zen(100_000_000);

// --- Concrete catalog identities (from the shipped `/data`). ---

const SWORD: ItemRef = ItemRef {
    group: 0,
    number: 3,
};
const CHAOS_AXE: ItemRef = ItemRef {
    group: 2,
    number: 6,
};
const CHAOS_BOW: ItemRef = ItemRef {
    group: 4,
    number: 6,
};
const CHAOS_STAFF: ItemRef = ItemRef {
    group: 5,
    number: 7,
};
const HELM: ItemRef = ItemRef {
    group: 7,
    number: 0,
};
const GLOVES: ItemRef = ItemRef {
    group: 10,
    number: 0,
};
const FAIRY_WINGS: ItemRef = ItemRef {
    group: 12,
    number: 0,
};
const HEAVEN_WINGS: ItemRef = ItemRef {
    group: 12,
    number: 1,
};
const SATAN_WINGS: ItemRef = ItemRef {
    group: 12,
    number: 2,
};
const CAPE_OF_LORD: ItemRef = ItemRef {
    group: 13,
    number: 30,
};
const JEWEL_OF_CHAOS: ItemRef = ItemRef {
    group: 12,
    number: 15,
};
const JEWEL_OF_BLESS: ItemRef = ItemRef {
    group: 14,
    number: 13,
};
const JEWEL_OF_SOUL: ItemRef = ItemRef {
    group: 14,
    number: 14,
};
const JEWEL_OF_CREATION: ItemRef = ItemRef {
    group: 14,
    number: 22,
};
const HORN_OF_UNIRIA: ItemRef = ItemRef {
    group: 13,
    number: 2,
};
const HORN_OF_DINORANT: ItemRef = ItemRef {
    group: 13,
    number: 3,
};
const LOCHS_FEATHER: ItemRef = ItemRef {
    group: 13,
    number: 14,
};
const DEVILS_EYE: ItemRef = ItemRef {
    group: 14,
    number: 17,
};
const DEVILS_KEY: ItemRef = ItemRef {
    group: 14,
    number: 18,
};
const DEVILS_INVITATION: ItemRef = ItemRef {
    group: 14,
    number: 19,
};
const ARCHANGEL_SCROLL: ItemRef = ItemRef {
    group: 13,
    number: 16,
};
const BLOOD_BONE: ItemRef = ItemRef {
    group: 13,
    number: 17,
};
const INVISIBILITY_CLOAK: ItemRef = ItemRef {
    group: 13,
    number: 18,
};
const STAT_FRUIT: ItemRef = ItemRef {
    group: 13,
    number: 15,
};

const FIRST_WINGS: [ItemRef; 3] = [FAIRY_WINGS, HEAVEN_WINGS, SATAN_WINGS];
const SECOND_WINGS: [ItemRef; 4] = [
    ItemRef {
        group: 12,
        number: 3,
    },
    ItemRef {
        group: 12,
        number: 4,
    },
    ItemRef {
        group: 12,
        number: 5,
    },
    ItemRef {
        group: 12,
        number: 6,
    },
];
const CHAOS_WEAPONS: [ItemRef; 3] = [CHAOS_AXE, CHAOS_BOW, CHAOS_STAFF];

// --- Instance builders over the real definitions. ---

fn item(atlas: &Atlas, id: ItemRef, level: u8) -> ItemInstance {
    let def = or_abort(atlas.item(id).ok_or(format!("unknown item {id:?}")));
    ItemInstance {
        item: id,
        level: or_abort(ItemLevel::new(level)),
        roll: RarityRoll::Normal,
        normal_option: None,
        luck: LuckRoll::Plain,
        skill: SkillRoll::NoSkill,
        durability: Durability::full(def.durability),
        augment: CraftedAugment::None,
    }
}

fn with_option(mut instance: ItemInstance, option: NormalOption) -> ItemInstance {
    instance.normal_option = Some(RolledNormalOption {
        option,
        level: OptionLevel::L1,
    });
    instance
}

fn excellent(mut instance: ItemInstance) -> ItemInstance {
    instance.roll = RarityRoll::Excellent {
        options: ExcellentOptions::Armor {
            options: ExcellentArmorSet::with_first(
                mu_core::components::item_options::ExcellentArmorOption::MaxHealth,
                [],
            ),
        },
    };
    instance
}

/// A +6 option-bearing sword — old value 26,100 zen (the pinned general-branch
/// price with the width discount and an L1 option).
fn option_sword(atlas: &Atlas) -> ItemInstance {
    with_option(item(atlas, SWORD, 6), NormalOption::PhysicalDamage)
}

fn run(atlas: &Atlas, placed: Vec<ItemInstance>, zen: Zen, seed: u64) -> MixOutcome {
    let mut rng = TestRng::new(seed);
    mix(placed, zen, atlas, &mut rng)
}

fn destroyed_refs(casualties: &[Casualty]) -> Vec<ItemRef> {
    casualties
        .iter()
        .filter_map(|casualty| match casualty {
            Casualty::Destroyed { item } => Some(*item),
            Casualty::Downgraded { .. } | Casualty::Returned { .. } => None,
        })
        .collect()
}

// --- One end-to-end mix per record: success and failure. ---

#[test]
fn chaos_weapon_mix_creates_a_catalog_weapon_and_charges_the_value_fee() {
    let atlas = real_atlas();
    // Old values: sword 26,100 + chaos 40,000 → rate 3, fee 30,000.
    let placed = vec![option_sword(&atlas), item(&atlas, JEWEL_OF_CHAOS, 0)];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success {
            fee,
            zen,
            created,
            returned,
        } => {
            assert_eq!(fee, Zen(30_000));
            assert_eq!(zen, Zen(BALANCE.0 - 30_000));
            assert!(CHAOS_WEAPONS.contains(&created.item));
            assert!(created.level.get() <= 4);
            let def = atlas.item(created.item).unwrap();
            let enhance = created.level.enhance_level().unwrap();
            assert_eq!(
                created.durability.max(),
                max_durability(def.durability, enhance, ItemRarity::Normal)
            );
            assert_eq!(created.durability.current(), created.durability.max());
            assert!(returned.is_empty());
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes rate 3")
        }
    }
}

#[test]
fn a_failed_chaos_weapon_mix_downgrades_the_sacrifice_and_destroys_the_jewel() {
    let atlas = real_atlas();
    let placed = vec![option_sword(&atlas), item(&atlas, JEWEL_OF_CHAOS, 0)];
    match run(&atlas, placed, BALANCE, SEED_FAILURE) {
        MixOutcome::Failed {
            fee,
            zen,
            casualties,
        } => {
            assert_eq!(fee, Zen(30_000));
            assert_eq!(zen, Zen(BALANCE.0 - 30_000), "the fee is never refunded");
            assert_eq!(casualties.len(), 2);
            match &casualties[0] {
                Casualty::Downgraded { item } => {
                    assert_eq!(item.item, SWORD);
                    assert!(item.level.get() <= 5, "downgraded strictly below +6");
                }
                Casualty::Destroyed { .. } | Casualty::Returned { .. } => {
                    panic!("the sacrifice downgrades, never vanishes")
                }
            }
            assert_eq!(destroyed_refs(&casualties), vec![JEWEL_OF_CHAOS]);
        }
        MixOutcome::Rejected { .. } | MixOutcome::Success { .. } => {
            panic!("seed {SEED_FAILURE} fails rate 3")
        }
    }
}

#[test]
fn an_over_stuffed_chaos_weapon_mix_saturates_to_certainty() {
    let atlas = real_atlas();
    // 26,100 + 40,000 + 60·100,000 old zen → 303 points, saturated to 100
    // (D2), which succeeds with no roll draw (D1) — even under the failing
    // seed. Fee: 10,000 × 100.
    let mut placed = vec![option_sword(&atlas), item(&atlas, JEWEL_OF_CHAOS, 0)];
    for _ in 0..60 {
        placed.push(item(&atlas, JEWEL_OF_BLESS, 0));
    }
    match run(&atlas, placed, BALANCE, SEED_FAILURE) {
        MixOutcome::Success { fee, .. } => assert_eq!(fee, Zen(1_000_000)),
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("rate 100 always succeeds")
        }
    }
}

#[test]
fn first_wings_mix_creates_a_first_wing_at_plus_zero() {
    let atlas = real_atlas();
    // Old values: staff 471,600 + chaos 40,000 → rate 25, fee 250,000.
    let staff = with_option(item(&atlas, CHAOS_STAFF, 7), NormalOption::WizardryDamage);
    let placed = vec![staff, item(&atlas, JEWEL_OF_CHAOS, 0)];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success { fee, created, .. } => {
            assert_eq!(fee, Zen(250_000));
            assert!(FIRST_WINGS.contains(&created.item));
            assert_eq!(created.level, ItemLevel::ZERO);
            assert_eq!(created.durability.current(), created.durability.max());
            // Wings carry no skill: the roll never grants one.
            assert_eq!(created.skill, SkillRoll::NoSkill);
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes rate 25")
        }
    }
}

#[test]
fn first_wings_extras_are_destroyed_on_both_outcomes() {
    let atlas = real_atlas();
    let staff = with_option(item(&atlas, CHAOS_STAFF, 7), NormalOption::WizardryDamage);
    // K3 on success: the extra sword is absent from the outcome entirely.
    let placed = vec![
        staff.clone(),
        option_sword(&atlas),
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success { returned, fee, .. } => {
            assert!(returned.is_empty(), "K3: extras never come back");
            // Rate 26 (471,600 + 26,100 + 40,000 over 20,000) → fee 260,000.
            assert_eq!(fee, Zen(260_000));
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes rate 26")
        }
    }
    // K3 on failure: the chaos weapon downgrades, the extra is destroyed.
    let placed = vec![staff, option_sword(&atlas), item(&atlas, JEWEL_OF_CHAOS, 0)];
    match run(&atlas, placed, BALANCE, SEED_FAILURE) {
        MixOutcome::Failed { casualties, .. } => {
            assert_eq!(casualties.len(), 3);
            match &casualties[0] {
                Casualty::Downgraded { item } => assert_eq!(item.item, CHAOS_STAFF),
                Casualty::Destroyed { .. } | Casualty::Returned { .. } => {
                    panic!("the chaos weapon downgrades")
                }
            }
            assert_eq!(destroyed_refs(&casualties), vec![SWORD, JEWEL_OF_CHAOS]);
        }
        MixOutcome::Rejected { .. } | MixOutcome::Success { .. } => {
            panic!("seed {SEED_FAILURE} fails rate 26")
        }
    }
}

#[test]
fn second_wings_mix_creates_a_second_wing_for_the_flat_fee() {
    let atlas = real_atlas();
    let placed = vec![
        item(&atlas, FAIRY_WINGS, 0),
        item(&atlas, LOCHS_FEATHER, 0),
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success {
            fee, created, zen, ..
        } => {
            assert_eq!(fee, Zen(5_000_000));
            assert_eq!(zen, Zen(BALANCE.0 - 5_000_000));
            assert!(SECOND_WINGS.contains(&created.item));
            assert_eq!(created.level, ItemLevel::ZERO);
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes the 13-point base rate")
        }
    }
}

#[test]
fn a_failed_second_wings_mix_destroys_everything() {
    let atlas = real_atlas();
    // Two excellent +8 gloves as fodder — the fee stays flat regardless.
    let placed = vec![
        item(&atlas, FAIRY_WINGS, 0),
        item(&atlas, LOCHS_FEATHER, 0),
        item(&atlas, JEWEL_OF_CHAOS, 0),
        excellent(item(&atlas, GLOVES, 8)),
        excellent(item(&atlas, GLOVES, 8)),
    ];
    match run(&atlas, placed, BALANCE, SEED_FAILURE) {
        MixOutcome::Failed {
            fee, casualties, ..
        } => {
            assert_eq!(fee, Zen(5_000_000));
            assert_eq!(casualties.len(), 5);
            assert_eq!(
                destroyed_refs(&casualties),
                vec![FAIRY_WINGS, LOCHS_FEATHER, JEWEL_OF_CHAOS, GLOVES, GLOVES],
                "wing, feather, jewel, and fodder are all destroyed"
            );
        }
        MixOutcome::Rejected { .. } | MixOutcome::Success { .. } => {
            panic!("seed {SEED_FAILURE} fails rate 34")
        }
    }
}

#[test]
fn a_plus_one_feather_crafts_the_cape_and_a_plus_zero_feather_a_second_wing() {
    let atlas = real_atlas();
    // The Monarch's Crest (+1 feather) satisfies Cape of Lord (crafting 24).
    let placed = vec![
        item(&atlas, HEAVEN_WINGS, 0),
        item(&atlas, LOCHS_FEATHER, 1),
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success { created, fee, .. } => {
            assert_eq!(created.item, CAPE_OF_LORD);
            assert_eq!(created.level, ItemLevel::ZERO);
            assert_eq!(fee, Zen(5_000_000));
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes the cape's base rate")
        }
    }
    // The +0 feather fails the crest gate and falls to Second Wings.
    let placed = vec![
        item(&atlas, HEAVEN_WINGS, 0),
        item(&atlas, LOCHS_FEATHER, 0),
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success { created, .. } => {
            assert!(SECOND_WINGS.contains(&created.item));
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("the +0 feather window matches Second Wings")
        }
    }
}

#[test]
fn a_failed_cape_mix_destroys_wing_crest_and_jewel() {
    let atlas = real_atlas();
    let placed = vec![
        item(&atlas, HEAVEN_WINGS, 0),
        item(&atlas, LOCHS_FEATHER, 1),
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_FAILURE) {
        MixOutcome::Failed {
            fee, casualties, ..
        } => {
            assert_eq!(fee, Zen(5_000_000));
            assert_eq!(
                destroyed_refs(&casualties),
                vec![HEAVEN_WINGS, LOCHS_FEATHER, JEWEL_OF_CHAOS]
            );
            assert_eq!(casualties.len(), 3);
        }
        MixOutcome::Rejected { .. } | MixOutcome::Success { .. } => {
            panic!("seed {SEED_FAILURE} fails the cape rate")
        }
    }
}

#[test]
fn plus_ten_upgrades_the_placed_item_in_place() {
    let atlas = real_atlas();
    let placed = vec![
        item(&atlas, HELM, 9),
        item(&atlas, JEWEL_OF_CHAOS, 0),
        item(&atlas, JEWEL_OF_BLESS, 0),
        item(&atlas, JEWEL_OF_SOUL, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success {
            fee,
            created,
            returned,
            ..
        } => {
            assert_eq!(fee, Zen(2_000_000));
            assert_eq!(created.item, HELM, "no new item — the placed one levels");
            assert_eq!(created.level.get(), 10);
            // The full gauge rescales to the new full maximum (34 → 51).
            assert_eq!(created.durability.current(), 51);
            assert_eq!(created.durability.max(), 51);
            assert!(returned.is_empty());
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes rate 50")
        }
    }
}

#[test]
fn a_failed_plus_ten_destroys_the_item_and_the_jewels() {
    let atlas = real_atlas();
    let placed = vec![
        item(&atlas, HELM, 9),
        item(&atlas, JEWEL_OF_CHAOS, 0),
        item(&atlas, JEWEL_OF_BLESS, 0),
        item(&atlas, JEWEL_OF_SOUL, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_FAILURE) {
        MixOutcome::Failed {
            fee, casualties, ..
        } => {
            assert_eq!(fee, Zen(2_000_000));
            assert_eq!(casualties.len(), 4);
            assert_eq!(
                destroyed_refs(&casualties),
                vec![HELM, JEWEL_OF_CHAOS, JEWEL_OF_BLESS, JEWEL_OF_SOUL],
                "the placed item is destroyed, never downgraded"
            );
        }
        MixOutcome::Rejected { .. } | MixOutcome::Success { .. } => {
            panic!("seed {SEED_FAILURE} fails rate 50")
        }
    }
}

#[test]
fn plus_eleven_needs_the_exact_two_two_jewel_counts() {
    let atlas = real_atlas();
    let placed = vec![
        item(&atlas, HELM, 10),
        item(&atlas, JEWEL_OF_CHAOS, 0),
        item(&atlas, JEWEL_OF_BLESS, 0),
        item(&atlas, JEWEL_OF_BLESS, 0),
        item(&atlas, JEWEL_OF_SOUL, 0),
        item(&atlas, JEWEL_OF_SOUL, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success { fee, created, .. } => {
            assert_eq!(fee, Zen(4_000_000));
            assert_eq!(created.item, HELM);
            assert_eq!(created.level.get(), 11);
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes rate 45")
        }
    }
    // One Bless short: the +11 claim fails and no lower recipe accepts the
    // window.
    let short = vec![
        item(&atlas, HELM, 10),
        item(&atlas, JEWEL_OF_CHAOS, 0),
        item(&atlas, JEWEL_OF_BLESS, 0),
        item(&atlas, JEWEL_OF_SOUL, 0),
        item(&atlas, JEWEL_OF_SOUL, 0),
    ];
    match run(&atlas, short, BALANCE, SEED_SUCCESS) {
        MixOutcome::Rejected { reason, items } => {
            assert_eq!(reason, RejectReason::NoRecipeMatch);
            assert_eq!(items.len(), 5, "every input handed back");
        }
        MixOutcome::Failed { .. } | MixOutcome::Success { .. } => {
            panic!("a short jewel count forms no recipe")
        }
    }
}

#[test]
fn dinorant_mix_always_carries_its_skill_and_gates_on_full_horns() {
    let atlas = real_atlas();
    let placed = vec![
        item(&atlas, HORN_OF_UNIRIA, 0),
        item(&atlas, HORN_OF_UNIRIA, 0),
        item(&atlas, HORN_OF_UNIRIA, 0),
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success {
            fee, created, zen, ..
        } => {
            assert_eq!(fee, Zen(250_000));
            assert_eq!(zen, Zen(BALANCE.0 - 250_000));
            assert_eq!(created.item, HORN_OF_DINORANT);
            assert_eq!(created.level, ItemLevel::ZERO);
            assert_eq!(created.skill, SkillRoll::WithSkill, "Fire Breath, no draw");
            assert_eq!(created.durability.max(), 255);
            assert_eq!(created.durability.current(), 255);
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes rate 70")
        }
    }
    // A worn horn fails the attempt before any fee.
    let mut worn = item(&atlas, HORN_OF_UNIRIA, 0);
    worn.durability = or_abort(Durability::new(200, 255));
    let placed = vec![
        item(&atlas, HORN_OF_UNIRIA, 0),
        item(&atlas, HORN_OF_UNIRIA, 0),
        worn,
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Rejected { reason, items } => {
            assert_eq!(reason, RejectReason::NoRecipeMatch);
            assert_eq!(items.len(), 4);
        }
        MixOutcome::Failed { .. } | MixOutcome::Success { .. } => {
            panic!("a worn horn forms no recipe")
        }
    }
}

#[test]
fn a_failed_dinorant_mix_destroys_horns_and_jewel() {
    let atlas = real_atlas();
    let placed = vec![
        item(&atlas, HORN_OF_UNIRIA, 0),
        item(&atlas, HORN_OF_UNIRIA, 0),
        item(&atlas, HORN_OF_UNIRIA, 0),
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_FAILURE) {
        MixOutcome::Failed {
            fee, casualties, ..
        } => {
            assert_eq!(fee, Zen(250_000));
            assert_eq!(
                destroyed_refs(&casualties),
                vec![
                    HORN_OF_UNIRIA,
                    HORN_OF_UNIRIA,
                    HORN_OF_UNIRIA,
                    JEWEL_OF_CHAOS
                ]
            );
            assert_eq!(casualties.len(), 4);
        }
        MixOutcome::Rejected { .. } | MixOutcome::Success { .. } => {
            panic!("seed {SEED_FAILURE} fails rate 70")
        }
    }
}

#[test]
fn dinorant_options_across_seeds_are_zero_one_or_two_distinct() {
    let atlas = real_atlas();
    let mut bare = 0u32;
    let mut single = 0u32;
    let mut pair = 0u32;
    for seed in 0..200 {
        let placed = vec![
            item(&atlas, HORN_OF_UNIRIA, 0),
            item(&atlas, HORN_OF_UNIRIA, 0),
            item(&atlas, HORN_OF_UNIRIA, 0),
            item(&atlas, JEWEL_OF_CHAOS, 0),
        ];
        if let MixOutcome::Success { created, .. } = run(&atlas, placed, BALANCE, seed) {
            match created.augment {
                CraftedAugment::None => bare += 1,
                // Distinctness is structural: the option set is a
                // one-bit-per-slot mask, so a duplicate pair (K2) collapses.
                CraftedAugment::Dinorant { options } => match options.count() {
                    1 => single += 1,
                    2 => pair += 1,
                    count => panic!("a dinorant carries at most two options, got {count}"),
                },
                CraftedAugment::WingBonus { .. } => panic!("a dinorant never rolls a wing bonus"),
            }
        }
    }
    assert!(bare > 0 && single > 0 && pair > 0, "{bare}/{single}/{pair}");
}

#[test]
fn fruit_mix_rolls_a_weighted_level_and_an_exact_balance_lands_on_zero() {
    let atlas = real_atlas();
    let placed = vec![
        item(&atlas, JEWEL_OF_CREATION, 0),
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    // Exact balance: the 3,000,000 fee lands the balance at zero.
    match run(&atlas, placed, Zen(3_000_000), SEED_SUCCESS) {
        MixOutcome::Success {
            fee, zen, created, ..
        } => {
            assert_eq!(fee, Zen(3_000_000));
            assert_eq!(zen, Zen(0));
            assert_eq!(created.item, STAT_FRUIT);
            assert!(created.level.get() <= 4);
            assert_eq!(created.durability.current(), 1);
            assert_eq!(created.durability.max(), 1);
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes rate 90")
        }
    }
    let placed = vec![
        item(&atlas, JEWEL_OF_CREATION, 0),
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_FAILURE) {
        MixOutcome::Failed { casualties, .. } => {
            assert_eq!(
                destroyed_refs(&casualties),
                vec![JEWEL_OF_CREATION, JEWEL_OF_CHAOS]
            );
            assert_eq!(casualties.len(), 2);
        }
        MixOutcome::Rejected { .. } | MixOutcome::Success { .. } => {
            panic!("seed {SEED_FAILURE} fails rate 90")
        }
    }
}

#[test]
fn devil_square_ticket_uses_the_shared_level_and_its_fee_row() {
    let atlas = real_atlas();
    let placed = vec![
        item(&atlas, DEVILS_EYE, 3),
        item(&atlas, DEVILS_KEY, 3),
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success { fee, created, .. } => {
            assert_eq!(fee, Zen(400_000), "the level-3 fee row");
            assert_eq!(created.item, DEVILS_INVITATION);
            assert_eq!(created.level.get(), 3);
            assert_eq!(created.durability.current(), 1);
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes rate 80")
        }
    }
    // Unequal levels fail the attempt: nothing matches, nothing is consumed.
    let unequal = vec![
        item(&atlas, DEVILS_EYE, 2),
        item(&atlas, DEVILS_KEY, 3),
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    match run(&atlas, unequal, BALANCE, SEED_SUCCESS) {
        MixOutcome::Rejected { reason, items } => {
            assert_eq!(reason, RejectReason::NoRecipeMatch);
            assert_eq!(items.len(), 3);
        }
        MixOutcome::Failed { .. } | MixOutcome::Success { .. } => {
            panic!("unequal ticket levels form no recipe")
        }
    }
    // A failed ticket mix destroys all three ingredients.
    let placed = vec![
        item(&atlas, DEVILS_EYE, 3),
        item(&atlas, DEVILS_KEY, 3),
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_FAILURE) {
        MixOutcome::Failed {
            fee, casualties, ..
        } => {
            assert_eq!(fee, Zen(400_000));
            assert_eq!(
                destroyed_refs(&casualties),
                vec![DEVILS_EYE, DEVILS_KEY, JEWEL_OF_CHAOS]
            );
        }
        MixOutcome::Rejected { .. } | MixOutcome::Success { .. } => {
            panic!("seed {SEED_FAILURE} fails rate 80")
        }
    }
}

#[test]
fn a_level_zero_ticket_pair_uses_the_level_one_row_k1() {
    let atlas = real_atlas();
    let placed = vec![
        item(&atlas, DEVILS_EYE, 0),
        item(&atlas, DEVILS_KEY, 0),
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success { fee, created, .. } => {
            assert_eq!(fee, Zen(100_000), "K1: the level-1 fee row");
            assert_eq!(created.level, ItemLevel::ZERO);
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes rate 80")
        }
    }
}

#[test]
fn blood_castle_ticket_mixes_at_the_shared_level() {
    let atlas = real_atlas();
    let placed = vec![
        item(&atlas, ARCHANGEL_SCROLL, 5),
        item(&atlas, BLOOD_BONE, 5),
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success { fee, created, .. } => {
            assert_eq!(fee, Zen(400_000), "the level-5 fee row");
            assert_eq!(created.item, INVISIBILITY_CLOAK);
            assert_eq!(created.level.get(), 5);
            assert_eq!(created.durability.current(), 1);
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes rate 80")
        }
    }
    let placed = vec![
        item(&atlas, ARCHANGEL_SCROLL, 5),
        item(&atlas, BLOOD_BONE, 5),
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_FAILURE) {
        MixOutcome::Failed { casualties, .. } => {
            assert_eq!(
                destroyed_refs(&casualties),
                vec![ARCHANGEL_SCROLL, BLOOD_BONE, JEWEL_OF_CHAOS]
            );
        }
        MixOutcome::Rejected { .. } | MixOutcome::Success { .. } => {
            panic!("seed {SEED_FAILURE} fails rate 80")
        }
    }
}

// --- Recipe inference precedence and the claim-all fall-throughs (§4.1). ---

#[test]
fn one_option_chaos_weapon_resolves_to_first_wings_not_a_sacrifice() {
    let atlas = real_atlas();
    let axe = with_option(item(&atlas, CHAOS_AXE, 6), NormalOption::PhysicalDamage);
    let placed = vec![axe, item(&atlas, JEWEL_OF_CHAOS, 0)];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success { created, .. } => {
            assert!(
                FIRST_WINGS.contains(&created.item),
                "First Wings (11) outranks Chaos Weapon (1)"
            );
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes rate 17")
        }
    }
}

#[test]
fn two_option_chaos_weapons_fall_through_to_a_chaos_weapon_sacrifice() {
    let atlas = real_atlas();
    let axe = with_option(item(&atlas, CHAOS_AXE, 6), NormalOption::PhysicalDamage);
    let placed = vec![axe.clone(), axe, item(&atlas, JEWEL_OF_CHAOS, 0)];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success { created, .. } => {
            assert!(
                CHAOS_WEAPONS.contains(&created.item),
                "the exactly-one claim fails First Wings; both weapons sacrifice"
            );
            assert!(created.level.get() <= 4, "a freshly rolled chaos weapon");
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes rate 32")
        }
    }
}

#[test]
fn an_upgrade_window_with_a_leftover_falls_through_to_chaos_weapon() {
    let atlas = real_atlas();
    // The +10 claim leaves the extra option-gloves unclaimed → attempt fails →
    // Chaos Weapon (1) sacrifices both option items and burns the boosters.
    let helm = with_option(item(&atlas, HELM, 9), NormalOption::Defense);
    let gloves = with_option(item(&atlas, GLOVES, 6), NormalOption::Defense);
    let placed = vec![
        helm,
        gloves,
        item(&atlas, JEWEL_OF_CHAOS, 0),
        item(&atlas, JEWEL_OF_BLESS, 0),
        item(&atlas, JEWEL_OF_SOUL, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success { created, .. } => {
            assert!(CHAOS_WEAPONS.contains(&created.item));
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes rate 29")
        }
    }
}

#[test]
fn a_ticket_window_returns_unclaimed_junk_untouched() {
    let atlas = real_atlas();
    let junk = item(&atlas, JEWEL_OF_BLESS, 0);
    let placed = vec![
        item(&atlas, DEVILS_EYE, 2),
        item(&atlas, DEVILS_KEY, 2),
        item(&atlas, JEWEL_OF_CHAOS, 0),
        junk.clone(),
    ];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success {
            created, returned, ..
        } => {
            assert_eq!(created.item, DEVILS_INVITATION);
            assert_eq!(created.level.get(), 2);
            assert_eq!(returned, vec![junk], "exactly the untouched extra");
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes rate 80")
        }
    }
}

#[test]
fn a_lone_jewel_forms_no_recipe_and_charges_nothing() {
    let atlas = real_atlas();
    let jewel = item(&atlas, JEWEL_OF_CHAOS, 0);
    match run(&atlas, vec![jewel.clone()], Zen(0), 7) {
        MixOutcome::Rejected { reason, items } => {
            assert_eq!(reason, RejectReason::NoRecipeMatch);
            assert_eq!(items, vec![jewel]);
        }
        MixOutcome::Failed { .. } | MixOutcome::Success { .. } => {
            panic!("a lone jewel forms no recipe")
        }
    }
}

// --- Casualty accounting: move-only, no silent loss. ---

#[test]
fn every_input_appears_exactly_once_across_a_failed_chaos_weapon_outcome() {
    let atlas = real_atlas();
    // Rate (2·26,100 + 2·40,000) / 20,000 = 6 — seed 8 fails it.
    let placed = vec![
        option_sword(&atlas),
        option_sword(&atlas),
        item(&atlas, JEWEL_OF_CHAOS, 0),
        item(&atlas, JEWEL_OF_CHAOS, 0),
    ];
    match run(&atlas, placed, BALANCE, SEED_FAILURE) {
        MixOutcome::Failed {
            fee, casualties, ..
        } => {
            assert_eq!(fee, Zen(60_000));
            assert_eq!(casualties.len(), 4, "each input exactly once");
            let downgraded: Vec<ItemRef> = casualties
                .iter()
                .filter_map(|casualty| match casualty {
                    Casualty::Downgraded { item } => Some(item.item),
                    Casualty::Destroyed { .. } | Casualty::Returned { .. } => None,
                })
                .collect();
            assert_eq!(downgraded, vec![SWORD, SWORD]);
            assert_eq!(
                destroyed_refs(&casualties),
                vec![JEWEL_OF_CHAOS, JEWEL_OF_CHAOS]
            );
        }
        MixOutcome::Rejected { .. } | MixOutcome::Success { .. } => {
            panic!("seed {SEED_FAILURE} fails rate 6")
        }
    }
}

#[test]
fn a_ticket_success_accounts_created_returned_and_consumed_exactly_once() {
    let atlas = real_atlas();
    let extra = item(&atlas, JEWEL_OF_SOUL, 0);
    let placed = vec![
        item(&atlas, DEVILS_EYE, 2),
        item(&atlas, DEVILS_KEY, 2),
        item(&atlas, JEWEL_OF_CHAOS, 0),
        extra.clone(),
    ];
    match run(&atlas, placed, BALANCE, SEED_SUCCESS) {
        MixOutcome::Success {
            created, returned, ..
        } => {
            assert_eq!(created.item, DEVILS_INVITATION);
            assert_eq!(returned, vec![extra], "the extra returns exactly once");
            // The eye, key, and consumed jewel appear in neither list.
            assert!(returned.iter().all(|item| item.item != DEVILS_EYE));
            assert!(returned.iter().all(|item| item.item != DEVILS_KEY));
            assert!(returned.iter().all(|item| item.item != JEWEL_OF_CHAOS));
        }
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes rate 80")
        }
    }
}

#[test]
fn no_outcome_over_any_seed_drops_an_input_silently() {
    let atlas = real_atlas();
    for seed in 0..40 {
        // A chaos-weapon window: 2 inputs.
        let placed = vec![option_sword(&atlas), item(&atlas, JEWEL_OF_CHAOS, 0)];
        match run(&atlas, placed, BALANCE, seed) {
            MixOutcome::Rejected { items, .. } => assert_eq!(items.len(), 2),
            MixOutcome::Failed { casualties, .. } => assert_eq!(casualties.len(), 2),
            MixOutcome::Success { returned, .. } => assert!(returned.is_empty()),
        }
        // A ticket window with an extra: 4 inputs, 3 consumed on success.
        let placed = vec![
            item(&atlas, DEVILS_EYE, 1),
            item(&atlas, DEVILS_KEY, 1),
            item(&atlas, JEWEL_OF_CHAOS, 0),
            item(&atlas, JEWEL_OF_SOUL, 0),
        ];
        match run(&atlas, placed, BALANCE, seed) {
            MixOutcome::Rejected { items, .. } => assert_eq!(items.len(), 4),
            MixOutcome::Failed { casualties, .. } => assert_eq!(casualties.len(), 4),
            MixOutcome::Success { returned, .. } => assert_eq!(returned.len(), 1),
        }
    }
}

// --- Zen arithmetic. ---

#[test]
fn an_unaffordable_fee_rejects_with_nothing_consumed() {
    let atlas = real_atlas();
    // The +11 fee is 4,000,000; one zen short rejects before any draw.
    let placed = vec![
        item(&atlas, HELM, 10),
        item(&atlas, JEWEL_OF_CHAOS, 0),
        item(&atlas, JEWEL_OF_BLESS, 0),
        item(&atlas, JEWEL_OF_BLESS, 0),
        item(&atlas, JEWEL_OF_SOUL, 0),
        item(&atlas, JEWEL_OF_SOUL, 0),
    ];
    match run(&atlas, placed, Zen(3_999_999), SEED_SUCCESS) {
        MixOutcome::Rejected { reason, items } => {
            assert_eq!(reason, RejectReason::InsufficientZen);
            assert_eq!(items.len(), 6, "every input handed back");
        }
        MixOutcome::Failed { .. } | MixOutcome::Success { .. } => {
            panic!("3,999,999 zen cannot pay a 4,000,000 fee")
        }
    }
}

#[test]
fn success_and_failure_both_charge_the_exact_fee() {
    let atlas = real_atlas();
    let window = |atlas: &Atlas| {
        vec![
            item(atlas, HORN_OF_UNIRIA, 0),
            item(atlas, HORN_OF_UNIRIA, 0),
            item(atlas, HORN_OF_UNIRIA, 0),
            item(atlas, JEWEL_OF_CHAOS, 0),
        ]
    };
    match run(&atlas, window(&atlas), Zen(1_000_000), SEED_SUCCESS) {
        MixOutcome::Success { zen, .. } => assert_eq!(zen, Zen(750_000)),
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed {SEED_SUCCESS} passes rate 70")
        }
    }
    match run(&atlas, window(&atlas), Zen(1_000_000), SEED_FAILURE) {
        MixOutcome::Failed { zen, .. } => {
            assert_eq!(zen, Zen(750_000), "the fee is never refunded");
        }
        MixOutcome::Rejected { .. } | MixOutcome::Success { .. } => {
            panic!("seed {SEED_FAILURE} fails rate 70")
        }
    }
}

// --- Determinism: bit-equality under a fixed seed. ---

#[test]
fn the_same_window_zen_and_seed_produce_a_bit_identical_outcome() {
    let atlas = real_atlas();
    for seed in [0u64, 8, 24, 42, 100] {
        let window = |atlas: &Atlas| {
            vec![
                option_sword(atlas),
                item(atlas, JEWEL_OF_CHAOS, 0),
                item(atlas, JEWEL_OF_BLESS, 0),
            ]
        };
        let first = run(&atlas, window(&atlas), BALANCE, seed);
        let second = run(&atlas, window(&atlas), BALANCE, seed);
        assert_eq!(first, second, "seed {seed} must replay bit-for-bit");

        let horns = |atlas: &Atlas| {
            vec![
                item(atlas, HORN_OF_UNIRIA, 0),
                item(atlas, HORN_OF_UNIRIA, 0),
                item(atlas, HORN_OF_UNIRIA, 0),
                item(atlas, JEWEL_OF_CHAOS, 0),
            ]
        };
        assert_eq!(
            run(&atlas, horns(&atlas), BALANCE, seed),
            run(&atlas, horns(&atlas), BALANCE, seed),
            "dinorant augments replay bit-for-bit under seed {seed}"
        );
    }
}

// --- Price routing: one priced instance per ItemKind row, exact values. ---

#[test]
fn every_item_kind_row_prices_at_its_pinned_value() {
    let atlas = real_atlas();
    // One priced instance per `ItemKind` row: (group, number, level, zen).
    // Weapon (width-1 discount) / bow / crossbow / staff / shield (discounted)
    // / helm / body armor / pants / gloves / boots / arrows / bolts on the
    // general branch; group-12 wings on the wing branch (the pinned 55.4M
    // anchor) with the cape routed cubic; ring / pendant / transformation ring
    // and the Uniria on the cubic branch; the Dinorant on its 960k special;
    // orb / skill scroll / jewel / stat fruit fixed-verbatim; the apple
    // consumable base 20 → tens 20 → ×3 pieces; the lucky box on the general
    // branch; event ticket and mix materials on their clamped per-level rows.
    let pinned: [(u8, u16, u8, u64); 27] = [
        (0, 3, 0, 1_500),
        (4, 6, 0, 80_900),
        (4, 8, 0, 180),
        (5, 7, 0, 80_900),
        (6, 0, 0, 110),
        (7, 2, 0, 240),
        (8, 0, 0, 2_400),
        (9, 0, 0, 1_600),
        (10, 0, 0, 1_200),
        (11, 0, 0, 1_000),
        (4, 15, 0, 100),
        (4, 7, 0, 100),
        (12, 0, 0, 55_400_000),
        (13, 30, 0, 5_832_100),
        (13, 9, 0, 5_000),
        (13, 12, 0, 9_300),
        (13, 10, 0, 100),
        (13, 2, 0, 15_700),
        (13, 3, 0, 960_000),
        (12, 7, 0, 29_000),
        (15, 0, 0, 17_000),
        (12, 15, 0, 810_000),
        (13, 15, 0, 33_000_000),
        (14, 0, 0, 60),
        (14, 11, 0, 100),
        (14, 19, 0, 60_000),
        (14, 17, 0, 10_000),
    ];
    for (group, number, level, expected) in pinned {
        let id = ItemRef { group, number };
        let def = atlas.item(id).unwrap();
        assert_eq!(
            buying_price(def, &item(&atlas, id, level)),
            Zen(expected),
            "price of {group}/{number} at +{level}"
        );
    }
    // The per-level table clamps to its last row past the leading run.
    let feather_def = atlas.item(LOCHS_FEATHER).unwrap();
    assert_eq!(
        buying_price(feather_def, &item(&atlas, LOCHS_FEATHER, 1)),
        Zen(7_500_000),
        "the Monarch's Crest row"
    );
}

#[test]
fn old_buying_price_overlays_only_the_jewels() {
    let atlas = real_atlas();
    let old = |id: ItemRef| {
        let def = atlas.item(id).unwrap();
        old_buying_price(def, &item(&atlas, id, 0)).0
    };
    assert_eq!(old(JEWEL_OF_BLESS), 100_000);
    assert_eq!(old(JEWEL_OF_SOUL), 70_000);
    assert_eq!(old(JEWEL_OF_CHAOS), 40_000);
    assert_eq!(
        old(ItemRef {
            group: 14,
            number: 16
        }),
        450_000
    );
    assert_eq!(old(JEWEL_OF_CREATION), 450_000);
    // A non-jewel falls through to the current price.
    let sword_def = atlas.item(SWORD).unwrap();
    let sword = option_sword(&atlas);
    assert_eq!(
        old_buying_price(sword_def, &sword),
        buying_price(sword_def, &sword)
    );
    assert_eq!(buying_price(sword_def, &sword), Zen(26_100));
}
