//! Cross-checked, resolved view over the entire static dataset, built once at
//! load. `Atlas::parse` is the single referential-integrity proof for every v2
//! file: per-file identity uniqueness plus resolution of every declared
//! cross-file reference, in one pass — and it *keeps* the proven-unique records
//! as total by-id lookups, so a consuming service reaches any definition
//! through the Atlas without re-scanning a raw `Vec`. Every accessor downstream
//! is total or genuinely optional.

use std::collections::{BTreeMap, BTreeSet};

use crate::components::interval::Interval;
use crate::components::levels::TransformationLevel;
use crate::components::spatial::{Facing, WorldPos, WorldRect};
use crate::components::tile::{TileFacing, WalkGrid};

use super::ancient_sets::{AncientRoster, AncientRosterError, AncientSet};
use super::box_drops::BoxDrop;
use super::chaos_mixes::{ChaosMix, ChaosRecipe};
use super::classes::{ClassRecord, ClassTable, ClassTableError};
use super::common::{DataFile, GateNumber, ItemRef, MapNumber, MonsterNumber, SkillNumber};
use super::drop_config::DropConfig;
use super::exp_tables::{ExpCurve, ExpTable, ExpTableError};
use super::game_config::GameConfig;
use super::gates_warps::{EnterGate, GateWarpRecord, SpawnGate, TargetGate, Warp, WarpIndex};
use super::item_definitions::{ItemDefinition, ItemKind};
use super::map_definitions::MapDefinition;
use super::monster_definitions::{MonsterAttack, MonsterDefinition, MonsterRole};
use super::skills::{Skill, SkillShape};
use super::spawns::Spawn;
use super::special_drops::{SpecialDrop, SpecialDropRecord};
use super::terrain::MapTerrain;

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
    /// The 11 `terrain/<map>.bin` walkability sidecars, one per map.
    pub terrain: Vec<MapTerrain>,
}

/// The landing side of a resolved gate reference, in world space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Landing {
    /// Map the traveler lands on.
    pub map: MapNumber,
    /// Landing rectangle in world space.
    pub area: WorldRect,
    /// Facing on arrival; absent = unspecified (never fabricated).
    pub facing: Option<Facing>,
}

/// An enter gate with its landing resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnterGateView<'a> {
    /// The trigger-side record.
    pub gate: &'a EnterGate,
    /// Where its target gate lands travelers.
    pub landing: Landing,
}

/// An enter gate with its trigger area projected to world space and its
/// landing resolved at parse.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedEnterGate {
    gate: EnterGate,
    trigger: WorldRect,
    landing: Landing,
}

/// A warp entry with its landing resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarpView<'a> {
    /// The warp-list record.
    pub warp: &'a Warp,
    /// Where its target gate lands travelers.
    pub landing: Landing,
}

/// A spawn record joined to the monster definition it names, retained per map
/// at parse. The join is proven total here (the monster is looked up once,
/// during resolution), so a consuming service reaches the definition without an
/// `Option` — mirroring the `warps: Vec<(Warp, Landing)>` owned-join precedent.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedSpawn {
    spawn: Spawn,
    monster: MonsterDefinition,
}

/// A spawn record borrowed with the monster definition it resolves to. The
/// public view over a [`ResolvedSpawn`], mirroring [`WarpView`]/[`EnterGateView`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnEntry<'a> {
    /// The spawn record.
    pub spawn: &'a Spawn,
    /// The monster definition it names — resolution proven at parse.
    pub monster: &'a MonsterDefinition,
}

/// A proven-present view of one map: its definition, its walk grid, and its
/// spawns joined to their monster definitions. Minted only by the [`Atlas`]
/// from resolved state — there is no public fabricating constructor — so its
/// walk grid and spawns are total, never `Option`.
#[derive(Debug, Clone, Copy)]
pub struct MapHandle<'a> {
    definition: &'a MapDefinition,
    walk_grid: &'a WalkGrid,
    spawns: &'a [ResolvedSpawn],
}

impl<'a> MapHandle<'a> {
    /// The map's definition — its environment and, on Arena, its soccer pitch.
    #[must_use]
    pub fn definition(&self) -> &'a MapDefinition {
        self.definition
    }

    /// The map's walk grid. Total — presence was proven at parse.
    #[must_use]
    pub fn walk_grid(&self) -> &'a WalkGrid {
        self.walk_grid
    }

    /// The map's spawn entries, each already joined to its monster definition.
    pub fn spawns(&self) -> impl Iterator<Item = SpawnEntry<'a>> {
        self.spawns.iter().map(|resolved| SpawnEntry {
            spawn: &resolved.spawn,
            monster: &resolved.monster,
        })
    }
}

/// Droppable items grouped by their base drop level, built once at load so drop
/// resolution range-queries the eligible pool (`O(log n)` plus the matches)
/// instead of linear-scanning every item definition on every kill. An item
/// enters the pool only when it drops from monsters; the classic per-level drop
/// index (OpenMU's `DropItemGroup`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropPool {
    by_drop_level: BTreeMap<u8, Vec<ItemRef>>,
}

impl DropPool {
    fn build<'a>(items: impl Iterator<Item = &'a ItemDefinition>) -> Self {
        let mut by_drop_level: BTreeMap<u8, Vec<ItemRef>> = BTreeMap::new();
        for item in items {
            if item.drops_from_monsters {
                by_drop_level
                    .entry(item.drop_level)
                    .or_default()
                    .push(item.id);
            }
        }
        Self { by_drop_level }
    }

    /// The droppable items whose base drop level falls in the inclusive window
    /// (a monster's level pool: floor = `monster_level - gap`, ceiling =
    /// `monster_level`). The window is an [`Interval`], so `min <= max` is
    /// proven and the range query never panics; an empty iterator is the
    /// genuine "no eligible item" answer.
    pub fn in_window(&self, window: Interval<u8>) -> impl Iterator<Item = ItemRef> + '_ {
        self.by_drop_level
            .range(window.min()..=window.max())
            .flat_map(|(_, refs)| refs.iter().copied())
    }
}

/// The static dataset with every cross-file reference resolved and every
/// proven-unique record retained as a total by-id lookup. Construction proves,
/// dataset-wide, per-file identity uniqueness and resolution of every declared
/// cross-file edge, then keeps the resolved store so a resolved handle proves
/// its referent exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Atlas {
    maps: BTreeMap<MapNumber, MapDefinition>,
    spawns_by_map: BTreeMap<MapNumber, Vec<ResolvedSpawn>>,
    spawn_gates_by_map: BTreeMap<MapNumber, Vec<SpawnGate>>,
    enter_gates_by_map: BTreeMap<MapNumber, Vec<ResolvedEnterGate>>,
    warps: Vec<(Warp, Landing)>,
    fallback: SpawnGate,
    walk_grids: BTreeMap<MapNumber, WalkGrid>,
    items: BTreeMap<ItemRef, ItemDefinition>,
    monsters: BTreeMap<MonsterNumber, MonsterDefinition>,
    skills: BTreeMap<SkillNumber, Skill>,
    classes: ClassTable,
    exp_curve: ExpCurve,
    ancient_roster: AncientRoster,
    drop_config: DropConfig,
    special_drops: Vec<SpecialDropRecord>,
    box_drops: Vec<BoxDrop>,
    drop_pool: DropPool,
}

impl Atlas {
    /// Builds the atlas from the whole dataset, proving referential integrity
    /// of every file in one pass and retaining the resolved store.
    ///
    /// # Errors
    /// Returns the first [`AtlasError`] found: a duplicated per-file identity, a
    /// cross-file reference that resolves to nothing (or to a wrong-kind gate),
    /// a malformed resolved structure (class table, experience curve, ancient
    /// roster), or a singleton config file that is not exactly one record.
    pub fn parse(data: StaticData) -> Result<Self, AtlasError> {
        let maps = index_maps(data.maps.records)?;
        let map_numbers: BTreeSet<MapNumber> = maps.keys().copied().collect();
        let monsters = index_monsters(data.monsters.records)?;
        let skills = index_skills(data.skills.records)?;
        let items = index_items(data.items.records)?;

        let gates = GatePartition::partition(data.gates_warps.records)?;
        gates.check_maps(&map_numbers)?;

        let landings = gates.landings();
        let warps = resolve_warps(gates.warps, &landings, &gates.enter_gate_numbers)?;
        let enter_gates_by_map =
            resolve_enter_gates(gates.enter_gates, &landings, &gates.enter_gate_numbers)?;

        let spawns_by_map = resolve_spawns(data.spawns.records, &map_numbers, &monsters)?;
        check_monster_attacks(&monsters, &skills)?;
        check_summons(&skills, &monsters)?;
        check_items(&items, &skills, &monsters)?;
        check_ancient_sets(&data.ancient_sets.records, &items)?;
        check_chaos_mixes(&data.chaos_mixes.records, &items)?;
        check_special_drops(&data.special_drops.records, &items, &monsters, &map_numbers)?;
        check_box_drops(&data.box_drops.records, &items)?;
        check_classes(&data.classes.records, &map_numbers)?;

        let classes = ClassTable::try_from(data.classes.records).map_err(AtlasError::ClassTable)?;
        let exp_table = take_single(data.exp_tables.records)
            .map_err(|found| AtlasError::ExpTableNotSingle { found })?;
        let exp_curve = ExpCurve::parse(exp_table).map_err(AtlasError::ExpCurve)?;
        let ancient_roster =
            AncientRoster::build(data.ancient_sets.records).map_err(AtlasError::AncientRoster)?;
        let game_config = take_single(data.game_config.records)
            .map_err(|found| AtlasError::GameConfigNotSingle { found })?;
        let drop_config = game_config.drops;
        let drop_pool = DropPool::build(items.values());

        let fallback = gates
            .spawn_gates
            .iter()
            .find(|gate| gate.map == FALLBACK_MAP)
            .cloned()
            .ok_or(AtlasError::FallbackSpawnGateMissing)?;

        let mut spawn_gates_by_map: BTreeMap<MapNumber, Vec<SpawnGate>> = BTreeMap::new();
        for gate in gates.spawn_gates {
            spawn_gates_by_map.entry(gate.map).or_default().push(gate);
        }

        let walk_grids = index_terrain(data.terrain, &map_numbers)?;

        Ok(Self {
            maps,
            spawns_by_map,
            spawn_gates_by_map,
            enter_gates_by_map,
            warps,
            fallback,
            walk_grids,
            items,
            monsters,
            skills,
            classes,
            exp_curve,
            ancient_roster,
            drop_config,
            special_drops: data.special_drops.records,
            box_drops: data.box_drops.records,
            drop_pool,
        })
    }

    /// All maps, ordered by number.
    pub fn maps(&self) -> impl Iterator<Item = &MapDefinition> {
        self.maps.values()
    }

    /// A proven-present handle per map, ordered by number. Both `maps` and
    /// `walk_grids` are keyed by the identical map-number set proven at parse,
    /// so iterating them in lockstep pairs each definition with its own walk
    /// grid — total, with no `Option` at any call site.
    pub fn map_handles(&self) -> impl Iterator<Item = MapHandle<'_>> {
        self.maps
            .values()
            .zip(self.walk_grids.values())
            .map(move |(definition, walk_grid)| MapHandle {
                definition,
                walk_grid,
                spawns: self.map_spawns(definition.number),
            })
    }

    /// The handle for one map; `None` when no record carries it — genuine
    /// optionality of an open `MapNumber` key. A number taken from a resolved
    /// edge is proven present by `parse`.
    #[must_use]
    pub fn map_handle(&self, map: MapNumber) -> Option<MapHandle<'_>> {
        let definition = self.maps.get(&map)?;
        let walk_grid = self.walk_grids.get(&map)?;
        Some(MapHandle {
            definition,
            walk_grid,
            spawns: self.map_spawns(map),
        })
    }

    /// The retained spawns for a map; the empty slice for a map with none — a
    /// real "this map spawns nothing" answer, mirroring [`Atlas::spawn_gates`].
    fn map_spawns(&self, map: MapNumber) -> &[ResolvedSpawn] {
        match self.spawns_by_map.get(&map) {
            Some(spawns) => spawns,
            None => &[],
        }
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

    /// The enter gate whose world trigger area covers a position, if any. The
    /// landing was resolved at parse, so the only optionality is whether a gate
    /// covers the position.
    #[must_use]
    pub fn enter_gate_at(&self, map: MapNumber, pos: WorldPos) -> Option<EnterGateView<'_>> {
        let gates = self.enter_gates_by_map.get(&map)?;
        let resolved = gates
            .iter()
            .find(|resolved| resolved.trigger.contains(pos))?;
        Some(EnterGateView {
            gate: &resolved.gate,
            landing: resolved.landing,
        })
    }

    /// The walk grid for a map; `None` when no record carries it — genuine
    /// optionality of an open `MapNumber` key. A number taken from a resolved
    /// edge (spawn, gate, landing, class home) is proven present by `parse`.
    #[must_use]
    pub fn walk_grid(&self, map: MapNumber) -> Option<&WalkGrid> {
        self.walk_grids.get(&map)
    }

    /// The warp list in index order, each with its landing resolved at parse.
    pub fn warps(&self) -> impl Iterator<Item = WarpView<'_>> {
        self.warps.iter().map(|(warp, landing)| WarpView {
            warp,
            landing: *landing,
        })
    }

    /// The item definition for an identity; `None` when no record carries it —
    /// genuine optionality of an open `{group, number}` key. A ref taken from a
    /// resolved edge (ancient piece, chaos recipe, drop) is proven present by
    /// `parse`.
    #[must_use]
    pub fn item(&self, id: ItemRef) -> Option<&ItemDefinition> {
        self.items.get(&id)
    }

    /// Every item definition, ordered by identity.
    pub fn items(&self) -> impl Iterator<Item = &ItemDefinition> {
        self.items.values()
    }

    /// The monster definition for a number; `None` when no record carries it —
    /// genuine optionality of an open number. A number taken from a resolved
    /// edge (spawn, summon, transformation skin, skill attack) is proven present
    /// by `parse`.
    #[must_use]
    pub fn monster(&self, number: MonsterNumber) -> Option<&MonsterDefinition> {
        self.monsters.get(&number)
    }

    /// Every monster definition, ordered by number.
    pub fn monsters(&self) -> impl Iterator<Item = &MonsterDefinition> {
        self.monsters.values()
    }

    /// The skill definition for a number; `None` when no record carries it —
    /// genuine optionality of an open number. A number taken from a resolved
    /// edge (monster attack, item skill/teaches, summon) is proven present by
    /// `parse`.
    #[must_use]
    pub fn skill(&self, number: SkillNumber) -> Option<&Skill> {
        self.skills.get(&number)
    }

    /// Every skill definition, ordered by number.
    pub fn skills(&self) -> impl Iterator<Item = &Skill> {
        self.skills.values()
    }

    /// The total class lookup — every class present exactly once, proven at
    /// parse.
    #[must_use]
    pub fn classes(&self) -> &ClassTable {
        &self.classes
    }

    /// The experience curve — level cap and per-level thresholds, proven at
    /// parse.
    #[must_use]
    pub fn exp_curve(&self) -> &ExpCurve {
        &self.exp_curve
    }

    /// The ancient set roster and its membership lookup.
    #[must_use]
    pub fn ancient_roster(&self) -> &AncientRoster {
        &self.ancient_roster
    }

    /// The global per-kill drop tuning.
    #[must_use]
    pub fn drop_config(&self) -> &DropConfig {
        &self.drop_config
    }

    /// The special-drop records, in load order.
    #[must_use]
    pub fn special_drops(&self) -> &[SpecialDropRecord] {
        &self.special_drops
    }

    /// The openable-box drop records, in load order.
    #[must_use]
    pub fn box_drops(&self) -> &[BoxDrop] {
        &self.box_drops
    }

    /// The per-level drop pool index over the droppable items.
    #[must_use]
    pub fn drop_pool(&self) -> &DropPool {
        &self.drop_pool
    }
}

/// The gate records partitioned by kind, with per-file identity sets proven
/// unique.
struct GatePartition {
    spawn_gates: Vec<SpawnGate>,
    enter_gates: Vec<EnterGate>,
    target_gates: Vec<TargetGate>,
    warps: Vec<Warp>,
    enter_gate_numbers: BTreeSet<GateNumber>,
}

impl GatePartition {
    fn partition(records: Vec<GateWarpRecord>) -> Result<Self, AtlasError> {
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

    fn check_maps(&self, map_numbers: &BTreeSet<MapNumber>) -> Result<(), AtlasError> {
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

    fn landings(&self) -> BTreeMap<GateNumber, Landing> {
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
fn resolve_warps(
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
fn resolve_enter_gates(
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
fn take_single<T>(records: Vec<T>) -> Result<T, usize> {
    let found = records.len();
    let mut iter = records.into_iter();
    match (iter.next(), iter.next()) {
        (Some(only), None) => Ok(only),
        (None | Some(_), _) => Err(found),
    }
}

fn claim_gate(seen: &mut BTreeSet<GateNumber>, number: GateNumber) -> Result<(), AtlasError> {
    if seen.insert(number) {
        Ok(())
    } else {
        Err(AtlasError::DuplicateGateNumber { number })
    }
}

fn require_map(
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
fn index_terrain(
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

fn index_maps(maps: Vec<MapDefinition>) -> Result<BTreeMap<MapNumber, MapDefinition>, AtlasError> {
    let mut by_number = BTreeMap::new();
    for map in maps {
        let number = map.number;
        if by_number.insert(number, map).is_some() {
            return Err(AtlasError::DuplicateMapNumber { number });
        }
    }
    Ok(by_number)
}

fn index_monsters(
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

fn index_skills(skills: Vec<Skill>) -> Result<BTreeMap<SkillNumber, Skill>, AtlasError> {
    let mut by_number = BTreeMap::new();
    for skill in skills {
        let number = skill.number;
        if by_number.insert(number, skill).is_some() {
            return Err(AtlasError::DuplicateSkillNumber { number });
        }
    }
    Ok(by_number)
}

fn index_items(
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
fn resolve_spawns(
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

fn check_monster_attacks(
    monsters: &BTreeMap<MonsterNumber, MonsterDefinition>,
    skills: &BTreeMap<SkillNumber, Skill>,
) -> Result<(), AtlasError> {
    for monster in monsters.values() {
        let attack = match &monster.role {
            MonsterRole::Monster { attack, .. } | MonsterRole::Trap { attack, .. } => attack,
            MonsterRole::Guard { .. } | MonsterRole::Npc { .. } | MonsterRole::SoccerBall => {
                continue;
            }
        };
        if let MonsterAttack::Skill { skill } = attack {
            require_skill(skills, *skill)?;
        }
    }
    Ok(())
}

fn check_summons(
    skills: &BTreeMap<SkillNumber, Skill>,
    monsters: &BTreeMap<MonsterNumber, MonsterDefinition>,
) -> Result<(), AtlasError> {
    for skill in skills.values() {
        if let SkillShape::Summon { monster } = skill.shape {
            require_monster(monsters, monster)?;
        }
    }
    Ok(())
}

fn check_items(
    items: &BTreeMap<ItemRef, ItemDefinition>,
    skills: &BTreeMap<SkillNumber, Skill>,
    monsters: &BTreeMap<MonsterNumber, MonsterDefinition>,
) -> Result<(), AtlasError> {
    for item in items.values() {
        match &item.kind {
            ItemKind::Weapon { skill, .. }
            | ItemKind::Bow { skill, .. }
            | ItemKind::Crossbow { skill, .. }
            | ItemKind::Staff { skill, .. }
            | ItemKind::Shield { skill, .. }
            | ItemKind::Pet { skill, .. } => {
                if let Some(skill) = skill {
                    require_skill(skills, *skill)?;
                }
            }
            ItemKind::Orb { teaches, .. } | ItemKind::SkillScroll { teaches, .. } => {
                require_skill(skills, *teaches)?;
            }
            ItemKind::TransformationRing { skins, .. } => {
                for level in TransformationLevel::ALL {
                    require_monster(monsters, skins.skin(level))?;
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

fn check_ancient_sets(
    sets: &[AncientSet],
    items: &BTreeMap<ItemRef, ItemDefinition>,
) -> Result<(), AtlasError> {
    for set in sets {
        for piece in &set.pieces {
            require_item(items, piece.item)?;
        }
    }
    Ok(())
}

fn check_chaos_mixes(
    mixes: &[ChaosMix],
    items: &BTreeMap<ItemRef, ItemDefinition>,
) -> Result<(), AtlasError> {
    for mix in mixes {
        for item in recipe_item_refs(&mix.recipe) {
            require_item(items, item)?;
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
    items: &BTreeMap<ItemRef, ItemDefinition>,
    monsters: &BTreeMap<MonsterNumber, MonsterDefinition>,
    map_numbers: &BTreeSet<MapNumber>,
) -> Result<(), AtlasError> {
    for record in records {
        match &record.drop {
            SpecialDrop::LevelBanded { item, .. } => require_item(items, *item)?,
            SpecialDrop::MonsterBound {
                monster,
                items: drops,
                ..
            } => {
                require_monster(monsters, *monster)?;
                for &item in drops.iter() {
                    require_item(items, item)?;
                }
            }
            SpecialDrop::MapBound { map, item, .. } => {
                if !map_numbers.contains(map) {
                    return Err(AtlasError::UnknownMapRef { map: *map });
                }
                require_item(items, *item)?;
            }
        }
    }
    Ok(())
}

fn check_box_drops(
    records: &[BoxDrop],
    items: &BTreeMap<ItemRef, ItemDefinition>,
) -> Result<(), AtlasError> {
    for record in records {
        require_item(items, record.box_item)?;
        for &item in record.items.iter() {
            require_item(items, item)?;
        }
    }
    Ok(())
}

fn check_classes(
    classes: &[ClassRecord],
    map_numbers: &BTreeSet<MapNumber>,
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

fn require_skill(
    skills: &BTreeMap<SkillNumber, Skill>,
    skill: SkillNumber,
) -> Result<(), AtlasError> {
    if skills.contains_key(&skill) {
        Ok(())
    } else {
        Err(AtlasError::UnknownSkillRef { skill })
    }
}

fn require_monster(
    monsters: &BTreeMap<MonsterNumber, MonsterDefinition>,
    monster: MonsterNumber,
) -> Result<(), AtlasError> {
    if monsters.contains_key(&monster) {
        Ok(())
    } else {
        Err(AtlasError::UnknownMonsterRef { monster })
    }
}

fn require_item(
    items: &BTreeMap<ItemRef, ItemDefinition>,
    item: ItemRef,
) -> Result<(), AtlasError> {
    if items.contains_key(&item) {
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
    /// The class records do not form a complete, unique roster.
    ClassTable(ClassTableError),
    /// The experience curve is malformed.
    ExpCurve(ExpTableError),
    /// The ancient roster has an ambiguous membership.
    AncientRoster(AncientRosterError),
    /// The `exp_tables` file does not carry exactly one record.
    ExpTableNotSingle {
        /// The number of records found.
        found: usize,
    },
    /// The `game_config` file does not carry exactly one record.
    GameConfigNotSingle {
        /// The number of records found.
        found: usize,
    },
    /// A terrain sidecar names a map with no record.
    TerrainForUnknownMap {
        /// The unresolved map.
        map: MapNumber,
    },
    /// Two terrain sidecars describe the same map.
    DuplicateTerrain {
        /// The repeated map.
        map: MapNumber,
    },
    /// A map has no terrain sidecar.
    TerrainMissingForMap {
        /// The uncovered map.
        map: MapNumber,
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
            Self::ClassTable(err) => write!(f, "class table: {err}"),
            Self::ExpCurve(err) => write!(f, "experience curve: {err}"),
            Self::AncientRoster(err) => write!(f, "ancient roster: {err}"),
            Self::ExpTableNotSingle { found } => {
                write!(f, "expected exactly one exp table, found {found}")
            }
            Self::GameConfigNotSingle { found } => {
                write!(f, "expected exactly one game config, found {found}")
            }
            Self::TerrainForUnknownMap { map } => {
                write!(f, "terrain sidecar for unknown map {map:?}")
            }
            Self::DuplicateTerrain { map } => write!(f, "duplicate terrain for map {map:?}"),
            Self::TerrainMissingForMap { map } => write!(f, "map {map:?} has no terrain sidecar"),
        }
    }
}

impl core::error::Error for AtlasError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::ClassTable(err) => Some(err),
            Self::ExpCurve(err) => Some(err),
            Self::AncientRoster(err) => Some(err),
            Self::DuplicateMapNumber { .. }
            | Self::DuplicateGateNumber { .. }
            | Self::DuplicateWarpIndex { .. }
            | Self::DuplicateMonsterNumber { .. }
            | Self::DuplicateSkillNumber { .. }
            | Self::DuplicateItemRef { .. }
            | Self::GateOnUnknownMap { .. }
            | Self::EnterTargetsUnknownGate { .. }
            | Self::EnterTargetsEnterGate { .. }
            | Self::WarpTargetsUnknownGate { .. }
            | Self::WarpTargetsEnterGate { .. }
            | Self::FallbackSpawnGateMissing
            | Self::UnknownMapRef { .. }
            | Self::UnknownMonsterRef { .. }
            | Self::UnknownSkillRef { .. }
            | Self::UnknownItemRef { .. }
            | Self::ExpTableNotSingle { .. }
            | Self::GameConfigNotSingle { .. }
            | Self::TerrainForUnknownMap { .. }
            | Self::DuplicateTerrain { .. }
            | Self::TerrainMissingForMap { .. } => None,
        }
    }
}
