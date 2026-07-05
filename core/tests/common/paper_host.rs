//! The end-to-end "paper host": one owned world value that drives the real
//! mu-core services exactly as a future SpacetimeDB host would, forcing a serde
//! round-trip between every service call.
//!
//! It is a deep module — it hides two things behind a small drive interface:
//! the persist round-trip (the [`persist`] seam, standing in for the database
//! write/read boundary) and the ordered live sets (identity is positional, an
//! index into a `Vec`, never a host id). A scenario reads live values out of
//! the world, hands them to a pure service with the world's held [`Atlas`] and
//! seeded [`TestRng`], and the driver writes every returned live value back
//! *through* [`persist`] before storing it — so the world only ever holds
//! values that have survived the boundary.
//!
//! This module is included with `#[path]` by the scenario binary rather than
//! re-exported from [`super`]: the movement suite that consumes `common` uses
//! none of the paper host, and under `-D warnings` its unused public surface
//! would be dead code there. The two shared leaves it needs — the dataset
//! loader and the single `SplitMix64` stream — it includes the same way, so no
//! algorithm is copied and no unused code crosses a binary boundary.

#[path = "dataset.rs"]
mod dataset;
#[path = "rng.rs"]
mod rng;

use serde::Serialize;
use serde::de::DeserializeOwned;

use mu_core::components::active_effect::{ActiveEffect, ActiveEffects};
use mu_core::components::combat_profile::CombatTarget;
use mu_core::components::element::PerElement;
use mu_core::components::equipment::{Equipment, EquipmentSlot};
use mu_core::components::inventory::{Cell, Footprint, Inventory};
use mu_core::components::item_instance::{
    CraftedAugment, Durability, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
};
use mu_core::components::item_quality::ItemRarity;
use mu_core::components::item_ref::ItemRef;
use mu_core::components::movement::{CombatLock, FlightChange, Movement, Wings};
use mu_core::components::placement::Placement;
use mu_core::components::pool::Pool;
use mu_core::components::spatial::{Facing, Fixed, UNITS_PER_TILE, WorldPos};
use mu_core::components::tile::{TileCoord, WalkGrid};
use mu_core::components::trade_window::Side;
use mu_core::components::units::{
    CarriedZen, Exp, ItemLevel, MapNumber, Resistance, Tick, TickDuration, Zen,
};
use mu_core::data::atlas::Atlas;
use mu_core::data::common::{MonsterNumber, SkillNumber};
use mu_core::data::effects::Ailment;
use mu_core::data::item_definitions::ItemKind;
use mu_core::data::monster_definitions::{MonsterCombat, MonsterRole};
use mu_core::data::npc_shops::ShelfSlot;
use mu_core::data::option_roll::OptionRollPolicy;
use mu_core::data::skills::AreaPattern;
use mu_core::data::spawns::SpawnPlacement;
use mu_core::entities::character::Character;
use mu_core::entities::monster_instance::MonsterInstance;
use mu_core::entities::trade_session::TradeSession;
use mu_core::entities::world_item::WorldItem;
use mu_core::entities::world_zen::WorldZen;
use mu_core::events::combat::AttackOutcome;
use mu_core::events::craft::MixOutcome;
use mu_core::events::effect::{BuffCastOutcome, EffectEvent};
use mu_core::events::inventory::{EquipOutcome, EquipRejection, PlaceOutcome, RemoveOutcome};
use mu_core::events::kill::KillResolution;
use mu_core::events::monster_ai::MonsterIntent;
use mu_core::events::movement::{FlightOutcome, StepOutcome};
use mu_core::events::progression::LevelUp;
use mu_core::events::shop::{BuyOutcome, SellOutcome};
use mu_core::events::skills::{SkillOutcome, TargetHit};
use mu_core::events::trade::{CancelReason, OfferOutcome, Settlement, ZenOfferOutcome};
use mu_core::services::combat::resolve_attack;
use mu_core::services::effects::{
    ApplicableBuff, advance_effects, apply_ailment, apply_buff, mobility,
};
use mu_core::services::inventory::{
    PickupOutcome, PlaceIntent, ZenPickupOutcome, equip, place_item, remove_item,
};
use mu_core::services::item_roll::roll_dropped_item;
use mu_core::services::kill::resolve_kill;
use mu_core::services::monster_ai::decide_monster_action;
use mu_core::services::movement::resolve_step;
use mu_core::services::profile::{character_profile, monster_profile};
use mu_core::services::skills::{DamagingSkill, SkillRouting, cast, cast_heal, route};
use mu_core::services::spawn::{SpawnResult, place_spawn};
use mu_core::services::trade::{
    AcceptOutcome, Holdings, LockResult, RequestOutcome, TradeAvailability, accept, cancel, lock,
    offer_item, offer_zen, request,
};

pub use dataset::or_abort;
use dataset::{real_atlas, real_static_data};
use rng::TestRng;

/// The one owned world value: the held static [`Atlas`], one seeded stream, the
/// current map context, and the ordered live sets. Every field except the
/// atlas, the stream, and the map is `serde`-persisted state; identity is the
/// index into each `Vec`, never a host id.
pub struct World {
    /// The static data index, rebuilt from source and held by value — the one
    /// carve-out from the persist seam (it never round-trips).
    atlas: Atlas,
    /// The drop-time option-roll policy, loaded from `game_config` alongside the
    /// atlas and held by value: it is static data the host loads (the atlas does
    /// not retain it), so like the atlas it never round-trips.
    drop_policy: OptionRollPolicy,
    /// The single seeded stream threaded through every randomised call.
    rng: TestRng,
    /// The current map; supplies the `map` of every ground drop the host lays.
    map: MapNumber,
    /// The live characters, addressed by index.
    characters: Vec<Character>,
    /// The live per-character bags, addressed by the same index as `characters`
    /// (a `Character` holds no inventory — it is a parallel live set the host
    /// keys by the same identity).
    inventories: Vec<Inventory>,
    /// The live per-character worn sets, addressed by the same index as
    /// `characters` (a `Character` holds no equipment either).
    equipment: Vec<Equipment>,
    /// The live monster instances, addressed by index (many share a number).
    monsters: Vec<MonsterInstance>,
    /// The items lying on the ground, addressed by index.
    ground_items: Vec<WorldItem>,
    /// The money piles lying on the ground, addressed by index.
    ground_zen: Vec<WorldZen>,
    /// The open trade sessions, addressed by index.
    sessions: Vec<TradeSession>,
}

/// The persist seam — the database write/read boundary abstracted. Serialises a
/// live value and reads it straight back, re-proving every invariant on load.
/// Infallible over the harness's own values (they are built from valid data),
/// so the two `Result`s resolve through [`or_abort`] rather than a banned
/// suppressor.
#[must_use]
pub fn persist<T: Serialize + DeserializeOwned>(value: T) -> T {
    let wire = or_abort(serde_json::to_string(&value));
    // The seam consumes the pre-persist value: the host no longer holds it,
    // only the copy read back from the boundary survives.
    drop(value);
    or_abort(serde_json::from_str(&wire))
}

/// The canonical wire string of a value — one half of the persist seam, for the
/// replay trace to record a step's outcome by its serialized form (the harness's
/// re-serialized-string identity idiom). Infallible over harness values, so the
/// `Result` resolves through [`or_abort`] rather than a banned suppressor — which
/// is why a scenario's composed-run driver can collect the trace without a test's
/// `unwrap`.
#[must_use]
pub fn wire<T: Serialize>(value: &T) -> String {
    or_abort(serde_json::to_string(value))
}

/// The live sets alone, serialised for a snapshot: the atlas, the stream, and
/// the (fixed-per-run) map are all excluded, so a snapshot is total over the
/// observable persisted state and nothing else.
#[derive(Serialize)]
struct LiveSnapshot<'a> {
    characters: &'a [Character],
    inventories: &'a [Inventory],
    equipment: &'a [Equipment],
    monsters: &'a [MonsterInstance],
    ground_items: &'a [WorldItem],
    ground_zen: &'a [WorldZen],
    sessions: &'a [TradeSession],
}

impl World {
    /// A fresh world on `map`, holding the real parsed [`Atlas`] and one stream
    /// seeded by `seed`, with every live set empty.
    #[must_use]
    pub fn new(seed: u64, map: MapNumber) -> Self {
        // The parsed atlas keeps every cross-checked record but drops the
        // drop-time option policy (it is not retained on the resolved index), so
        // the policy is lifted from a raw dataset load — the one static value the
        // atlas cannot supply. Both are static data the host loads once; neither
        // round-trips.
        let atlas = real_atlas();
        let drop_policy = or_abort(
            real_static_data()
                .game_config
                .records
                .first()
                .ok_or("empty game config"),
        )
        .option_roll
        .clone();
        Self {
            atlas,
            drop_policy,
            rng: TestRng::new(seed),
            map,
            characters: Vec::new(),
            inventories: Vec::new(),
            equipment: Vec::new(),
            monsters: Vec::new(),
            ground_items: Vec::new(),
            ground_zen: Vec::new(),
            sessions: Vec::new(),
        }
    }

    /// The held static data index — passed to services, never persisted.
    #[must_use]
    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    /// The character at `index`.
    #[must_use]
    pub fn character(&self, index: usize) -> &Character {
        or_abort(self.characters.get(index).ok_or("no character at index"))
    }

    /// The bag of the character at `index`.
    #[must_use]
    pub fn inventory(&self, index: usize) -> &Inventory {
        or_abort(self.inventories.get(index).ok_or("no inventory at index"))
    }

    /// The worn set of the character at `index`.
    #[must_use]
    pub fn equipment(&self, index: usize) -> &Equipment {
        or_abort(self.equipment.get(index).ok_or("no equipment at index"))
    }

    /// The monster instance at `index`.
    #[must_use]
    pub fn monster(&self, index: usize) -> MonsterInstance {
        *or_abort(self.monsters.get(index).ok_or("no monster at index"))
    }

    /// The ground item at `index`.
    #[must_use]
    pub fn ground_item(&self, index: usize) -> &WorldItem {
        or_abort(
            self.ground_items
                .get(index)
                .ok_or("no ground item at index"),
        )
    }

    /// How many items lie on the ground — proves a picked-up item left the set
    /// (it exists in exactly one place) and a rejected one stayed put.
    #[must_use]
    pub fn ground_item_count(&self) -> usize {
        self.ground_items.len()
    }

    /// The ground zen pile at `index`.
    #[must_use]
    pub fn ground_zen(&self, index: usize) -> &WorldZen {
        or_abort(self.ground_zen.get(index).ok_or("no ground zen at index"))
    }

    /// The trade session at `index`.
    #[must_use]
    pub fn session(&self, index: usize) -> &TradeSession {
        or_abort(self.sessions.get(index).ok_or("no session at index"))
    }

    /// Seats a character through the persist seam and returns its index. Its
    /// parallel bag (an empty main-inventory grid) and worn set (fully
    /// unequipped) are seated at the same index through the same seam, so the
    /// three live sets stay aligned by identity.
    pub fn seat_character(&mut self, character: Character) -> usize {
        let index = self.characters.len();
        self.characters.push(persist(character));
        self.inventories.push(persist(bag()));
        self.equipment.push(persist(Equipment::empty()));
        index
    }

    /// Seats a monster instance through the persist seam and returns its index.
    pub fn seat_monster(&mut self, monster: MonsterInstance) -> usize {
        let index = self.monsters.len();
        self.monsters.push(persist(monster));
        index
    }

    /// Lays an item on the ground at `position` with despawn tick `despawn`,
    /// synthesising its `map` from the world's current context (the ground-drop
    /// map a host must supply), seats it through the persist seam, and returns
    /// its index.
    pub fn seat_ground_item(
        &mut self,
        instance: ItemInstance,
        position: WorldPos,
        despawn: Tick,
    ) -> usize {
        let index = self.ground_items.len();
        let world_item = WorldItem {
            instance,
            position,
            map: self.map,
            despawn,
        };
        self.ground_items.push(persist(world_item));
        index
    }

    /// Lays a money pile on the ground at `position` with despawn tick
    /// `despawn`, synthesising its `map` from the world's current context, seats
    /// it through the persist seam, and returns its index.
    pub fn seat_ground_zen(&mut self, amount: Zen, position: WorldPos, despawn: Tick) -> usize {
        let index = self.ground_zen.len();
        let world_zen = WorldZen {
            amount,
            position,
            map: self.map,
            despawn,
        };
        self.ground_zen.push(persist(world_zen));
        index
    }

    /// Seats a trade session through the persist seam and returns its index.
    pub fn seat_session(&mut self, session: TradeSession) -> usize {
        let index = self.sessions.len();
        self.sessions.push(persist(session));
        index
    }

    /// Drives one physical strike from the character at `attacker_index` onto
    /// the monster at `target_index`: derives both combat profiles from held
    /// state, resolves the strike over the world's stream, writes the returned
    /// health back onto the monster *through* the persist seam, and returns the
    /// outcome for the scenario to assert on.
    pub fn strike(&mut self, attacker_index: usize, target_index: usize) -> AttackOutcome {
        let attacker = or_abort(self.characters.get(attacker_index).ok_or("no attacker"));
        let (attacker_profile, _maxima) = character_profile(attacker);

        let monster = *or_abort(self.monsters.get(target_index).ok_or("no target"));
        let def = or_abort(
            self.atlas
                .monster(monster.number)
                .ok_or("unknown monster def"),
        );
        let target_profile = match &def.role {
            MonsterRole::Monster {
                combat,
                resistances,
                ..
            }
            | MonsterRole::Guard {
                combat,
                resistances,
                ..
            }
            | MonsterRole::Trap {
                combat,
                resistances,
                ..
            } => monster_profile(combat, resistances, combat.level),
            MonsterRole::Npc { .. } | MonsterRole::SoccerBall => {
                return or_abort(Err::<AttackOutcome, _>(
                    "strike was handed a non-combat monster",
                ));
            }
        };

        let (new_health, outcome) = resolve_attack(
            &attacker_profile,
            &target_profile,
            monster.health,
            &mut self.rng,
        );

        let mut updated = monster;
        updated.health = new_health;
        let persisted = persist(updated);
        let slot = or_abort(self.monsters.get_mut(target_index).ok_or("no target slot"));
        *slot = persisted;
        outcome
    }

    /// Resolves one spawn record for monster `number` on the world's current map
    /// (seam 6): looks up the definition and the map's walk grid from the held
    /// atlas, runs [`place_spawn`] over the world's stream, and hands the
    /// `SpawnResult` back for the scenario to seat and correlate. The returned
    /// aggregate's positional pairing to its event is the delivery key (V8) — the
    /// harness never invents an id.
    pub fn spawn_from(&mut self, number: MonsterNumber, placement: SpawnPlacement) -> SpawnResult {
        let def = or_abort(self.atlas.monster(number).ok_or("unknown monster number"));
        let grid = or_abort(self.atlas.walk_grid(self.map).ok_or("no walk grid for map"));
        place_spawn(def, &placement, grid, self.map, &mut self.rng)
    }

    /// Advances the monster at `index` by one AI decision (seam 6/7): derives its
    /// mobility from its own effect store, looks up its behavior and the map's
    /// grid from the atlas, runs [`decide_monster_action`] over the world's
    /// stream at `now`, writes the advanced instance back *through* the persist
    /// seam, and returns the chosen intent. The mob acts on a fixed 50 ms tick —
    /// the host owns the clock (U1), fed here as a constant cadence.
    pub fn advance_monster(
        &mut self,
        index: usize,
        target: Option<WorldPos>,
        now: Tick,
    ) -> MonsterIntent {
        let mob = self.monster(index);
        let def = or_abort(
            self.atlas
                .monster(mob.number)
                .ok_or("unknown monster number"),
        );
        let behavior = match &def.role {
            MonsterRole::Monster { behavior, .. }
            | MonsterRole::Guard { behavior, .. }
            | MonsterRole::Trap { behavior, .. } => *behavior,
            MonsterRole::Npc { .. } | MonsterRole::SoccerBall => {
                return or_abort(Err::<MonsterIntent, _>(
                    "advance handed a non-combat monster",
                ));
            }
        };
        let grid = or_abort(
            self.atlas
                .walk_grid(mob.placement.map)
                .ok_or("no walk grid"),
        );
        let capability = mobility(&mob.active_effects);
        let tick = host_tick();
        let (advanced, intent) = decide_monster_action(
            &mob,
            &behavior,
            target,
            now,
            tick,
            grid,
            capability,
            &mut self.rng,
        );
        let persisted = persist(advanced);
        let slot = or_abort(self.monsters.get_mut(index).ok_or("no monster slot"));
        *slot = persisted;
        intent
    }

    /// Resolves the kill reward for the killer at `killer_index` over the victim
    /// at `victim_index` (seam 1/3, V3a): the `Killed` outcome the strike loop
    /// reached is routed here, composing exp + level-ups + drops from values
    /// already in hand. The `KillResolution` is an outcome the host delivers, not
    /// live state, so it does not re-enter the world through persist.
    pub fn resolve_kill_of(&mut self, killer_index: usize, victim_index: usize) -> KillResolution {
        let killer = or_abort(self.characters.get(killer_index).ok_or("no killer"));
        let victim = *or_abort(self.monsters.get(victim_index).ok_or("no victim"));
        resolve_kill(killer, &victim, &self.atlas, &mut self.rng)
    }

    /// Materialises a `Drop::Item` as a ground item (seam 1, V1/V7): rolls the
    /// decided `{item, level, rarity}` into a full instance over the world's
    /// stream and held policy, then lays it at `position` on the world's current
    /// map with a host-chosen `despawn` tick — the position comes from the
    /// victim's placement (available), the map from context (available), the
    /// despawn from host policy (no core source — spec §5.2/V7). Returns the
    /// ground index and the rolled instance for byte-identity checks.
    pub fn drop_item_to_ground(
        &mut self,
        item: ItemRef,
        level: ItemLevel,
        rarity: ItemRarity,
        position: WorldPos,
        despawn: Tick,
    ) -> (usize, ItemInstance) {
        let def = or_abort(self.atlas.item(item).ok_or("unknown dropped item"));
        let rolled = roll_dropped_item(def, level, rarity, &self.drop_policy, &mut self.rng);
        let index = self.seat_ground_item(rolled.clone(), position, despawn);
        (index, rolled)
    }

    /// Places `item` into the bag of the character at `char_index` at `anchor`
    /// with `footprint` (seam 1 setup): folds [`place_item`] and writes the new
    /// bag back *through* the persist seam. On rejection the bag is unchanged and
    /// the bounced item rides the returned outcome.
    pub fn place_in_bag(
        &mut self,
        char_index: usize,
        item: ItemInstance,
        footprint: Footprint,
        anchor: Cell,
    ) -> PlaceOutcome {
        let inventory = or_abort(self.inventories.get(char_index).ok_or("no bag")).clone();
        let (new_inventory, outcome) = place_item(
            inventory,
            PlaceIntent {
                anchor,
                footprint,
                item,
            },
        );
        let persisted = persist(new_inventory);
        let slot = or_abort(self.inventories.get_mut(char_index).ok_or("no bag slot"));
        *slot = persisted;
        outcome
    }

    /// Picks the ground item at `ground_index` into the bag of the character at
    /// `char_index` at `anchor` (seam 1, V2): the footprint is the host's atlas
    /// lookup (I1), the reach gate is host-owned (V2). On `PickedUp` the world
    /// item is consumed — removed from the ground set — and the new bag is
    /// written back through the seam; on `Rejected` the ground set is untouched
    /// and the untouched world item is reassembled in the outcome (move-only —
    /// nothing double-credited). Returns the outcome.
    pub fn pickup(
        &mut self,
        char_index: usize,
        ground_index: usize,
        anchor: Cell,
    ) -> PickupOutcome {
        let world_item =
            or_abort(self.ground_items.get(ground_index).ok_or("no ground item")).clone();
        let footprint = footprint_of(&self.atlas, world_item.instance.item);
        let inventory = or_abort(self.inventories.get(char_index).ok_or("no bag")).clone();
        let (new_inventory, outcome) =
            mu_core::services::inventory::pickup(world_item, inventory, anchor, footprint);
        match &outcome {
            PickupOutcome::PickedUp { .. } => {
                let persisted = persist(new_inventory);
                let slot = or_abort(self.inventories.get_mut(char_index).ok_or("no bag slot"));
                *slot = persisted;
                self.ground_items.remove(ground_index);
            }
            PickupOutcome::Rejected { .. } => {
                // The bag is unchanged and the ground item stays put; the
                // reassembled world item rides the outcome for the caller.
                drop(new_inventory);
            }
        }
        outcome
    }

    /// Removes the item covering `cell` from the bag of the character at
    /// `char_index` (seam 1): folds [`remove_item`], writes the new bag back
    /// through the seam, and hands the removed item out in the outcome — the
    /// move a host makes to take a bagged item into hand before equipping it.
    pub fn remove_from_bag(&mut self, char_index: usize, cell: Cell) -> RemoveOutcome {
        let inventory = or_abort(self.inventories.get(char_index).ok_or("no bag")).clone();
        let (new_inventory, outcome) = remove_item(inventory, cell);
        let persisted = persist(new_inventory);
        let slot = or_abort(self.inventories.get_mut(char_index).ok_or("no bag slot"));
        *slot = persisted;
        outcome
    }

    /// Equips `item` onto the character at `char_index`, into the first slot the
    /// equip service accepts it in (seam 1): the core [`equip`] service is the
    /// slot oracle — a host auto-equip tries slots and keeps the one that takes.
    /// On success the new worn set is written back through the persist seam; a
    /// non-equippable item is handed back in the returned rejection.
    pub fn equip_first_available(&mut self, char_index: usize, item: ItemInstance) -> EquipOutcome {
        let worn = or_abort(self.equipment.get(char_index).ok_or("no worn set")).clone();
        let def = or_abort(self.atlas.item(item.item).ok_or("unknown item to equip"));
        let (new_worn, outcome) = equip_into_first_slot(worn, item, &def.kind, &self.atlas);
        let persisted = persist(new_worn);
        let slot = or_abort(self.equipment.get_mut(char_index).ok_or("no worn slot"));
        *slot = persisted;
        outcome
    }

    /// Equips `item` onto the character at `char_index` into a NAMED `slot`
    /// (seam 1): the core [`equip`] service decides — accepting the item, or
    /// rejecting it with the reason its kind and the slot's occupancy dictate
    /// ([`EquipRejection::IncompatibleSlot`], [`EquipRejection::SlotOccupied`], or
    /// [`EquipRejection::TwoHandedConflict`]). On a rejection the worn set is
    /// returned unchanged and the bounced item rides the outcome — the failure
    /// branch [`Self::equip_first_available`] cannot reach, since that oracle only
    /// ever surfaces `IncompatibleSlot`. The returned worn set is written back
    /// *through* the persist seam either way.
    ///
    /// [`EquipRejection::IncompatibleSlot`]: mu_core::events::inventory::EquipRejection::IncompatibleSlot
    /// [`EquipRejection::SlotOccupied`]: mu_core::events::inventory::EquipRejection::SlotOccupied
    /// [`EquipRejection::TwoHandedConflict`]: mu_core::events::inventory::EquipRejection::TwoHandedConflict
    pub fn equip_into(
        &mut self,
        char_index: usize,
        item: ItemInstance,
        slot: EquipmentSlot,
    ) -> EquipOutcome {
        let worn = or_abort(self.equipment.get(char_index).ok_or("no worn set")).clone();
        let def = or_abort(self.atlas.item(item.item).ok_or("unknown item to equip"));
        let (new_worn, outcome) = equip(worn, item, &def.kind, slot, &self.atlas);
        let persisted = persist(new_worn);
        let worn_slot = or_abort(self.equipment.get_mut(char_index).ok_or("no worn slot"));
        *worn_slot = persisted;
        outcome
    }

    /// Casts the damaging `skill` from the caster at `caster_index`, aimed at
    /// `aim`, over the batch of monsters at `target_indices` (seam 9, V4 twin for
    /// offence): routes the skill from the held atlas (aborting on a non-damaging
    /// one), derives the caster's own combat profile and one [`CombatTarget`] per
    /// batch monster from held state, resolves [`cast`] over the map's grid and the
    /// world's stream, persists the caster's spent vitals (K1), then writes each
    /// struck target's returned health, effects, and any knockback displacement
    /// back onto its monster *through* the persist seam — mapping each hit's
    /// batch-position `target_index` to the monster index the caller supplied.
    /// Returns the [`SkillOutcome`]. A rejection spends nothing and touches no
    /// target.
    pub fn cast_damaging(
        &mut self,
        caster_index: usize,
        skill: SkillNumber,
        aim: WorldPos,
        target_indices: &[usize],
    ) -> SkillOutcome {
        let (spent_vitals, outcome) = {
            let skill_def = or_abort(self.atlas.skill(skill).ok_or("unknown skill"));
            let damaging = match route(skill_def) {
                SkillRouting::Damaging(reference) => reference,
                SkillRouting::Heal(_) | SkillRouting::Buff(_) | SkillRouting::Deferred => {
                    return or_abort(Err::<SkillOutcome, _>("skill is not a damaging skill"));
                }
            };
            let mut targets = Vec::with_capacity(target_indices.len());
            for &index in target_indices {
                let mob = *or_abort(self.monsters.get(index).ok_or("no target monster"));
                let def = or_abort(self.atlas.monster(mob.number).ok_or("unknown monster def"));
                let profile = match &def.role {
                    MonsterRole::Monster {
                        combat,
                        resistances,
                        ..
                    }
                    | MonsterRole::Guard {
                        combat,
                        resistances,
                        ..
                    }
                    | MonsterRole::Trap {
                        combat,
                        resistances,
                        ..
                    } => monster_profile(combat, resistances, combat.level),
                    MonsterRole::Npc { .. } | MonsterRole::SoccerBall => {
                        return or_abort(Err::<SkillOutcome, _>(
                            "cast was handed a non-combat monster",
                        ));
                    }
                };
                targets.push(CombatTarget::new(
                    profile,
                    mob.health,
                    mob.placement,
                    mob.active_effects,
                ));
            }
            let caster = or_abort(self.characters.get(caster_index).ok_or("no caster"));
            let grid = or_abort(
                self.atlas
                    .walk_grid(caster.placement().map)
                    .ok_or("no walk grid"),
            );
            cast(caster, damaging, aim, &targets, grid, &mut self.rng)
        };
        let vitals_value = or_abort(serde_json::to_value(spent_vitals));
        self.persist_character_with(caster_index, "vitals", vitals_value);
        if let SkillOutcome::Cast { hits, .. } = &outcome {
            for hit in hits {
                self.write_back_target_hit(target_indices, hit);
            }
        }
        outcome
    }

    /// Writes one struck target's [`TargetHit`] back onto its monster: maps the
    /// hit's batch-position `target_index` to the monster index the caster passed,
    /// sets the returned health and effects, applies any knockback displacement,
    /// and persists the updated instance through the seam.
    fn write_back_target_hit(&mut self, target_indices: &[usize], hit: &TargetHit) {
        let (batch_index, health, active_effects, displacement) = match hit {
            TargetHit::Missed {
                target_index,
                health,
                active_effects,
            }
            | TargetHit::Killed {
                target_index,
                health,
                active_effects,
                ..
            } => (*target_index, *health, *active_effects, None),
            TargetHit::Landed {
                target_index,
                health,
                active_effects,
                displacement,
                ..
            } => (*target_index, *health, *active_effects, *displacement),
        };
        let monster_index = *or_abort(
            target_indices
                .get(batch_index)
                .ok_or("hit target index outside the batch"),
        );
        let mut updated = *or_abort(
            self.monsters
                .get(monster_index)
                .ok_or("no monster at index"),
        );
        updated.health = health;
        updated.active_effects = active_effects;
        if let Some(placement) = displacement {
            updated.placement = placement;
        }
        let persisted = persist(updated);
        let slot = or_abort(
            self.monsters
                .get_mut(monster_index)
                .ok_or("no monster slot"),
        );
        *slot = persisted;
    }

    /// Applies a kill's `gained` experience and `level_ups` to the character at
    /// `char_index` by editing its wire form and re-loading it — the only path a
    /// `Character` mutates (serde-only), which re-proves the class↔stats gate on
    /// the way in.
    ///
    /// This method IS the R1 missing-port finding made executable (spec §0.3,
    /// §5.2): no core service applies a `LevelUp`. It host-invents two rules — the
    /// `points_per_level × level_ups` stat-point grant (read from the atlas class
    /// table) AND the refill-vitals-to-max rule (re-derived from the grown
    /// character's class-formula `VitalMaxima`). Neither is expressed by any
    /// returned value or blessed by any doc; the harness drives the growth seam
    /// so the fight-feedback assertion can run, but this is NOT clean host-policy.
    pub fn apply_growth(&mut self, char_index: usize, gained: Exp, level_ups: &[LevelUp]) {
        let character = or_abort(self.characters.get(char_index).ok_or("no character"));
        let class = character.class();
        let base_level = character.level();
        let base_points = character.unspent_points();
        let base_exp = character.experience();
        let mut wire = or_abort(serde_json::to_value(character));

        let new_level = match level_ups.iter().map(|level_up| level_up.level).max() {
            Some(level) => level,
            None => base_level,
        };
        let per_level = self.atlas.classes().record(class).points_per_level;
        let crossings = or_abort(u16::try_from(level_ups.len()));
        let granted = u16::from(per_level).saturating_mul(crossings);
        let new_points = base_points.saturating_add(granted);
        let new_exp = Exp(base_exp.0.saturating_add(gained.0));

        {
            let object = or_abort(wire.as_object_mut().ok_or("character is not an object"));
            object.insert("experience".to_owned(), serde_json::json!(new_exp.0));
            object.insert("level".to_owned(), serde_json::json!(new_level.get()));
            object.insert("unspent_points".to_owned(), serde_json::json!(new_points));
        }

        // Re-derive the class-formula maxima on the grown character, then seat the
        // vitals full at them — the host-invented refill rule.
        let grown: Character = or_abort(serde_json::from_value(wire.clone()));
        let (_, maxima) = character_profile(&grown);
        {
            let object = or_abort(wire.as_object_mut().ok_or("character is not an object"));
            object.insert(
                "vitals".to_owned(),
                serde_json::json!({
                    "health": {"current": maxima.max_health, "max": maxima.max_health},
                    "mana": {"current": maxima.max_mana, "max": maxima.max_mana},
                    "ability": {"current": maxima.max_ability, "max": maxima.max_ability},
                }),
            );
        }
        let final_character: Character = or_abort(serde_json::from_value(wire));
        let persisted = persist(final_character);
        let slot = or_abort(
            self.characters
                .get_mut(char_index)
                .ok_or("no character slot"),
        );
        *slot = persisted;
    }

    /// Drives one monster strike onto the player (seam 7, V6): the forwarded
    /// `MonsterIntent::Attack` is resolved with the monster's profile as attacker,
    /// the player's `character_profile` as target, and the player's current health
    /// `Pool` as `target_health` — the three inputs V6 pins. The returned health
    /// is written back onto the player by serde-editing its `vitals.health` (the
    /// reverse-direction twin of the growth writeback), and the outcome is
    /// returned. `resolve_attack` is symmetric: the same service that kills
    /// monsters kills a player, only the profiles swapped.
    pub fn player_struck_by_monster(
        &mut self,
        player_index: usize,
        monster_index: usize,
    ) -> AttackOutcome {
        let player = or_abort(self.characters.get(player_index).ok_or("no player"));
        let (target_profile, _maxima) = character_profile(player);
        let target_health = player.vitals().health;

        let monster = *or_abort(self.monsters.get(monster_index).ok_or("no attacker"));
        let def = or_abort(self.atlas.monster(monster.number).ok_or("unknown attacker"));
        let attacker_profile = match &def.role {
            MonsterRole::Monster {
                combat,
                resistances,
                ..
            }
            | MonsterRole::Guard {
                combat,
                resistances,
                ..
            }
            | MonsterRole::Trap {
                combat,
                resistances,
                ..
            } => monster_profile(combat, resistances, combat.level),
            MonsterRole::Npc { .. } | MonsterRole::SoccerBall => {
                return or_abort(Err::<AttackOutcome, _>(
                    "a non-combat monster cannot strike",
                ));
            }
        };

        let (new_health, outcome) = resolve_attack(
            &attacker_profile,
            &target_profile,
            target_health,
            &mut self.rng,
        );

        self.set_health(player_index, new_health);
        outcome
    }

    /// Sets the health pool of the character at `char_index` by swapping its
    /// `vitals.health` and writing the whole vitals block back through the
    /// serde-only [`Self::persist_character_with`] seam. This is the persist seam
    /// in the load direction for health (the twin of [`Self::set_wallet`]): a host
    /// loading a saved character restores its health, and every service that
    /// returns a new health [`Pool`] — a strike, a poison tick, a heal
    /// reconstruction — writes it back through here.
    pub fn set_health(&mut self, char_index: usize, health: Pool) {
        let mut vitals = self.character(char_index).vitals();
        vitals.health = health;
        let value = or_abort(serde_json::to_value(vitals));
        self.persist_character_with(char_index, "vitals", value);
    }

    /// Applies `ailment` to the character at `char_index` (seam 9): resolves its
    /// magnitude from `caster_energy` (poison scales off the caster) and its
    /// schedule from `now` on the host's fixed tick cadence, then writes the new
    /// store back onto the character by serde-editing its `active_effects` field —
    /// the persist seam in the effects direction (effects live inside their
    /// owner). Returns the resolved [`ActiveEffect`], the payload a host wraps in
    /// an `EffectApplied` delivery event.
    pub fn apply_ailment_to(
        &mut self,
        char_index: usize,
        ailment: Ailment,
        caster_energy: u16,
        now: Tick,
    ) -> ActiveEffect {
        let existing = self.character(char_index).active_effects();
        let (updated, effect) = apply_ailment(ailment, caster_energy, existing, now, host_tick());
        let value = or_abort(serde_json::to_value(updated));
        self.persist_character_with(char_index, "active_effects", value);
        effect
    }

    /// Applies `buff` to the character at `char_index` (seam 9): resolves its
    /// energy-scaled magnitude and absolute expiry, writes the new store back onto
    /// the character's `active_effects` through the persist seam, and returns the
    /// resolved [`ActiveEffect`]. The twin of [`Self::apply_ailment_to`] for a
    /// beneficial effect.
    pub fn apply_buff_to(
        &mut self,
        char_index: usize,
        buff: ApplicableBuff,
        caster_energy: u16,
        now: Tick,
    ) -> ActiveEffect {
        let existing = self.character(char_index).active_effects();
        let (updated, effect) = apply_buff(buff, caster_energy, existing, now, host_tick());
        let value = or_abort(serde_json::to_value(updated));
        self.persist_character_with(char_index, "active_effects", value);
        effect
    }

    /// Advances the timed-effect store of the character at `char_index` to `now`
    /// (seam 9, E3): reads the character's own effect store and health, ticks
    /// [`advance_effects`] (expiries + poison damage-over-time), and writes BOTH
    /// the advanced store and the drained health back through the persist seam.
    /// Returns the effect events. A lethal poison tick clears the store and zeroes
    /// health here — but there is NO reward pathway for it (spec §5.2, V3b): a
    /// `PoisonKilled` event awards no exp and no drops, so the harness reports the
    /// event and stops.
    pub fn advance_effects_on(&mut self, char_index: usize, now: Tick) -> Vec<EffectEvent> {
        let effects = self.character(char_index).active_effects();
        let health = self.character(char_index).vitals().health;
        let (new_effects, new_health, events) = advance_effects(effects, health, now);
        let value = or_abort(serde_json::to_value(new_effects));
        self.persist_character_with(char_index, "active_effects", value);
        self.set_health(char_index, new_health);
        events
    }

    /// Casts the heal `skill` from the caster at `caster_index` onto the receiver
    /// at `receiver_index` (seam 9, V4): routes the skill from the held atlas,
    /// reads the receiver's current health, runs [`cast_heal`], persists the
    /// caster's spent vitals (K1), then reconstructs the receiver's post-heal pool
    /// by crediting the returned pre-clamped `amount` to the SAME `Pool` it passed
    /// in as `receiver_health` — never a stale copy — and persists it through
    /// [`Self::set_health`]. `current + amount <= max` holds only against that very
    /// pool (the amount is pre-clamped against it). Returns the outcome and the
    /// reconstructed pool.
    pub fn cast_heal_on(
        &mut self,
        caster_index: usize,
        receiver_index: usize,
        skill: SkillNumber,
    ) -> (BuffCastOutcome, Pool) {
        let (spent_vitals, outcome, receiver_health) = {
            let skill_def = or_abort(self.atlas.skill(skill).ok_or("unknown skill"));
            let heal = match route(skill_def) {
                SkillRouting::Heal(heal) => heal,
                SkillRouting::Damaging(_) | SkillRouting::Buff(_) | SkillRouting::Deferred => {
                    return or_abort(Err::<(BuffCastOutcome, Pool), _>("skill is not a heal"));
                }
            };
            let receiver_health =
                or_abort(self.characters.get(receiver_index).ok_or("no receiver"))
                    .vitals()
                    .health;
            let caster = or_abort(self.characters.get(caster_index).ok_or("no caster"));
            let (vitals, outcome) = cast_heal(caster, heal, receiver_health);
            (vitals, outcome, receiver_health)
        };
        let vitals_value = or_abort(serde_json::to_value(spent_vitals));
        self.persist_character_with(caster_index, "vitals", vitals_value);
        let reconstructed = match outcome {
            BuffCastOutcome::Healed { amount } => receiver_health.restored(amount),
            BuffCastOutcome::Rejected { .. } | BuffCastOutcome::Applied { .. } => receiver_health,
        };
        self.set_health(receiver_index, reconstructed);
        (outcome, reconstructed)
    }

    /// Sets the wallet of the character at `char_index` by editing its wire form
    /// and re-loading it (the only path a `Character` mutates — serde-only,
    /// re-proving the carry cap on load). This is the persist seam in the
    /// database-load direction: a host loading a saved account restores its zen,
    /// and every service that returns a new `CarriedZen` balance (buy, sell, a
    /// mix fee, a zen offer, a pickup) writes it back through here.
    pub fn set_wallet(&mut self, char_index: usize, wallet: CarriedZen) {
        let value = or_abort(serde_json::to_value(wallet));
        self.persist_character_with(char_index, "zen", value);
    }

    /// Steps the character at `char_index` one tile toward `target` (seam 4):
    /// reads its placement, runs [`resolve_step`] over the map's walk grid from
    /// the held atlas at a one-tile speed, and on `Resolved` writes the new
    /// placement back *through* the persist seam by serde-editing the character's
    /// `placement` field. A `Blocked` step leaves the character put. Returns the
    /// step outcome. Grounded steps are grid-checked, so a scenario walks along a
    /// walkable run ([`walkable_run`]).
    pub fn step(&mut self, char_index: usize, target: WorldPos) -> StepOutcome {
        let placement = self.character(char_index).placement();
        let grid = or_abort(self.atlas.walk_grid(placement.map).ok_or("no walk grid"));
        let outcome = resolve_step(placement, target, ONE_TILE, grid);
        if let StepOutcome::Resolved { placement } = &outcome {
            let value = or_abort(serde_json::to_value(placement));
            self.persist_character_with(char_index, "placement", value);
        }
        outcome
    }

    /// Changes the flight mode of the character at `char_index` (seam 8): the
    /// host derives the `Wings` eligibility fact from whether a wing is worn in
    /// the character's own equipment (I1-style host derivation), reads the map's
    /// environment from the held atlas, and runs [`change_flight`] free of any
    /// combat lock. The returned authoritative `Movement` is written back onto
    /// the character's `placement.movement` through the persist seam; the outcome
    /// vec is returned for the scenario to assert on.
    ///
    /// [`change_flight`]: mu_core::services::movement::change_flight
    pub fn change_flight(&mut self, char_index: usize, change: FlightChange) -> Vec<FlightOutcome> {
        let placement = self.character(char_index).placement();
        let wings = match self.equipment(char_index).get(EquipmentSlot::Wings) {
            Some(_) => Wings::Equipped,
            None => Wings::None,
        };
        let env = or_abort(self.atlas.map_handle(placement.map).ok_or("no map handle"))
            .definition()
            .environment;
        let (movement, outcomes) = mu_core::services::movement::change_flight(
            placement.movement,
            change,
            env,
            wings,
            CombatLock::Free,
        );
        let mut moved = placement;
        moved.movement = movement;
        let value = or_abort(serde_json::to_value(moved));
        self.persist_character_with(char_index, "placement", value);
        outcomes
    }

    /// Buys the shelf entry at `slot` from merchant `npc` for the character at
    /// `char_index` (seam 2): resolves the shop view from the held atlas, reads
    /// the character's wallet and its current placement as the buyer position
    /// (so movement-into-range gates the buy — seam 4), and runs [`buy`]. The
    /// returned bag is written back through the persist seam; a success balance
    /// is written to the wallet via [`Self::set_wallet`].
    ///
    /// [`buy`]: mu_core::services::shop::buy
    pub fn buy(
        &mut self,
        char_index: usize,
        npc: MonsterNumber,
        slot: ShelfSlot,
        merchant_pos: WorldPos,
    ) -> BuyOutcome {
        let wallet = self.character(char_index).zen();
        let buyer_pos = self.character(char_index).placement().position;
        let inventory = self.inventory(char_index).clone();
        let shop = or_abort(self.atlas.shop(npc).ok_or("no shop for npc"));
        let (new_inventory, outcome) =
            mu_core::services::shop::buy(inventory, wallet, shop, slot, buyer_pos, merchant_pos);
        self.store_inventory(char_index, new_inventory);
        match &outcome {
            BuyOutcome::NewItem { balance, .. } | BuyOutcome::Merged { balance, .. } => {
                self.set_wallet(char_index, *balance);
            }
            BuyOutcome::OutOfRange
            | BuyOutcome::UnknownShelfSlot
            | BuyOutcome::InventoryFull
            | BuyOutcome::InsufficientZen => {}
        }
        outcome
    }

    /// Sells the item covering `cell` from the bag of the character at
    /// `char_index` to any merchant at `merchant_pos` (seam 2/8): reads the
    /// character's wallet and placement (range gate), runs [`sell`] over the held
    /// atlas, writes the returned bag back through the persist seam, and credits
    /// a `Sold` balance to the wallet — the crafted item priced from its own
    /// instance, destroyed by value.
    ///
    /// [`sell`]: mu_core::services::shop::sell
    pub fn sell(&mut self, char_index: usize, cell: Cell, merchant_pos: WorldPos) -> SellOutcome {
        let wallet = self.character(char_index).zen();
        let buyer_pos = self.character(char_index).placement().position;
        let inventory = self.inventory(char_index).clone();
        let (new_inventory, outcome) = mu_core::services::shop::sell(
            inventory,
            wallet,
            cell,
            buyer_pos,
            merchant_pos,
            &self.atlas,
        );
        self.store_inventory(char_index, new_inventory);
        match &outcome {
            SellOutcome::Sold { balance, .. } => self.set_wallet(char_index, *balance),
            SellOutcome::OutOfRange | SellOutcome::NoItemAtCell | SellOutcome::WalletFull => {}
        }
        outcome
    }

    /// Runs a chaos-machine mix for the character at `char_index` (seam 8): the
    /// host hands the placed multiset and the character's current wallet to
    /// [`mix`] over the held atlas and the world's stream (duty X1), then writes
    /// the returned balance back on a charged outcome (`Success`/`Failed`) — the
    /// fee is never refunded. A `Rejected` mix charges nothing and leaves the
    /// wallet untouched. The created/returned items ride the outcome for the
    /// scenario to land with the inventory drive methods (V5).
    ///
    /// [`mix`]: mu_core::services::craft::mix
    pub fn mix(&mut self, char_index: usize, placed: Vec<ItemInstance>) -> MixOutcome {
        let wallet = self.character(char_index).zen();
        let outcome = mu_core::services::craft::mix(placed, wallet, &self.atlas, &mut self.rng);
        match &outcome {
            MixOutcome::Failed { zen, .. } | MixOutcome::Success { zen, .. } => {
                self.set_wallet(char_index, *zen);
            }
            MixOutcome::Rejected { .. } => {}
        }
        outcome
    }

    /// Picks the whole zen pile at `ground_zen_index` into the wallet of the
    /// character at `char_index` (seam 2, V1/V7, P3): reads the character's
    /// wallet, runs [`pickup_zen`], and on `PickedUp` credits the returned
    /// balance and consumes the pile from the ground set. On `OverCap` the wallet
    /// and the untouched pile both stand — the over-cap pile is never clamped or
    /// split, it stays whole on the ground. Returns the outcome.
    ///
    /// [`pickup_zen`]: mu_core::services::inventory::pickup_zen
    pub fn pickup_zen(&mut self, char_index: usize, ground_zen_index: usize) -> ZenPickupOutcome {
        let wallet = self.character(char_index).zen();
        let pile = or_abort(
            self.ground_zen
                .get(ground_zen_index)
                .ok_or("no ground zen at index"),
        )
        .clone();
        let (balance, outcome) = mu_core::services::inventory::pickup_zen(pile, wallet);
        if let ZenPickupOutcome::PickedUp = &outcome {
            self.set_wallet(char_index, balance);
            self.ground_zen.remove(ground_zen_index);
        }
        outcome
    }

    /// Opens and accepts a trade between the requester at `requester_index` and
    /// the partner at `partner_index` (seam 4/10): [`request`] gates on the pair
    /// being within trade reach (from their current placements), then [`accept`]
    /// re-checks range as the partner; the two range-gated steps a trade needs.
    /// The `Requested` session is persisted between the two calls, and the opened
    /// session is seated through the persist seam. Returns its index.
    pub fn open_and_accept_trade(&mut self, requester_index: usize, partner_index: usize) -> usize {
        let requester_pos = self.character(requester_index).placement().position;
        let partner_pos = self.character(partner_index).placement().position;
        let (opened, _events) = request(
            requester_pos,
            TradeAvailability::Available {
                position: partner_pos,
            },
        );
        let requested = match opened {
            RequestOutcome::Opened { session } => persist(session),
            RequestOutcome::Rejected { .. } => {
                return or_abort(Err::<usize, _>("trade request was rejected"));
            }
        };
        let (accepted, _events) = accept(requested, Side::Partner, requester_pos, partner_pos);
        let open = match accepted {
            AcceptOutcome::Accepted { session } => session,
            AcceptOutcome::WrongSide { .. }
            | AcceptOutcome::OutOfRange { .. }
            | AcceptOutcome::NotRequested { .. } => {
                return or_abort(Err::<usize, _>("trade accept did not open the session"));
            }
        };
        self.seat_session(open)
    }

    /// Offers the bag item covering `from` from the character at `char_index`
    /// into that actor's window at `to` (seam 10, escrow-by-move): reads the
    /// session and the actor's bag, runs [`offer_item`], and writes both the new
    /// session and the (item-lightened) bag back through the persist seam. Post-
    /// accept, no position is consulted — this succeeds no matter how far the
    /// actor has walked (seam 4, S-REACH-2).
    pub fn offer_item_to_trade(
        &mut self,
        session_index: usize,
        char_index: usize,
        actor: Side,
        from: Cell,
        to: Cell,
    ) -> OfferOutcome {
        let session = self.session(session_index).clone();
        let inventory = self.inventory(char_index).clone();
        let (new_session, new_inventory, outcome, _events) =
            offer_item(session, actor, inventory, from, to);
        self.store_session(session_index, new_session);
        self.store_inventory(char_index, new_inventory);
        outcome
    }

    /// Sets the acting side's zen offer to `amount` for the character at
    /// `char_index` (seam 10): reads that side's wallet, runs [`offer_zen`]
    /// (moving the wallet by the delta only), and writes the new session and the
    /// new wallet back through the persist seam.
    pub fn offer_zen_to_trade(
        &mut self,
        session_index: usize,
        char_index: usize,
        actor: Side,
        amount: Zen,
    ) -> ZenOfferOutcome {
        let session = self.session(session_index).clone();
        let wallet = self.character(char_index).zen();
        let (new_session, new_wallet, outcome, _events) = offer_zen(session, actor, wallet, amount);
        self.store_session(session_index, new_session);
        self.set_wallet(char_index, new_wallet);
        outcome
    }

    /// Locks the acting side of the trade (seam 10): builds each side's
    /// [`Holdings`] from its own bag and wallet, runs [`lock`], and always writes
    /// both handed-back holdings home through the persist seam (transformed on
    /// `Completed`, untouched on a bounce). On `Completed` the consumed session
    /// is removed from the world; every other result carries the session back and
    /// is re-seated. Returns the lock result.
    pub fn lock_trade(
        &mut self,
        session_index: usize,
        requester_index: usize,
        partner_index: usize,
        actor: Side,
    ) -> LockResult {
        let session = self.session(session_index).clone();
        let requester = self.holdings_of(requester_index);
        let partner = self.holdings_of(partner_index);
        let (new_requester, new_partner, result, _events) =
            lock(session, actor, requester, partner);
        self.store_holdings(requester_index, new_requester);
        self.store_holdings(partner_index, new_partner);
        match &result {
            LockResult::Completed => {
                self.sessions.remove(session_index);
            }
            LockResult::Locked { session }
            | LockResult::AlreadyLocked { session }
            | LockResult::NotOpen { session }
            | LockResult::Bounced { session, .. } => {
                self.store_session(session_index, session.clone());
            }
        }
        result
    }

    /// Cancels the trade for `reason` (seam 10): builds each side's [`Holdings`],
    /// runs [`cancel`] (total over every phase), writes each settled bag and
    /// wallet home through the persist seam, and removes the consumed session.
    /// Whatever escrow cannot land rides the returned [`Settlement`]'s per-side
    /// overflow for the host to ground-drop (T5) — returned to the scenario, not
    /// dropped here.
    pub fn cancel_trade(
        &mut self,
        session_index: usize,
        reason: CancelReason,
        requester_index: usize,
        partner_index: usize,
    ) -> Settlement {
        let session = self.session(session_index).clone();
        let requester = self.holdings_of(requester_index);
        let partner = self.holdings_of(partner_index);
        let (settlement, _events) = cancel(session, reason, requester, partner);
        self.store_holdings(
            requester_index,
            Holdings {
                inventory: settlement.requester.inventory.clone(),
                wallet: settlement.requester.wallet,
            },
        );
        self.store_holdings(
            partner_index,
            Holdings {
                inventory: settlement.partner.inventory.clone(),
                wallet: settlement.partner.wallet,
            },
        );
        self.sessions.remove(session_index);
        settlement
    }

    /// The character's [`Holdings`] — its bag and wallet fused — read for a trade
    /// completion or settlement.
    fn holdings_of(&self, char_index: usize) -> Holdings {
        Holdings {
            inventory: self.inventory(char_index).clone(),
            wallet: self.character(char_index).zen(),
        }
    }

    /// Writes a handed-back [`Holdings`] home: the bag persisted into its slot,
    /// the wallet through the serde-only [`Self::set_wallet`] seam.
    fn store_holdings(&mut self, char_index: usize, holdings: Holdings) {
        self.store_inventory(char_index, holdings.inventory);
        self.set_wallet(char_index, holdings.wallet);
    }

    /// Persists `inventory` into the bag slot of the character at `char_index`.
    fn store_inventory(&mut self, char_index: usize, inventory: Inventory) {
        let slot = or_abort(self.inventories.get_mut(char_index).ok_or("no bag slot"));
        *slot = persist(inventory);
    }

    /// Persists `session` into the trade slot at `session_index`.
    fn store_session(&mut self, session_index: usize, session: TradeSession) {
        let slot = or_abort(
            self.sessions
                .get_mut(session_index)
                .ok_or("no session slot"),
        );
        *slot = persist(session);
    }

    /// Replaces one top-level wire field of the character at `char_index` with
    /// `value` and re-loads it — the serde-only mutation path (`Character` has no
    /// setters), which re-proves every invariant on the way in. The one seam a
    /// wallet balance or a stepped placement rides back through.
    fn persist_character_with(&mut self, char_index: usize, field: &str, value: serde_json::Value) {
        let character = or_abort(self.characters.get(char_index).ok_or("no character"));
        let mut wire = or_abort(serde_json::to_value(character));
        {
            let object = or_abort(wire.as_object_mut().ok_or("character is not an object"));
            object.insert(field.to_owned(), value);
        }
        let updated: Character = or_abort(serde_json::from_value(wire));
        let persisted = persist(updated);
        let slot = or_abort(
            self.characters
                .get_mut(char_index)
                .ok_or("no character slot"),
        );
        *slot = persisted;
    }

    /// Serialises exactly the live sets — the atlas, the stream, and the map are
    /// excluded — so a replay divergence cannot hide in an unpersisted field.
    #[must_use]
    pub fn snapshot(&self) -> String {
        or_abort(serde_json::to_string(&LiveSnapshot {
            characters: &self.characters,
            inventories: &self.inventories,
            equipment: &self.equipment,
            monsters: &self.monsters,
            ground_items: &self.ground_items,
            ground_zen: &self.ground_zen,
            sessions: &self.sessions,
        }))
    }
}

// --- Shared fixtures over the real dataset. ----------------------------------

/// A tile coordinate — whole-tile positions keep every fixture on the grid.
#[must_use]
pub fn tile(x: u8, y: u8) -> TileCoord {
    TileCoord::new(x, y)
}

/// A world position at the centre of tile `(x, y)`.
#[must_use]
pub fn pos(x: u8, y: u8) -> WorldPos {
    TileCoord::new(x, y).to_world()
}

/// A capped wallet holding `value`.
#[must_use]
pub fn zen(value: u64) -> CarriedZen {
    or_abort(CarriedZen::new(value))
}

/// A plausible gearless Dark Knight at the given level, strength, and tile —
/// built the only way a character can be, by deserialising its wire form.
#[must_use]
pub fn dark_knight(level: u16, strength: u16, at: TileCoord) -> Character {
    let position = or_abort(serde_json::to_value(at.to_world()));
    let json = serde_json::json!({
        "class": "dark_knight",
        "level": level,
        "experience": 0,
        "stats": {"kind": "standard", "strength": strength, "agility": 120, "vitality": 100, "energy": 30},
        "unspent_points": 0,
        "zen": 0,
        "placement": {"position": position, "facing": {"x": 1, "y": 0}, "movement": "grounded", "map": 0},
        "vitals": {
            "health": {"current": 500, "max": 500},
            "mana": {"current": 400, "max": 400},
            "ability": {"current": 400, "max": 400}
        }
    });
    or_abort(serde_json::from_value(json))
}

/// The first fighting monster at or below `max_level`, with its combat block
/// and resistances — only the `Monster` role carries a droppable, fightable
/// block usable as a plain kill target.
#[must_use]
pub fn low_level_monster(
    atlas: &Atlas,
    max_level: u16,
) -> (MonsterNumber, MonsterCombat, PerElement<Resistance>) {
    or_abort(
        atlas
            .monsters()
            .find_map(|definition| match &definition.role {
                MonsterRole::Monster {
                    combat,
                    resistances,
                    ..
                } => (combat.level.get() <= max_level && combat.hp > 0).then_some((
                    definition.number,
                    *combat,
                    *resistances,
                )),
                MonsterRole::Guard { .. }
                | MonsterRole::Trap { .. }
                | MonsterRole::Npc { .. }
                | MonsterRole::SoccerBall => None,
            })
            .ok_or("the dataset has no low-level fighting monster"),
    )
}

/// A live instance of monster `number` at full `hp`, anchored on tile `at`.
#[must_use]
pub fn monster_instance(number: MonsterNumber, hp: u32, at: TileCoord) -> MonsterInstance {
    MonsterInstance {
        number,
        placement: Placement {
            position: at.to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: MapNumber(0),
        },
        health: Pool::full(hp),
        anchor: at.to_world(),
        next_action: Tick(0),
        active_effects: ActiveEffects::EMPTY,
    }
}

/// A fresh instance of real item `id` at plus-`level`, full base gauge — the
/// leveled ingredient a chaos recipe needs (a +9 helm, a +6 sword). The base
/// durability is intentionally not level-scaled: mix ingredients are handed
/// straight to the service (never persisted), and the service reads only the
/// plus-level.
#[must_use]
pub fn item_at_level(atlas: &Atlas, id: ItemRef, level: u8) -> ItemInstance {
    let mut instance = item_instance(atlas, id);
    instance.level = or_abort(ItemLevel::new(level));
    instance
}

/// A fresh instance of real item `id` at plus-level zero, full gauge.
#[must_use]
pub fn item_instance(atlas: &Atlas, id: ItemRef) -> ItemInstance {
    let def = or_abort(atlas.item(id).ok_or("unknown item"));
    ItemInstance {
        item: id,
        level: ItemLevel::ZERO,
        roll: RarityRoll::Normal,
        normal_option: None,
        luck: LuckRoll::Plain,
        skill: SkillRoll::NoSkill,
        durability: Durability::full(def.durability),
        augment: CraftedAugment::None,
    }
}

/// Real item `id`'s cell footprint — what a host looks up from the atlas to
/// pick the item up off the ground.
#[must_use]
pub fn footprint_of(atlas: &Atlas, id: ItemRef) -> Footprint {
    let def = or_abort(atlas.item(id).ok_or("unknown item"));
    or_abort(Footprint::new(def.width, def.height))
}

/// An empty main-inventory bag — the classic 8×8 grid the client renders — the
/// parallel live set [`World::seat_character`] seats for every character.
#[must_use]
pub fn bag() -> Inventory {
    Inventory::empty(8, 8)
}

/// A bag cell coordinate — the anchor a pickup or a bag placement addresses.
#[must_use]
pub fn cell(row: u8, col: u8) -> Cell {
    Cell { row, col }
}

/// The first fighting monster at or above `min_level`, with its combat block and
/// resistances — a heavier victim than [`low_level_monster`], for a kill whose
/// experience crosses several levels or whose strike drains a player fast. Only
/// the `Monster` role carries a droppable, fightable block usable as a plain
/// target.
#[must_use]
pub fn fighting_monster_from(
    atlas: &Atlas,
    min_level: u16,
) -> (MonsterNumber, MonsterCombat, PerElement<Resistance>) {
    or_abort(
        atlas
            .monsters()
            .find_map(|definition| match &definition.role {
                MonsterRole::Monster {
                    combat,
                    resistances,
                    ..
                } => (combat.level.get() >= min_level && combat.hp > 0).then_some((
                    definition.number,
                    *combat,
                    *resistances,
                )),
                MonsterRole::Guard { .. }
                | MonsterRole::Trap { .. }
                | MonsterRole::Npc { .. }
                | MonsterRole::SoccerBall => None,
            })
            .ok_or("the dataset has no fighting monster at that level"),
    )
}

/// The number of the first fighting monster that both LANDS on a low-level
/// knight and stays survivable hit-for-hit: an attack rate at or above 80
/// (out-rating a level-20 knight's defense so the bite reliably connects) with
/// per-hit damage capped at 70 (a wound a single self-heal can chase). Re-found
/// from the roster on every run, never a hard-coded number.
#[must_use]
pub fn pressing_monster(atlas: &Atlas) -> MonsterNumber {
    or_abort(
        atlas
            .monsters()
            .find_map(|definition| match &definition.role {
                MonsterRole::Monster { combat, .. } => {
                    (combat.attack_rate >= 80 && combat.max_phys_damage <= 70 && combat.hp > 0)
                        .then_some(definition.number)
                }
                MonsterRole::Guard { .. }
                | MonsterRole::Trap { .. }
                | MonsterRole::Npc { .. }
                | MonsterRole::SoccerBall => None,
            })
            .ok_or("the dataset has no pressing-but-survivable monster"),
    )
}

/// The number of the first passive definition — a town NPC or the soccer ball —
/// whose placement resolves to a [`mu_core::entities::spawned::Spawned::Placed`],
/// never a fightable mob.
#[must_use]
pub fn first_passive_monster(atlas: &Atlas) -> MonsterNumber {
    or_abort(
        atlas
            .monsters()
            .find_map(|definition| match &definition.role {
                MonsterRole::Npc { .. } | MonsterRole::SoccerBall => Some(definition.number),
                MonsterRole::Monster { .. }
                | MonsterRole::Guard { .. }
                | MonsterRole::Trap { .. } => None,
            })
            .ok_or("the dataset has no passive definition"),
    )
}

/// The number of the first fighting monster whose behavior can attack (attack
/// range at least one tile) — a mob that returns [`MonsterIntent::Attack`] on a
/// cardinal-adjacent target, so a retarget flips the intent's `target` verbatim.
/// The one attack-range-zero monster in the roster is skipped.
///
/// [`MonsterIntent::Attack`]: mu_core::events::monster_ai::MonsterIntent::Attack
#[must_use]
pub fn aggressive_monster(atlas: &Atlas) -> MonsterNumber {
    or_abort(
        atlas
            .monsters()
            .find_map(|definition| match &definition.role {
                MonsterRole::Monster { behavior, .. } => {
                    (behavior.attack_range >= 1).then_some(definition.number)
                }
                MonsterRole::Guard { .. }
                | MonsterRole::Trap { .. }
                | MonsterRole::Npc { .. }
                | MonsterRole::SoccerBall => None,
            })
            .ok_or("the dataset has no attacking monster"),
    )
}

/// The number of the first skill the router resolves to a heal — the atlas is the
/// source, never a hard-coded skill id, so the fixture re-finds it from the
/// shipped catalog on every run.
#[must_use]
pub fn heal_skill(atlas: &Atlas) -> SkillNumber {
    or_abort(
        atlas
            .skills()
            .find_map(|skill| match route(skill) {
                SkillRouting::Heal(_) => Some(skill.number),
                SkillRouting::Damaging(_) | SkillRouting::Buff(_) | SkillRouting::Deferred => None,
            })
            .ok_or("the dataset has no heal skill"),
    )
}

/// The number of the first non-elemental single-target damaging skill — a clean
/// `DirectHit` carrying a real mana cost, re-found from the shipped catalog on
/// every run (the router is the source, never a hard-coded skill id). Chosen
/// non-elemental so a landed hit inflicts no ailment and triggers no knockback,
/// keeping the kill-chain writeback a pure health drain.
#[must_use]
pub fn direct_hit_skill(atlas: &Atlas) -> SkillNumber {
    or_abort(
        atlas
            .skills()
            .find_map(|skill| match route(skill) {
                SkillRouting::Damaging(reference) => {
                    (matches!(reference.shape(), DamagingSkill::DirectHit)
                        && skill.element.is_none()
                        && skill.cost.mana > 0)
                        .then_some(skill.number)
                }
                SkillRouting::Buff(_) | SkillRouting::Heal(_) | SkillRouting::Deferred => None,
            })
            .ok_or("the dataset has no non-elemental direct-hit skill"),
    )
}

/// The number of the first caster-centred area skill — a `Nova`-pattern strike
/// whose region is a disc around the caster, so one cast sweeps every seated mob
/// within range. Re-found from the catalog, never hard-coded.
#[must_use]
pub fn nova_skill(atlas: &Atlas) -> SkillNumber {
    or_abort(
        atlas
            .skills()
            .find_map(|skill| match route(skill) {
                SkillRouting::Damaging(reference) => matches!(
                    reference.shape(),
                    DamagingSkill::Area {
                        pattern: AreaPattern::Nova
                    }
                )
                .then_some(skill.number),
                SkillRouting::Buff(_) | SkillRouting::Heal(_) | SkillRouting::Deferred => None,
            })
            .ok_or("the dataset has no nova area skill"),
    )
}

/// The twelve worn slots, in the order the auto-equip oracle tries them — an
/// item lands in the first that accepts it.
const EQUIP_SLOTS: [EquipmentSlot; 12] = [
    EquipmentSlot::Helm,
    EquipmentSlot::Armor,
    EquipmentSlot::Pants,
    EquipmentSlot::Gloves,
    EquipmentSlot::Boots,
    EquipmentSlot::LeftHand,
    EquipmentSlot::RightHand,
    EquipmentSlot::Wings,
    EquipmentSlot::Pet,
    EquipmentSlot::Pendant,
    EquipmentSlot::Ring1,
    EquipmentSlot::Ring2,
];

/// Equips `item` into the first slot the core [`equip`] service accepts it in —
/// the service is the compatibility oracle, so no kind→slot rule is re-derived
/// here. When no slot accepts (a non-equippable drop), the item rides back an
/// `IncompatibleSlot` rejection.
fn equip_into_first_slot(
    worn: Equipment,
    item: ItemInstance,
    kind: &ItemKind,
    atlas: &Atlas,
) -> (Equipment, EquipOutcome) {
    let mut worn = worn;
    let mut item = item;
    for slot in EQUIP_SLOTS {
        let (updated, outcome) = equip(worn, item, kind, slot, atlas);
        match outcome {
            EquipOutcome::Equipped { slot } => return (updated, EquipOutcome::Equipped { slot }),
            EquipOutcome::Rejected { item: bounced, .. } => {
                worn = updated;
                item = bounced;
            }
        }
    }
    (
        worn,
        EquipOutcome::Rejected {
            reason: EquipRejection::IncompatibleSlot,
            item,
        },
    )
}

/// Whether the core equip service accepts item `id` into any worn slot — the
/// host's "is this a wearable drop" test, with the service itself as the slot
/// oracle (a jewel or consumable is a `Drop::Item` too, but wearable by none).
#[must_use]
pub fn is_equippable(atlas: &Atlas, id: ItemRef) -> bool {
    let def = or_abort(atlas.item(id).ok_or("unknown item"));
    let (_, outcome) = equip_into_first_slot(
        Equipment::empty(),
        item_instance(atlas, id),
        &def.kind,
        atlas,
    );
    matches!(outcome, EquipOutcome::Equipped { .. })
}

/// One-tile-per-step movement grain — the speed the step drive method walks a
/// character at, so each [`resolve_step`] lands exactly on the next tile centre.
const ONE_TILE: Fixed = Fixed::from_raw(UNITS_PER_TILE);

/// The host's fixed 50 ms tick cadence — the clock the host owns (U1), fed to
/// every service that converts millisecond durations to absolute ticks (the AI
/// reschedule, effect application and advance). One place decides the cadence.
fn host_tick() -> TickDuration {
    or_abort(TickDuration::new(50))
}

/// The first horizontal run of `length` consecutive walkable tiles on `map`,
/// discovered by scanning the real walk grid in row-major order — the
/// walkable corridor a grounded character is stepped along, so no step is ever
/// `Blocked` off the run. Returned as tile coordinates left-to-right; the caller
/// walks from the first toward the last. Data-driven, never a hard-coded tile:
/// the run is re-found from the shipped terrain on every run.
#[must_use]
pub fn walkable_run(atlas: &Atlas, map: MapNumber, length: usize) -> Vec<TileCoord> {
    let grid = or_abort(atlas.walk_grid(map).ok_or("no walk grid for map"));
    or_abort(first_walkable_run(grid, length).ok_or("no walkable run of that length"))
}

/// Scans `grid` row by row for the first run of `length` consecutive walkable
/// tiles, returning them left-to-right. `None` when no row carries such a run.
fn first_walkable_run(grid: &WalkGrid, length: usize) -> Option<Vec<TileCoord>> {
    for y in 0u8..=u8::MAX {
        let mut run: Vec<TileCoord> = Vec::new();
        for x in 0u8..=u8::MAX {
            let tile = TileCoord::new(x, y);
            if grid.walkable(tile.to_world()) {
                run.push(tile);
                if run.len() == length {
                    return Some(run);
                }
            } else {
                run.clear();
            }
        }
    }
    None
}
