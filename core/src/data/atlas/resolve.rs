//! The resolution and indexing machinery [`Atlas::parse`](super::Atlas::parse)
//! runs: partitioning the gate/warp file, joining every cross-file edge to its
//! landing or definition while proving it resolves, and folding each per-file
//! record set into a by-id lookup with its identity uniqueness proven in the
//! same pass. Every helper returns the first [`AtlasError`] it finds.

use std::collections::{BTreeMap, BTreeSet};

use crate::components::tile::{TileFacing, WalkGrid};
use crate::data::chaos_mixes::ChaosRecipe;
use crate::data::common::{GateNumber, ItemRef, MapNumber, MonsterNumber, SkillNumber};
use crate::data::gates_warps::{EnterGate, GateWarpRecord, SpawnGate, TargetGate, Warp};
use crate::data::item_definitions::ItemDefinition;
use crate::data::map_definitions::MapDefinition;
use crate::data::monster_definitions::MonsterDefinition;
use crate::data::skills::Skill;
use crate::data::spawns::Spawn;
use crate::data::terrain::MapTerrain;

use super::AtlasError;
use super::views::{Landing, ResolvedEnterGate, ResolvedSpawn};

/// The gate records partitioned by kind, with per-file identity sets proven
/// unique.
pub(super) struct GatePartition {
    pub(super) spawn_gates: Vec<SpawnGate>,
    pub(super) enter_gates: Vec<EnterGate>,
    target_gates: Vec<TargetGate>,
    pub(super) warps: Vec<Warp>,
    pub(super) enter_gate_numbers: BTreeSet<GateNumber>,
}

impl GatePartition {
    pub(super) fn partition(records: Vec<GateWarpRecord>) -> Result<Self, AtlasError> {
        let mut spawn_gates = Vec::new();
        let mut enter_gates = Vec::new();
        let mut target_gates = Vec::new();
        let mut warps = Vec::new();
        let mut gate_numbers = BTreeSet::new();
        let mut warp_indices = BTreeSet::new();
        let mut enter_gate_numbers = BTreeSet::new();

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

    pub(super) fn check_maps(&self, map_numbers: &BTreeSet<MapNumber>) -> Result<(), AtlasError> {
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

    pub(super) fn landings(&self) -> BTreeMap<GateNumber, Landing> {
        let mut landings = BTreeMap::new();
        for gate in &self.spawn_gates {
            landings.insert(
                gate.number,
                Landing {
                    map: gate.map,
                    area: gate.area.to_world(),
                    facing: gate.direction.map(TileFacing::to_facing),
                },
            );
        }
        for gate in &self.target_gates {
            landings.insert(
                gate.number,
                Landing {
                    map: gate.map,
                    area: gate.area.to_world(),
                    facing: gate.direction.map(TileFacing::to_facing),
                },
            );
        }
        landings
    }
}

/// Joins each warp to its landing, proving referential integrity in the same
/// pass. Every returned pair carries a resolved [`Landing`]; a target that
/// resolves to nothing (or to a wrong-kind enter gate) is the error.
pub(super) fn resolve_warps(
    warps: Vec<Warp>,
    landings: &BTreeMap<GateNumber, Landing>,
    enter_gate_numbers: &BTreeSet<GateNumber>,
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
pub(super) fn resolve_enter_gates(
    enter_gates: Vec<EnterGate>,
    landings: &BTreeMap<GateNumber, Landing>,
    enter_gate_numbers: &BTreeSet<GateNumber>,
) -> Result<BTreeMap<MapNumber, Vec<ResolvedEnterGate>>, AtlasError> {
    let mut by_map: BTreeMap<MapNumber, Vec<ResolvedEnterGate>> = BTreeMap::new();
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
        let trigger = gate.area.to_world();
        let map = gate.map;
        by_map.entry(map).or_default().push(ResolvedEnterGate {
            gate,
            trigger,
            landing,
        });
    }
    Ok(by_map)
}

/// Extracts the sole record of a singleton file; the count is the error payload
/// when the file does not carry exactly one.
pub(super) fn take_single<T>(records: Vec<T>) -> Result<T, usize> {
    let found = records.len();
    let mut iter = records.into_iter();
    match (iter.next(), iter.next()) {
        (Some(only), None) => Ok(only),
        (None | Some(_), _) => Err(found),
    }
}

pub(super) fn claim_gate(
    seen: &mut BTreeSet<GateNumber>,
    number: GateNumber,
) -> Result<(), AtlasError> {
    if seen.insert(number) {
        Ok(())
    } else {
        Err(AtlasError::DuplicateGateNumber { number })
    }
}

pub(super) fn require_map(
    map_numbers: &BTreeSet<MapNumber>,
    gate: GateNumber,
    map: MapNumber,
) -> Result<(), AtlasError> {
    if map_numbers.contains(&map) {
        Ok(())
    } else {
        Err(AtlasError::GateOnUnknownMap { gate, map })
    }
}

/// Parses each terrain sidecar into a [`WalkGrid`] and proves a bijection with
/// the map set: every terrain names a known map, no two share a map, and every
/// map carries exactly one terrain — so the resulting per-map lookup is complete.
pub(super) fn index_terrain(
    terrain: Vec<MapTerrain>,
    map_numbers: &BTreeSet<MapNumber>,
) -> Result<BTreeMap<MapNumber, WalkGrid>, AtlasError> {
    let mut grids: BTreeMap<MapNumber, WalkGrid> = BTreeMap::new();
    for entry in terrain {
        if !map_numbers.contains(&entry.map) {
            return Err(AtlasError::TerrainForUnknownMap { map: entry.map });
        }
        let grid = WalkGrid::from_terrain(entry.bytes.as_array());
        if grids.insert(entry.map, grid).is_some() {
            return Err(AtlasError::DuplicateTerrain { map: entry.map });
        }
    }
    for &map in map_numbers {
        if !grids.contains_key(&map) {
            return Err(AtlasError::TerrainMissingForMap { map });
        }
    }
    Ok(grids)
}

pub(super) fn index_maps(
    maps: Vec<MapDefinition>,
) -> Result<BTreeMap<MapNumber, MapDefinition>, AtlasError> {
    let mut by_number = BTreeMap::new();
    for map in maps {
        let number = map.number;
        if by_number.insert(number, map).is_some() {
            return Err(AtlasError::DuplicateMapNumber { number });
        }
    }
    Ok(by_number)
}

pub(super) fn index_monsters(
    monsters: Vec<MonsterDefinition>,
) -> Result<BTreeMap<MonsterNumber, MonsterDefinition>, AtlasError> {
    let mut by_number = BTreeMap::new();
    for monster in monsters {
        let number = monster.number;
        if by_number.insert(number, monster).is_some() {
            return Err(AtlasError::DuplicateMonsterNumber { number });
        }
    }
    Ok(by_number)
}

pub(super) fn index_skills(skills: Vec<Skill>) -> Result<BTreeMap<SkillNumber, Skill>, AtlasError> {
    let mut by_number = BTreeMap::new();
    for skill in skills {
        let number = skill.number;
        if by_number.insert(number, skill).is_some() {
            return Err(AtlasError::DuplicateSkillNumber { number });
        }
    }
    Ok(by_number)
}

pub(super) fn index_items(
    items: Vec<ItemDefinition>,
) -> Result<BTreeMap<ItemRef, ItemDefinition>, AtlasError> {
    let mut by_ref = BTreeMap::new();
    for item in items {
        let id = item.id;
        if by_ref.insert(id, item).is_some() {
            return Err(AtlasError::DuplicateItemRef { item: id });
        }
    }
    Ok(by_ref)
}

/// Joins each spawn to the monster definition it names and groups the pairs by
/// map, proving referential integrity in the same pass and RETAINING the join.
/// A spawn on an unknown map, or naming a monster with no record, is the error;
/// otherwise every retained [`ResolvedSpawn`] carries a definition proven present.
pub(super) fn resolve_spawns(
    spawns: Vec<Spawn>,
    map_numbers: &BTreeSet<MapNumber>,
    monsters: &BTreeMap<MonsterNumber, MonsterDefinition>,
) -> Result<BTreeMap<MapNumber, Vec<ResolvedSpawn>>, AtlasError> {
    let mut by_map: BTreeMap<MapNumber, Vec<ResolvedSpawn>> = BTreeMap::new();
    for spawn in spawns {
        if !map_numbers.contains(&spawn.map) {
            return Err(AtlasError::UnknownMapRef { map: spawn.map });
        }
        let monster = match monsters.get(&spawn.monster) {
            Some(monster) => monster.clone(),
            None => {
                return Err(AtlasError::UnknownMonsterRef {
                    monster: spawn.monster,
                });
            }
        };
        by_map
            .entry(spawn.map)
            .or_default()
            .push(ResolvedSpawn { spawn, monster });
    }
    Ok(by_map)
}

pub(super) fn recipe_item_refs(recipe: &ChaosRecipe) -> Vec<ItemRef> {
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
