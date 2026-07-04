//! Cross-checked, resolved view over the entire static dataset, built once at
//! load. `Atlas::parse` is the single referential-integrity proof for every v2
//! file: per-file identity uniqueness plus resolution of every declared
//! cross-file reference, in one pass — and it *keeps* the proven-unique records
//! as total by-id lookups, so a consuming service reaches any definition
//! through the Atlas without re-scanning a raw `Vec`. Every accessor downstream
//! is total or genuinely optional.

mod check;
mod resolve;
mod views;

use std::collections::{BTreeMap, BTreeSet};

use crate::components::spatial::WorldPos;
use crate::components::tile::WalkGrid;
use crate::data::ancient_sets::{AncientRoster, AncientRosterError, AncientSet};
use crate::data::box_drops::BoxDrop;
use crate::data::chaos_mixes::ChaosMix;
use crate::data::classes::{ClassRecord, ClassTable, ClassTableError};
use crate::data::common::{DataFile, GateNumber, ItemRef, MapNumber, MonsterNumber, SkillNumber};
use crate::data::drop_config::DropConfig;
use crate::data::exp_tables::{ExpCurve, ExpTable, ExpTableError};
use crate::data::game_config::{GameConfig, ProgressionConfig};
use crate::data::gates_warps::{GateWarpRecord, SpawnGate, Warp, WarpIndex};
use crate::data::item_definitions::ItemDefinition;
use crate::data::map_definitions::MapDefinition;
use crate::data::monster_definitions::MonsterDefinition;
use crate::data::skills::Skill;
use crate::data::spawns::Spawn;
use crate::data::special_drops::SpecialDropRecord;
use crate::data::terrain::MapTerrain;

pub use crate::data::drop_pool::DropPool;
pub use views::{
    EnterGateView, Landing, MapHandle, ResolvedOutput, ResolvedRecipe, SpawnEntry, WarpView,
};

use check::{
    check_ancient_sets, check_box_drops, check_chaos_mixes, check_classes, check_items,
    check_monster_attacks, check_special_drops, check_summons,
};
use resolve::{
    GatePartition, index_items, index_maps, index_monsters, index_skills, index_terrain,
    resolve_chaos_recipes, resolve_enter_gates, resolve_spawns, resolve_warps, take_single,
};
use views::{ResolvedEnterGate, ResolvedSpawn};

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
    progression: ProgressionConfig,
    special_drops: Vec<SpecialDropRecord>,
    box_drops: Vec<BoxDrop>,
    drop_pool: DropPool,
    chaos_recipes: Vec<ResolvedRecipe>,
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
        let chaos_recipes = resolve_chaos_recipes(data.chaos_mixes.records, &items)?;
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
        let progression = game_config.progression;
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
            progression,
            special_drops: data.special_drops.records,
            box_drops: data.box_drops.records,
            drop_pool,
            chaos_recipes,
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
    /// edge (monster attack, item skill/teaches, summon) is proven present
    /// by `parse`.
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

    /// The party and experience-award facts (including the per-kill experience
    /// jitter band).
    #[must_use]
    pub fn progression(&self) -> ProgressionConfig {
        self.progression
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

    /// The chaos-recipe catalog, definition-joined at parse, in descending
    /// authentic crafting-number scan order — the order the mix service
    /// attempts recipes in.
    pub fn chaos_recipes(&self) -> impl Iterator<Item = &ResolvedRecipe> {
        self.chaos_recipes.iter()
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
