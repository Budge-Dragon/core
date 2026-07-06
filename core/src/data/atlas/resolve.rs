//! The resolution and indexing machinery [`Atlas::parse`](super::Atlas::parse)
//! runs: partitioning the gate/warp file, joining every cross-file edge to its
//! landing or definition while proving it resolves, and folding each per-file
//! record set into a by-id lookup with its identity uniqueness proven in the
//! same pass. Every helper returns the first [`AtlasError`] it finds.

use core::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use crate::components::collections::{EmptyCollection, OneOrMore};
use crate::components::spatial::WorldPos;
use crate::components::tile::{TileFacing, WalkGrid};
use crate::data::chaos_mixes::{ChaosMix, ChaosRecipe, UpgradeTarget};
use crate::data::common::{GateNumber, ItemRef, MapNumber, MonsterNumber, SkillNumber};
use crate::data::gates_warps::{EnterGate, GateWarpRecord, SpawnGate, TargetGate, Warp};
use crate::data::item_definitions::{ItemDefinition, ItemKind};
use crate::data::map_definitions::MapDefinition;
use crate::data::monster_definitions::MonsterDefinition;
use crate::data::npc_shops::{MerchantShop, ShelfSlot, ShelfStock};
use crate::data::skills::Skill;
use crate::data::spawns::Spawn;
use crate::data::terrain::MapTerrain;

use super::AtlasError;
use super::views::{
    Landing, ResolvedEnterGate, ResolvedOutput, ResolvedRecipe, ResolvedShelfEntry, ResolvedShop,
    ResolvedSpawn, ResolvedSpawnGate,
};

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

/// Resolves the respawn spawn gate of each map — the file-order-first gate on
/// the map — to its walkable landing set, RETAINING the join (the
/// [`resolve_spawns`] retain-at-parse precedent). The landing is the walkable
/// tiles inside the gate's area; a gate whose area holds none is the error, so
/// every retained landing is a non-empty [`OneOrMore`] and respawn's uniform
/// draw is total by type. Only the map's first gate is a respawn point (the
/// pinned first-gate policy); its later gates are travel-landing targets
/// resolved through [`GatePartition::landings`], not respawn sites, so their
/// walkability is not this invariant's concern.
pub(super) fn resolve_spawn_gates(
    spawn_gates: Vec<SpawnGate>,
    walk_grids: &BTreeMap<MapNumber, WalkGrid>,
) -> Result<BTreeMap<MapNumber, ResolvedSpawnGate>, AtlasError> {
    let mut first_by_map: BTreeMap<MapNumber, SpawnGate> = BTreeMap::new();
    for gate in spawn_gates {
        first_by_map.entry(gate.map).or_insert(gate);
    }

    let mut resolved = BTreeMap::new();
    for (map, gate) in first_by_map {
        // The grid is present — `check_maps` proved `gate.map` is a known map and
        // `index_terrain` proved every map carries one — so the absent branch is
        // unreachable; it folds to the empty-landing error, never a fabricated grid.
        let cells: Vec<WorldPos> = match walk_grids.get(&map) {
            Some(grid) => grid.walkable_positions_in(gate.area.to_world()).collect(),
            None => Vec::new(),
        };
        let landing = OneOrMore::new(cells).map_err(|EmptyCollection| {
            AtlasError::SpawnGateWithoutWalkableLanding {
                map,
                gate: gate.number,
            }
        })?;
        resolved.insert(map, ResolvedSpawnGate { gate, landing });
    }
    Ok(resolved)
}

/// Joins each merchant's shelf entries to their item definitions and re-proves
/// the shelf contract in the same pass, RETAINING the join anchor-indexed per
/// merchant (the [`resolve_spawns`] precedent): every entry `ItemRef`
/// resolves, every footprint (from the joined definition) fits the 8×15 grid
/// with no two entries overlapping, every stock tag agrees with its
/// definition's kind, and every stack fits its definition's cap. The
/// shop/merchant edge and per-NPC record uniqueness are the check module's
/// proof.
pub(super) fn resolve_shops(
    shops: Vec<MerchantShop>,
    items: &BTreeMap<ItemRef, ItemDefinition>,
) -> Result<BTreeMap<MonsterNumber, ResolvedShop>, AtlasError> {
    let mut by_npc = BTreeMap::new();
    for shop in shops {
        let npc = shop.npc;
        let mut occupied: BTreeSet<(u16, u16)> = BTreeSet::new();
        let mut entries = BTreeMap::new();
        for entry in shop.shelf {
            let slot = entry.slot;
            let def = items
                .get(&entry.item)
                .ok_or(AtlasError::UnknownItemRef { item: entry.item })?;
            if !stock_fits(&entry.stock, def) {
                return Err(AtlasError::ShelfStockKindMismatch { npc, slot });
            }
            if let ShelfStock::Stack { pieces } = &entry.stock {
                if pieces.get() > def.durability {
                    return Err(AtlasError::ShelfStackOverCap { npc, slot });
                }
            }
            claim_footprint(&mut occupied, npc, slot, def)?;
            entries.insert(
                slot,
                ResolvedShelfEntry {
                    level: entry.level,
                    stock: entry.stock,
                    def: def.clone(),
                },
            );
        }
        by_npc.insert(npc, ResolvedShop { entries });
    }
    Ok(by_npc)
}

/// Claims the cells an entry's footprint covers on the 8×15 grid, proving the
/// footprint stays in-grid and collides with no previously claimed cell
/// (which also proves anchor uniqueness — a repeated anchor collides on its
/// own first cell).
fn claim_footprint(
    occupied: &mut BTreeSet<(u16, u16)>,
    npc: MonsterNumber,
    slot: ShelfSlot,
    def: &ItemDefinition,
) -> Result<(), AtlasError> {
    let row = u16::from(slot.row());
    let col = u16::from(slot.col());
    let height = u16::from(def.height);
    let width = u16::from(def.width);
    if row + height > u16::from(ShelfSlot::ROWS) || col + width > u16::from(ShelfSlot::COLUMNS) {
        return Err(AtlasError::ShelfFootprintOutOfGrid { npc, slot });
    }
    for covered_row in row..row + height {
        for covered_col in col..col + width {
            if !occupied.insert((covered_row, covered_col)) {
                return Err(AtlasError::ShelfSlotOverlap { npc, slot });
            }
        }
    }
    Ok(())
}

/// The stock gate a definition's kind admits — the kind axis of the
/// parse-time stock/kind cross-check. The family discriminant is dual-sourced
/// (the wire stock tag and the joined kind), so the two must agree here once
/// and the buy service never re-checks.
enum ShelfKindBucket {
    /// Wearable non-ammo equipment — admits `Gear`.
    Gear,
    /// Ammunition — admits `Quiver`.
    Ammo,
    /// A consumable — admits `Stack` above durability 1, `Single` at 1.
    Consumable,
    /// Every other non-wearable kind — admits `Single` at durability 1.
    Inert,
}

/// The bucket of one kind — total over [`ItemKind`].
fn shelf_kind_bucket(kind: &ItemKind) -> ShelfKindBucket {
    match kind {
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
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
        | ItemKind::TransformationRing { .. } => ShelfKindBucket::Gear,
        ItemKind::Arrows { .. } | ItemKind::Bolts { .. } => ShelfKindBucket::Ammo,
        ItemKind::Consumable { .. } => ShelfKindBucket::Consumable,
        ItemKind::Orb { .. }
        | ItemKind::SkillScroll { .. }
        | ItemKind::Jewel { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => ShelfKindBucket::Inert,
    }
}

/// Whether a shelf entry's configured stock tag matches the joined
/// definition: `Gear` on a wearable non-ammo kind, `Stack` on a stackable
/// consumable (durability above 1), `Quiver` on ammunition, `Single` on any
/// durability-1 non-wearable piece (skill scrolls, orbs, Ale, Town Portal
/// Scroll).
fn stock_fits(stock: &ShelfStock, def: &ItemDefinition) -> bool {
    match (stock, shelf_kind_bucket(&def.kind)) {
        (ShelfStock::Gear { .. }, ShelfKindBucket::Gear)
        | (ShelfStock::Quiver, ShelfKindBucket::Ammo) => true,
        (ShelfStock::Stack { .. }, ShelfKindBucket::Consumable) => def.durability > 1,
        (ShelfStock::Single, ShelfKindBucket::Consumable | ShelfKindBucket::Inert) => {
            def.durability == 1
        }
        (
            ShelfStock::Gear { .. },
            ShelfKindBucket::Ammo | ShelfKindBucket::Consumable | ShelfKindBucket::Inert,
        )
        | (
            ShelfStock::Quiver,
            ShelfKindBucket::Gear | ShelfKindBucket::Consumable | ShelfKindBucket::Inert,
        )
        | (
            ShelfStock::Stack { .. },
            ShelfKindBucket::Gear | ShelfKindBucket::Ammo | ShelfKindBucket::Inert,
        )
        | (ShelfStock::Single, ShelfKindBucket::Gear | ShelfKindBucket::Ammo) => false,
    }
}

/// The authentic descending crafting-number scan order — a total match over
/// the recipe families ([`ChaosRecipe::ItemUpgrade`] split by target), never a
/// drift-prone data field. [`resolve_chaos_recipes`] sorts the catalog by this
/// once at parse, so [`Atlas::chaos_recipes`](super::Atlas::chaos_recipes)
/// needs no per-call sort.
fn crafting_number(recipe: &ChaosRecipe) -> u8 {
    match recipe {
        ChaosRecipe::CapeOfLord { .. } => 24,
        ChaosRecipe::FirstWings { .. } => 11,
        ChaosRecipe::BloodCastleTicket { .. } => 8,
        ChaosRecipe::SecondWings { .. } => 7,
        ChaosRecipe::Fruits { .. } => 6,
        ChaosRecipe::Dinorant { .. } => 5,
        ChaosRecipe::ItemUpgrade {
            target: UpgradeTarget::PlusEleven,
            ..
        } => 4,
        ChaosRecipe::ItemUpgrade {
            target: UpgradeTarget::PlusTen,
            ..
        } => 3,
        ChaosRecipe::DevilSquareTicket { .. } => 2,
        ChaosRecipe::ChaosWeapon { .. } => 1,
    }
}

/// Joins one output ref to its owned definition, proving it resolves.
fn joined_item(
    items: &BTreeMap<ItemRef, ItemDefinition>,
    item: ItemRef,
) -> Result<ItemDefinition, AtlasError> {
    items
        .get(&item)
        .cloned()
        .ok_or(AtlasError::UnknownItemRef { item })
}

/// Joins a multi-candidate output — a proven-present head plus its tail, so
/// the non-empty pick pool builds totally (no fallible conversion downstream).
fn joined_choice(
    items: &BTreeMap<ItemRef, ItemDefinition>,
    first: ItemRef,
    rest: &[ItemRef],
) -> Result<ResolvedOutput, AtlasError> {
    let tail: Vec<ItemDefinition> = rest
        .iter()
        .map(|&item| joined_item(items, item))
        .collect::<Result<_, _>>()?;
    Ok(ResolvedOutput::Choice(OneOrMore::with_head(
        joined_item(items, first)?,
        tail,
    )))
}

/// Joins a deterministic single output — no pick draw exists for it.
fn joined_single(
    items: &BTreeMap<ItemRef, ItemDefinition>,
    item: ItemRef,
) -> Result<ResolvedOutput, AtlasError> {
    Ok(ResolvedOutput::Single(joined_item(items, item)?))
}

/// Joins each chaos recipe's output refs to their definitions and sorts the
/// catalog into descending authentic crafting-number scan order, proving the
/// output refs resolve and RETAINING the proof as the join (the
/// [`resolve_spawns`] precedent; ingredient refs are proven by the check
/// module and stay refs on the resolved record).
pub(super) fn resolve_chaos_recipes(
    mixes: Vec<ChaosMix>,
    items: &BTreeMap<ItemRef, ItemDefinition>,
) -> Result<Vec<ResolvedRecipe>, AtlasError> {
    let mut recipes: Vec<ChaosRecipe> = mixes.into_iter().map(|mix| mix.recipe).collect();
    recipes.sort_by_key(|recipe| Reverse(crafting_number(recipe)));

    let mut resolved = Vec::with_capacity(recipes.len());
    for recipe in recipes {
        resolved.push(resolve_chaos_recipe(&recipe, items)?);
    }
    Ok(resolved)
}

/// Joins one recipe's outputs: [`ResolvedOutput::Choice`] over the fixed
/// multi-candidate arrays (head split off, so `OneOrMore` builds totally),
/// [`ResolvedOutput::Single`] for the deterministic outputs; every other field
/// carries over verbatim. One arm per family — the function's length is the
/// nine-family catalog's vocabulary breadth, not a second concern.
fn resolve_chaos_recipe(
    recipe: &ChaosRecipe,
    items: &BTreeMap<ItemRef, ItemDefinition>,
) -> Result<ResolvedRecipe, AtlasError> {
    match *recipe {
        ChaosRecipe::ChaosWeapon {
            sacrifice_levels,
            weapons,
        } => {
            let [first, rest @ ..] = weapons;
            Ok(ResolvedRecipe::ChaosWeapon {
                sacrifice_levels,
                weapons: joined_choice(items, first, &rest)?,
            })
        }
        ChaosRecipe::FirstWings {
            chaos_weapons,
            chaos_weapon_levels,
            extra_sacrifice_levels,
            wings,
        } => {
            let [first, rest @ ..] = wings;
            Ok(ResolvedRecipe::FirstWings {
                chaos_weapons,
                chaos_weapon_levels,
                extra_sacrifice_levels,
                wings: joined_choice(items, first, &rest)?,
            })
        }
        ChaosRecipe::SecondWings {
            first_wings,
            wing_levels,
            excellent_levels,
            feather,
            economics,
            wings,
        } => {
            let [first, rest @ ..] = wings;
            Ok(ResolvedRecipe::SecondWings {
                first_wings,
                wing_levels,
                excellent_levels,
                feather,
                economics,
                wings: joined_choice(items, first, &rest)?,
            })
        }
        ChaosRecipe::CapeOfLord {
            first_wings,
            wing_levels,
            excellent_levels,
            crest,
            economics,
            cape,
        } => Ok(ResolvedRecipe::CapeOfLord {
            first_wings,
            wing_levels,
            excellent_levels,
            crest,
            economics,
            cape: joined_single(items, cape)?,
        }),
        ChaosRecipe::ItemUpgrade {
            target,
            bless,
            soul,
            base_success_percent,
            fee_zen,
        } => Ok(ResolvedRecipe::ItemUpgrade {
            target,
            bless,
            soul,
            base_success_percent,
            fee_zen,
        }),
        ChaosRecipe::Dinorant {
            horn,
            horn_count,
            success_percent,
            fee_zen,
            dinorant,
        } => Ok(ResolvedRecipe::Dinorant {
            horn,
            horn_count,
            success_percent,
            fee_zen,
            dinorant: joined_single(items, dinorant)?,
        }),
        ChaosRecipe::Fruits {
            catalyst,
            success_percent,
            fee_zen,
            fruit,
        } => Ok(ResolvedRecipe::Fruits {
            catalyst,
            success_percent,
            fee_zen,
            fruit: joined_single(items, fruit)?,
        }),
        ChaosRecipe::DevilSquareTicket {
            eye,
            key,
            invitation,
            fee_zen_by_level,
            success_percent_by_level,
        } => Ok(ResolvedRecipe::DevilSquareTicket {
            eye,
            key,
            invitation: joined_single(items, invitation)?,
            fee_zen_by_level,
            success_percent_by_level,
        }),
        ChaosRecipe::BloodCastleTicket {
            scroll,
            bone,
            cloak,
            fee_zen_by_level,
            success_percent_by_level,
        } => Ok(ResolvedRecipe::BloodCastleTicket {
            scroll,
            bone,
            cloak: joined_single(items, cloak)?,
            fee_zen_by_level,
            success_percent_by_level,
        }),
    }
}
