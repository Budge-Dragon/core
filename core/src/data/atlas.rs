//! Cross-checked, resolved view over the entire static dataset, built once at
//! load. `Atlas::parse` is the single referential-integrity proof for every v2
//! file: per-file identity uniqueness plus resolution of every declared
//! cross-file reference, in one pass. Every accessor downstream is total or
//! genuinely optional.

use std::collections::{HashMap, HashSet};

use crate::components::geometry::{Direction, Point, Rect};
use crate::components::levels::TransformationLevel;

use super::ancient_sets::AncientSet;
use super::box_drops::BoxDrop;
use super::chaos_mixes::{ChaosMix, ChaosRecipe};
use super::classes::ClassRecord;
use super::common::{DataFile, GateNumber, ItemRef, MapNumber, MonsterNumber, SkillNumber};
use super::exp_tables::ExpTable;
use super::game_config::GameConfig;
use super::gates_warps::{EnterGate, GateWarpRecord, SpawnGate, TargetGate, Warp, WarpIndex};
use super::item_definitions::{ItemDefinition, ItemKind};
use super::map_definitions::MapDefinition;
use super::monster_definitions::{MonsterAttack, MonsterDefinition, MonsterRole};
use super::skills::{Skill, SkillShape};
use super::spawns::Spawn;
use super::special_drops::{SpecialDrop, SpecialDropRecord};

/// Respawn fallback map (Lorencia); the Atlas proves it carries a spawn gate.
const FALLBACK_MAP: MapNumber = MapNumber(0);

/// Every v2 data file, parsed from JSON but not yet cross-checked. One field
/// per file; the host fills it once and hands it to [`Atlas::parse`].
pub struct StaticData {
    /// `map_definitions.json`.
    pub maps: DataFile<MapDefinition>,
    /// `gates_warps.json`.
    pub gates_warps: DataFile<GateWarpRecord>,
    /// `monster_definitions.json`.
    pub monsters: DataFile<MonsterDefinition>,
    /// `spawns.json`.
    pub spawns: DataFile<Spawn>,
    /// `skills.json`.
    pub skills: DataFile<Skill>,
    /// `item_definitions.json`.
    pub items: DataFile<ItemDefinition>,
    /// `box_drops.json`.
    pub box_drops: DataFile<BoxDrop>,
    /// `special_drops.json`.
    pub special_drops: DataFile<SpecialDropRecord>,
    /// `ancient_sets.json`.
    pub ancient_sets: DataFile<AncientSet>,
    /// `chaos_mixes.json`.
    pub chaos_mixes: DataFile<ChaosMix>,
    /// `classes.json`.
    pub classes: DataFile<ClassRecord>,
    /// `exp_tables.json`.
    pub exp_tables: DataFile<ExpTable>,
    /// `game_config.json`.
    pub game_config: DataFile<GameConfig>,
}

/// The landing side of a resolved gate reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Landing {
    /// Map the traveler lands on.
    pub map: MapNumber,
    /// Landing rectangle.
    pub area: Rect,
    /// Facing on arrival; absent = unspecified.
    pub facing: Option<Direction>,
}

/// An enter gate with its landing resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnterGateView<'a> {
    /// The trigger-side record.
    pub gate: &'a EnterGate,
    /// Where its target gate lands travelers.
    pub landing: Landing,
}

/// A warp entry with its landing resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarpView<'a> {
    /// The warp-list record.
    pub warp: &'a Warp,
    /// Where its target gate lands travelers.
    pub landing: Landing,
}

/// The static dataset with every cross-file reference resolved. Construction
/// proves, dataset-wide, per-file identity uniqueness and resolution of every
/// declared cross-file edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Atlas {
    maps: Vec<MapDefinition>,
    spawn_gates_by_map: HashMap<MapNumber, Vec<SpawnGate>>,
    enter_gates_by_map: HashMap<MapNumber, Vec<(EnterGate, Landing)>>,
    warps: Vec<(Warp, Landing)>,
    fallback: SpawnGate,
}

impl Atlas {
    /// Builds the atlas from the whole dataset, proving referential integrity
    /// of every file in one pass.
    ///
    /// # Errors
    /// Returns the first [`AtlasError`] found: a duplicated per-file identity
    /// or a cross-file reference that resolves to nothing (or to a wrong-kind
    /// gate).
    pub fn parse(data: StaticData) -> Result<Self, AtlasError> {
        let map_numbers = unique_map_numbers(&data.maps.records)?;
        let monster_numbers = unique_monster_numbers(&data.monsters.records)?;
        let skill_numbers = unique_skill_numbers(&data.skills.records)?;
        let item_refs = unique_item_refs(&data.items.records)?;

        let gates = GatePartition::partition(data.gates_warps.records)?;
        gates.check_maps(&map_numbers)?;

        let landings = gates.landings();
        let warps = resolve_warps(gates.warps, &landings, &gates.enter_gate_numbers)?;
        let enter_gates_by_map =
            resolve_enter_gates(gates.enter_gates, &landings, &gates.enter_gate_numbers)?;

        check_spawns(&data.spawns.records, &map_numbers, &monster_numbers)?;
        check_monster_attacks(&data.monsters.records, &skill_numbers)?;
        check_summons(&data.skills.records, &monster_numbers)?;
        check_items(&data.items.records, &skill_numbers, &monster_numbers)?;
        check_ancient_sets(&data.ancient_sets.records, &item_refs)?;
        check_chaos_mixes(&data.chaos_mixes.records, &item_refs)?;
        check_special_drops(
            &data.special_drops.records,
            &item_refs,
            &monster_numbers,
            &map_numbers,
        )?;
        check_box_drops(&data.box_drops.records, &item_refs)?;
        check_classes(&data.classes.records, &map_numbers)?;

        let fallback = gates
            .spawn_gates
            .iter()
            .find(|gate| gate.map == FALLBACK_MAP)
            .cloned()
            .ok_or(AtlasError::FallbackSpawnGateMissing)?;

        let mut spawn_gates_by_map: HashMap<MapNumber, Vec<SpawnGate>> = HashMap::new();
        for gate in gates.spawn_gates {
            spawn_gates_by_map.entry(gate.map).or_default().push(gate);
        }

        Ok(Self {
            maps: data.maps.records,
            spawn_gates_by_map,
            enter_gates_by_map,
            warps,
            fallback,
        })
    }

    /// All maps, in load order.
    pub fn maps(&self) -> impl Iterator<Item = &MapDefinition> {
        self.maps.iter()
    }

    /// Spawn gates on a map; empty for maps without one.
    #[must_use]
    pub fn spawn_gates(&self, map: MapNumber) -> &[SpawnGate] {
        match self.spawn_gates_by_map.get(&map) {
            Some(gates) => gates,
            None => &[],
        }
    }

    /// The fallback spawn gate on Lorencia — presence proven by `parse`.
    #[must_use]
    pub fn fallback_spawn_gate(&self) -> &SpawnGate {
        &self.fallback
    }

    /// The enter gate covering a tile, if any. The landing was resolved at
    /// parse, so the only optionality is whether a gate covers the tile.
    #[must_use]
    pub fn enter_gate_at(&self, map: MapNumber, tile: Point) -> Option<EnterGateView<'_>> {
        let gates = self.enter_gates_by_map.get(&map)?;
        let (gate, landing) = gates.iter().find(|(gate, _)| gate.area.contains(tile))?;
        Some(EnterGateView {
            gate,
            landing: *landing,
        })
    }

    /// The warp list in index order, each with its landing resolved at parse.
    pub fn warps(&self) -> impl Iterator<Item = WarpView<'_>> {
        self.warps.iter().map(|(warp, landing)| WarpView {
            warp,
            landing: *landing,
        })
    }
}

/// The gate records partitioned by kind, with per-file identity sets proven
/// unique.
struct GatePartition {
    spawn_gates: Vec<SpawnGate>,
    enter_gates: Vec<EnterGate>,
    target_gates: Vec<TargetGate>,
    warps: Vec<Warp>,
    enter_gate_numbers: HashSet<GateNumber>,
}

impl GatePartition {
    fn partition(records: Vec<GateWarpRecord>) -> Result<Self, AtlasError> {
        let mut spawn_gates = Vec::new();
        let mut enter_gates = Vec::new();
        let mut target_gates = Vec::new();
        let mut warps = Vec::new();
        let mut gate_numbers = HashSet::new();
        let mut warp_indices = HashSet::new();
        let mut enter_gate_numbers = HashSet::new();

        for record in records {
            match record {
                GateWarpRecord::SpawnGate(gate) => {
                    claim_gate(&mut gate_numbers, gate.number)?;
                    spawn_gates.push(gate);
                }
                GateWarpRecord::TargetGate(gate) => {
                    claim_gate(&mut gate_numbers, gate.number)?;
                    target_gates.push(gate);
                }
                GateWarpRecord::EnterGate(gate) => {
                    claim_gate(&mut gate_numbers, gate.number)?;
                    enter_gate_numbers.insert(gate.number);
                    enter_gates.push(gate);
                }
                GateWarpRecord::Warp(warp) => {
                    if !warp_indices.insert(warp.index) {
                        return Err(AtlasError::DuplicateWarpIndex { index: warp.index });
                    }
                    warps.push(warp);
                }
            }
        }

        Ok(Self {
            spawn_gates,
            enter_gates,
            target_gates,
            warps,
            enter_gate_numbers,
        })
    }

    fn check_maps(&self, map_numbers: &HashSet<MapNumber>) -> Result<(), AtlasError> {
        for gate in &self.spawn_gates {
            require_map(map_numbers, gate.number, gate.map)?;
        }
        for gate in &self.target_gates {
            require_map(map_numbers, gate.number, gate.map)?;
        }
        for gate in &self.enter_gates {
            require_map(map_numbers, gate.number, gate.map)?;
        }
        Ok(())
    }

    fn landings(&self) -> HashMap<GateNumber, Landing> {
        let mut landings = HashMap::new();
        for gate in &self.spawn_gates {
            landings.insert(
                gate.number,
                Landing {
                    map: gate.map,
                    area: gate.area,
                    facing: gate.direction,
                },
            );
        }
        for gate in &self.target_gates {
            landings.insert(
                gate.number,
                Landing {
                    map: gate.map,
                    area: gate.area,
                    facing: gate.direction,
                },
            );
        }
        landings
    }
}

/// Joins each warp to its landing, proving referential integrity in the same
/// pass. Every returned pair carries a resolved [`Landing`]; a target that
/// resolves to nothing (or to a wrong-kind enter gate) is the error.
fn resolve_warps(
    warps: Vec<Warp>,
    landings: &HashMap<GateNumber, Landing>,
    enter_gate_numbers: &HashSet<GateNumber>,
) -> Result<Vec<(Warp, Landing)>, AtlasError> {
    let mut resolved = Vec::with_capacity(warps.len());
    for warp in warps {
        let target = warp.target_gate;
        let landing = match landings.get(&target) {
            Some(&landing) => landing,
            None if enter_gate_numbers.contains(&target) => {
                return Err(AtlasError::WarpTargetsEnterGate {
                    warp: warp.index,
                    target,
                });
            }
            None => {
                return Err(AtlasError::WarpTargetsUnknownGate {
                    warp: warp.index,
                    target,
                });
            }
        };
        resolved.push((warp, landing));
    }
    Ok(resolved)
}

/// Joins each enter gate to its landing and groups the pairs by map, proving
/// referential integrity in the same pass. A target that resolves to nothing
/// (or to a wrong-kind enter gate) is the error.
fn resolve_enter_gates(
    enter_gates: Vec<EnterGate>,
    landings: &HashMap<GateNumber, Landing>,
    enter_gate_numbers: &HashSet<GateNumber>,
) -> Result<HashMap<MapNumber, Vec<(EnterGate, Landing)>>, AtlasError> {
    let mut by_map: HashMap<MapNumber, Vec<(EnterGate, Landing)>> = HashMap::new();
    for gate in enter_gates {
        let target = gate.target_gate;
        let landing = match landings.get(&target) {
            Some(&landing) => landing,
            None if enter_gate_numbers.contains(&target) => {
                return Err(AtlasError::EnterTargetsEnterGate {
                    gate: gate.number,
                    target,
                });
            }
            None => {
                return Err(AtlasError::EnterTargetsUnknownGate {
                    gate: gate.number,
                    target,
                });
            }
        };
        by_map.entry(gate.map).or_default().push((gate, landing));
    }
    Ok(by_map)
}

fn claim_gate(seen: &mut HashSet<GateNumber>, number: GateNumber) -> Result<(), AtlasError> {
    if seen.insert(number) {
        Ok(())
    } else {
        Err(AtlasError::DuplicateGateNumber { number })
    }
}

fn require_map(
    map_numbers: &HashSet<MapNumber>,
    gate: GateNumber,
    map: MapNumber,
) -> Result<(), AtlasError> {
    if map_numbers.contains(&map) {
        Ok(())
    } else {
        Err(AtlasError::GateOnUnknownMap { gate, map })
    }
}

fn unique_map_numbers(maps: &[MapDefinition]) -> Result<HashSet<MapNumber>, AtlasError> {
    let mut set = HashSet::new();
    for map in maps {
        if !set.insert(map.number) {
            return Err(AtlasError::DuplicateMapNumber { number: map.number });
        }
    }
    Ok(set)
}

fn unique_monster_numbers(
    monsters: &[MonsterDefinition],
) -> Result<HashSet<MonsterNumber>, AtlasError> {
    let mut set = HashSet::new();
    for monster in monsters {
        if !set.insert(monster.number) {
            return Err(AtlasError::DuplicateMonsterNumber {
                number: monster.number,
            });
        }
    }
    Ok(set)
}

fn unique_skill_numbers(skills: &[Skill]) -> Result<HashSet<SkillNumber>, AtlasError> {
    let mut set = HashSet::new();
    for skill in skills {
        if !set.insert(skill.number) {
            return Err(AtlasError::DuplicateSkillNumber {
                number: skill.number,
            });
        }
    }
    Ok(set)
}

fn unique_item_refs(items: &[ItemDefinition]) -> Result<HashSet<ItemRef>, AtlasError> {
    let mut set = HashSet::new();
    for item in items {
        if !set.insert(item.id) {
            return Err(AtlasError::DuplicateItemRef { item: item.id });
        }
    }
    Ok(set)
}

fn check_spawns(
    spawns: &[Spawn],
    map_numbers: &HashSet<MapNumber>,
    monster_numbers: &HashSet<MonsterNumber>,
) -> Result<(), AtlasError> {
    for spawn in spawns {
        if !map_numbers.contains(&spawn.map) {
            return Err(AtlasError::UnknownMapRef { map: spawn.map });
        }
        if !monster_numbers.contains(&spawn.monster) {
            return Err(AtlasError::UnknownMonsterRef {
                monster: spawn.monster,
            });
        }
    }
    Ok(())
}

fn check_monster_attacks(
    monsters: &[MonsterDefinition],
    skill_numbers: &HashSet<SkillNumber>,
) -> Result<(), AtlasError> {
    for monster in monsters {
        let attack = match &monster.role {
            MonsterRole::Monster { attack, .. } | MonsterRole::Trap { attack, .. } => attack,
            MonsterRole::Guard { .. } | MonsterRole::Npc { .. } | MonsterRole::SoccerBall => {
                continue;
            }
        };
        if let MonsterAttack::Skill { skill } = attack {
            require_skill(skill_numbers, *skill)?;
        }
    }
    Ok(())
}

fn check_summons(
    skills: &[Skill],
    monster_numbers: &HashSet<MonsterNumber>,
) -> Result<(), AtlasError> {
    for skill in skills {
        if let SkillShape::Summon { monster } = skill.shape {
            require_monster(monster_numbers, monster)?;
        }
    }
    Ok(())
}

fn check_items(
    items: &[ItemDefinition],
    skill_numbers: &HashSet<SkillNumber>,
    monster_numbers: &HashSet<MonsterNumber>,
) -> Result<(), AtlasError> {
    for item in items {
        match &item.kind {
            ItemKind::Weapon { skill, .. }
            | ItemKind::Bow { skill, .. }
            | ItemKind::Crossbow { skill, .. }
            | ItemKind::Staff { skill, .. }
            | ItemKind::Shield { skill, .. }
            | ItemKind::Pet { skill, .. } => {
                if let Some(skill) = skill {
                    require_skill(skill_numbers, *skill)?;
                }
            }
            ItemKind::Orb { teaches, .. } | ItemKind::SkillScroll { teaches, .. } => {
                require_skill(skill_numbers, *teaches)?;
            }
            ItemKind::TransformationRing { skins, .. } => {
                for level in [
                    TransformationLevel::L0,
                    TransformationLevel::L1,
                    TransformationLevel::L2,
                    TransformationLevel::L3,
                    TransformationLevel::L4,
                    TransformationLevel::L5,
                ] {
                    require_monster(monster_numbers, skins.skin(level))?;
                }
            }
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
            | ItemKind::Jewel { .. }
            | ItemKind::Consumable { .. }
            | ItemKind::LuckyBox
            | ItemKind::EventTicket { .. }
            | ItemKind::MixMaterial
            | ItemKind::StatFruit => {}
        }
    }
    Ok(())
}

fn check_ancient_sets(sets: &[AncientSet], item_refs: &HashSet<ItemRef>) -> Result<(), AtlasError> {
    for set in sets {
        for piece in &set.pieces {
            require_item(item_refs, piece.item)?;
        }
    }
    Ok(())
}

fn check_chaos_mixes(mixes: &[ChaosMix], item_refs: &HashSet<ItemRef>) -> Result<(), AtlasError> {
    for mix in mixes {
        for item in recipe_item_refs(&mix.recipe) {
            require_item(item_refs, item)?;
        }
    }
    Ok(())
}

fn recipe_item_refs(recipe: &ChaosRecipe) -> Vec<ItemRef> {
    match recipe {
        ChaosRecipe::ChaosWeapon { weapons, .. } => weapons.to_vec(),
        ChaosRecipe::FirstWings {
            chaos_weapons,
            wings,
            ..
        } => chaos_weapons.iter().chain(wings.iter()).copied().collect(),
        ChaosRecipe::SecondWings {
            first_wings,
            feather,
            wings,
            ..
        } => first_wings
            .iter()
            .copied()
            .chain(core::iter::once(feather.item))
            .chain(wings.iter().copied())
            .collect(),
        ChaosRecipe::CapeOfLord {
            first_wings,
            crest,
            cape,
            ..
        } => first_wings
            .iter()
            .copied()
            .chain([crest.item, *cape])
            .collect(),
        ChaosRecipe::ItemUpgrade { .. } => Vec::new(),
        ChaosRecipe::Dinorant { horn, dinorant, .. } => vec![*horn, *dinorant],
        ChaosRecipe::Fruits {
            catalyst, fruit, ..
        } => vec![*catalyst, *fruit],
        ChaosRecipe::DevilSquareTicket {
            eye,
            key,
            invitation,
            ..
        } => vec![*eye, *key, *invitation],
        ChaosRecipe::BloodCastleTicket {
            scroll,
            bone,
            cloak,
            ..
        } => vec![*scroll, *bone, *cloak],
    }
}

fn check_special_drops(
    records: &[SpecialDropRecord],
    item_refs: &HashSet<ItemRef>,
    monster_numbers: &HashSet<MonsterNumber>,
    map_numbers: &HashSet<MapNumber>,
) -> Result<(), AtlasError> {
    for record in records {
        match &record.drop {
            SpecialDrop::LevelBanded { item, .. } => require_item(item_refs, *item)?,
            SpecialDrop::MonsterBound { monster, items, .. } => {
                require_monster(monster_numbers, *monster)?;
                for &item in items.iter() {
                    require_item(item_refs, item)?;
                }
            }
            SpecialDrop::MapBound { map, item, .. } => {
                if !map_numbers.contains(map) {
                    return Err(AtlasError::UnknownMapRef { map: *map });
                }
                require_item(item_refs, *item)?;
            }
        }
    }
    Ok(())
}

fn check_box_drops(records: &[BoxDrop], item_refs: &HashSet<ItemRef>) -> Result<(), AtlasError> {
    for record in records {
        require_item(item_refs, record.box_item)?;
        for &item in record.items.iter() {
            require_item(item_refs, item)?;
        }
    }
    Ok(())
}

fn check_classes(
    classes: &[ClassRecord],
    map_numbers: &HashSet<MapNumber>,
) -> Result<(), AtlasError> {
    for class in classes {
        if !map_numbers.contains(&class.home_map) {
            return Err(AtlasError::UnknownMapRef {
                map: class.home_map,
            });
        }
    }
    Ok(())
}

fn require_skill(skills: &HashSet<SkillNumber>, skill: SkillNumber) -> Result<(), AtlasError> {
    if skills.contains(&skill) {
        Ok(())
    } else {
        Err(AtlasError::UnknownSkillRef { skill })
    }
}

fn require_monster(
    monsters: &HashSet<MonsterNumber>,
    monster: MonsterNumber,
) -> Result<(), AtlasError> {
    if monsters.contains(&monster) {
        Ok(())
    } else {
        Err(AtlasError::UnknownMonsterRef { monster })
    }
}

fn require_item(items: &HashSet<ItemRef>, item: ItemRef) -> Result<(), AtlasError> {
    if items.contains(&item) {
        Ok(())
    } else {
        Err(AtlasError::UnknownItemRef { item })
    }
}

/// Why the dataset does not form a consistent world — one variant per proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtlasError {
    /// Two map records share a number.
    DuplicateMapNumber {
        /// The repeated map number.
        number: MapNumber,
    },
    /// Two gate records share a number.
    DuplicateGateNumber {
        /// The repeated gate number.
        number: GateNumber,
    },
    /// Two warp records share an index.
    DuplicateWarpIndex {
        /// The repeated warp index.
        index: WarpIndex,
    },
    /// Two monster records share a number.
    DuplicateMonsterNumber {
        /// The repeated monster number.
        number: MonsterNumber,
    },
    /// Two skill records share a number.
    DuplicateSkillNumber {
        /// The repeated skill number.
        number: SkillNumber,
    },
    /// Two item records share an identity.
    DuplicateItemRef {
        /// The repeated item identity.
        item: ItemRef,
    },
    /// A gate names a map with no record.
    GateOnUnknownMap {
        /// The gate carrying the dangling reference.
        gate: GateNumber,
        /// The unresolved map.
        map: MapNumber,
    },
    /// An enter gate's target resolves to nothing.
    EnterTargetsUnknownGate {
        /// The enter gate.
        gate: GateNumber,
        /// The unresolved target.
        target: GateNumber,
    },
    /// An enter gate targets another enter gate.
    EnterTargetsEnterGate {
        /// The enter gate.
        gate: GateNumber,
        /// The wrong-kind target.
        target: GateNumber,
    },
    /// A warp's target resolves to nothing.
    WarpTargetsUnknownGate {
        /// The warp.
        warp: WarpIndex,
        /// The unresolved target.
        target: GateNumber,
    },
    /// A warp targets an enter gate.
    WarpTargetsEnterGate {
        /// The warp.
        warp: WarpIndex,
        /// The wrong-kind target.
        target: GateNumber,
    },
    /// The respawn fallback map (Lorencia) has no spawn gate.
    FallbackSpawnGateMissing,
    /// A record references a map with no record.
    UnknownMapRef {
        /// The unresolved map.
        map: MapNumber,
    },
    /// A record references a monster with no record.
    UnknownMonsterRef {
        /// The unresolved monster.
        monster: MonsterNumber,
    },
    /// A record references a skill with no record.
    UnknownSkillRef {
        /// The unresolved skill.
        skill: SkillNumber,
    },
    /// A record references an item with no record.
    UnknownItemRef {
        /// The unresolved item.
        item: ItemRef,
    },
}

impl core::fmt::Display for AtlasError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DuplicateMapNumber { number } => write!(f, "duplicate map number {number:?}"),
            Self::DuplicateGateNumber { number } => write!(f, "duplicate gate number {number:?}"),
            Self::DuplicateWarpIndex { index } => write!(f, "duplicate warp index {index:?}"),
            Self::DuplicateMonsterNumber { number } => {
                write!(f, "duplicate monster number {number:?}")
            }
            Self::DuplicateSkillNumber { number } => write!(f, "duplicate skill number {number:?}"),
            Self::DuplicateItemRef { item } => write!(f, "duplicate item {item:?}"),
            Self::GateOnUnknownMap { gate, map } => {
                write!(f, "gate {gate:?} sits on unknown map {map:?}")
            }
            Self::EnterTargetsUnknownGate { gate, target } => {
                write!(f, "enter gate {gate:?} targets unknown gate {target:?}")
            }
            Self::EnterTargetsEnterGate { gate, target } => {
                write!(f, "enter gate {gate:?} targets enter gate {target:?}")
            }
            Self::WarpTargetsUnknownGate { warp, target } => {
                write!(f, "warp {warp:?} targets unknown gate {target:?}")
            }
            Self::WarpTargetsEnterGate { warp, target } => {
                write!(f, "warp {warp:?} targets enter gate {target:?}")
            }
            Self::FallbackSpawnGateMissing => {
                write!(f, "the fallback map has no spawn gate")
            }
            Self::UnknownMapRef { map } => write!(f, "reference to unknown map {map:?}"),
            Self::UnknownMonsterRef { monster } => {
                write!(f, "reference to unknown monster {monster:?}")
            }
            Self::UnknownSkillRef { skill } => write!(f, "reference to unknown skill {skill:?}"),
            Self::UnknownItemRef { item } => write!(f, "reference to unknown item {item:?}"),
        }
    }
}
