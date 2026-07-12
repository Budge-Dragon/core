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

use core::num::NonZeroU16;

use serde::Serialize;
use serde::de::DeserializeOwned;

use mu_core::components::active_effect::{ActiveEffect, ActiveEffects};
use mu_core::components::collections::OneOrMore;
use mu_core::components::combat_profile::{CombatTarget, TargetKind};
use mu_core::components::drop_claim::PickerStanding;
use mu_core::components::element::PerElement;
use mu_core::components::equipment::{Equipment, EquipmentSlot};
use mu_core::components::interval::Interval;
use mu_core::components::inventory::{Cell, Footprint, Inventory};
use mu_core::components::item_instance::{
    CraftedAugment, Durability, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
};
use mu_core::components::item_quality::ItemRarity;
use mu_core::components::item_ref::ItemRef;
use mu_core::components::movement::{CombatLock, FlightChange, Movement, Wings};
use mu_core::components::party::{MemberSlot, Vitality};
use mu_core::components::placement::Placement;
use mu_core::components::pool::Pool;
use mu_core::components::spatial::{Facing, StepMagnitude, WorldPos};
use mu_core::components::tile::{TerrainGrid, TileArea, TileCoord};
use mu_core::components::trade_window::Side;
use mu_core::components::units::{
    CarriedZen, DurationMs, Exp, ItemLevel, Level, MapNumber, Resistance, Tick, TickDuration, Zen,
};
use mu_core::data::atlas::{Atlas, MiniGameHandle};
use mu_core::data::common::{MonsterNumber, SkillNumber};
use mu_core::data::effects::Ailment;
use mu_core::data::gates_warps::WarpIndex;
use mu_core::data::item_definitions::{EventKind, ItemDefinition, ItemKind};
use mu_core::data::minigame::{
    EntranceGate, EventLevel, MiniGameDefinition, MiniGameKey, MiniGameKind, PhaseSpan,
    PlayerBounds, Rank, RewardDropGroup, RewardEntry, RewardKind, RosterSlot, Score,
    SessionMonsterId, SpawnWave, SuccessFlag, SuccessFlags, TicketRequirement, WaveNumber,
    WaveRespawn, WaveSpawnArea,
};
use mu_core::data::monster_definitions::{MonsterCombat, MonsterRole};
use mu_core::data::npc_shops::ShelfSlot;
use mu_core::data::skills::{AreaDisplacement, AreaGeometry, DamageType, Skill};
use mu_core::data::spawns::SpawnPlacement;
use mu_core::entities::character::Character;
use mu_core::entities::minigame_session::MiniGameSession;
use mu_core::entities::monster_instance::MonsterInstance;
use mu_core::entities::party_session::{PartyInvite, PartySession};
use mu_core::entities::trade_session::TradeSession;
use mu_core::entities::world_item::WorldItem;
use mu_core::entities::world_zen::WorldZen;
use mu_core::events::combat::AttackOutcome;
use mu_core::events::consume::ConsumeEvent;
use mu_core::events::craft::MixOutcome;
use mu_core::events::death::{DeathEvent, Respawned};
use mu_core::events::effect::{BuffCastOutcome, EffectEvent};
use mu_core::events::ground::DespawnEvent;
use mu_core::events::inventory::{EquipOutcome, EquipRejection, PlaceOutcome, RemoveOutcome};
use mu_core::events::kill::KillResolution;
use mu_core::events::minigame::MiniGameEvent;
use mu_core::events::monster_ai::MonsterIntent;
use mu_core::events::movement::{FlightOutcome, StepOutcome};
use mu_core::events::party::{MemberAward, PartyEvent};
use mu_core::events::progression::GrowthEvent;
use mu_core::events::shop::{BuyOutcome, RepairOutcome, SellOutcome};
use mu_core::events::skills::{SkillOutcome, TargetHit};
use mu_core::events::trade::{CancelReason, OfferOutcome, Settlement, ZenOfferOutcome};
use mu_core::events::travel::{
    EnterGateOutcome, TownPortalOutcome, WarpEntryStatus, WarpTravelOutcome,
};
use mu_core::services::combat::StrikeBasis;
use mu_core::services::consume::use_consumable;
use mu_core::services::death::{DeathPenalty, combat_death_penalty, resolve_death, respawn};
use mu_core::services::effects::{
    ApplicableBuff, advance_effects, apply_ailment, apply_buff, mobility,
};
use mu_core::services::experience::apply_experience;
use mu_core::services::ground::{
    DropOrigin, ItemStamp, ZenStamp, reap_ground, stamp_item, stamp_zen,
};
use mu_core::services::inventory::{
    PickupOutcome, PlaceIntent, Wearer, ZenPickupOutcome, equip, place_item, remove_item,
};
use mu_core::services::item_roll::roll_dropped_item;
use mu_core::services::kill::resolve_kill;
use mu_core::services::minigame;
use mu_core::services::monster_ai::decide_monster_action;
use mu_core::services::movement::resolve_step;
use mu_core::services::party;
use mu_core::services::profile::{effective_profile, equipped_profile, monster_profile};
use mu_core::services::shop::{RepairSite, RepairSubject, repair};
use mu_core::services::skills::{
    DamagingSkill, DamagingSkillRef, Designation, SkillRouting, cast, cast_heal, route,
};
use mu_core::services::spawn::{SpawnResult, place_spawn};
use mu_core::services::trade::{
    AcceptOutcome, Holdings, LockResult, RequestOutcome, TradeAvailability, accept, cancel, lock,
    offer_item, offer_zen, request,
};
use mu_core::services::travel::{resolve_warp, traverse_enter_gate, use_town_portal, warp_menu};
use mu_core::services::wear::{WearEvent, resolve_strike_with_wear, wear_from_strike};

use dataset::real_static_data;
pub use dataset::{or_abort, real_atlas};
use rng::TestRng;

/// The one owned world value: the held static [`Atlas`], one seeded stream, the
/// current map context, and the ordered live sets. Every field except the
/// atlas, the stream, and the map is `serde`-persisted state; identity is the
/// index into each `Vec`, never a host id.
pub struct World {
    /// The static data index, rebuilt from source and held by value — the one
    /// carve-out from the persist seam (it never round-trips).
    atlas: Atlas,
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
    /// The live parties, addressed by index. A party's `MemberSlot(i)` maps to
    /// the character at index `i` — the character `Vec` index is the account↔slot
    /// map (the host owns the mapping; core sees only positional slots).
    parties: Vec<PartySession>,
    /// The pending party invites, addressed by index — owned data with a TTL,
    /// reaped independently of any session.
    pending_invites: Vec<PartyInvite>,
    /// The delivery log for durability-wear events — the host's outward route
    /// for the [`WearEvent`]s each strike or cast returns (a monster side wears
    /// nothing, so every logged event belongs to a character). Scenarios drain
    /// it with [`World::drain_wear_events`].
    delivered_wear: Vec<WearEvent>,
    /// The live mini-game sessions, addressed by index — each a caller-owned
    /// serde value the framework services thread through the persist seam. A
    /// session's `RosterSlot(i)` maps to the character at index `i` (the same
    /// positional account↔slot convention the party live set uses).
    mini_sessions: Vec<MiniGameSession>,
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
    parties: &'a [PartySession],
    pending_invites: &'a [PartyInvite],
    mini_sessions: &'a [MiniGameSession],
}

/// Addresses one combatant in a cast's target batch by the live set that owns
/// it: a seated player (a character index) or a seated monster (a monster
/// index). The paper host keys characters and monsters in separate positional
/// live sets, so a mixed player-and-monster batch needs this both to build each
/// target's combat view and to route each struck target's returned state back to
/// the set that owns it.
#[derive(Debug, Clone, Copy)]
pub enum Combatant {
    /// The character at this index — a player-kind combat target.
    Player(usize),
    /// The monster instance at this index — a monster-kind combat target.
    Monster(usize),
}

/// The write-back fields a resolved [`TargetHit`] carries in every variant: its
/// batch position, the target's returned health and effect store, and any
/// displacement a killed target is never displaced, so a `Killed` hit carries
/// `None`. Shared by the monster-only and the mixed player-and-monster
/// write-back drivers so the two never destructure the outcome differently.
fn target_hit_fields(hit: &TargetHit) -> (usize, Pool, ActiveEffects, Option<Placement>) {
    match hit {
        TargetHit::Killed {
            target_index,
            health,
            active_effects,
            ..
        } => (*target_index, *health, *active_effects, None),
        TargetHit::Missed {
            target_index,
            health,
            active_effects,
            displacement,
        }
        | TargetHit::Landed {
            target_index,
            health,
            active_effects,
            displacement,
            ..
        } => (*target_index, *health, *active_effects, *displacement),
    }
}

impl World {
    /// A fresh world on `map`, holding the real parsed [`Atlas`] and one stream
    /// seeded by `seed`, with every live set empty.
    #[must_use]
    pub fn new(seed: u64, map: MapNumber) -> Self {
        Self::from_atlas(real_atlas(), seed, map)
    }

    /// A fresh world on `map` whose held atlas carries the authored mini-game
    /// `definitions` resolved against the real terrain — the only way a scenario
    /// runs an event, since no mini-game rows ship. The real dataset is parsed
    /// with the definitions injected, so the entrance landing, the town hop, and
    /// the wave monster defs all resolve at parse exactly as a shipped row would.
    /// The scenario authors its definitions against a base [`real_atlas`] so it
    /// can find the ticket item and walkable rectangles by pattern, never a
    /// hard-coded number.
    #[must_use]
    pub fn with_mini_games(
        seed: u64,
        map: MapNumber,
        definitions: Vec<MiniGameDefinition>,
    ) -> Self {
        let mut data = real_static_data();
        data.mini_games.records = definitions;
        Self::from_atlas(or_abort(Atlas::parse(data)), seed, map)
    }

    /// The shared constructor: a fresh world on `map` over an already-parsed
    /// `atlas` and a stream seeded by `seed`, every live set empty.
    #[must_use]
    fn from_atlas(atlas: Atlas, seed: u64, map: MapNumber) -> Self {
        Self {
            atlas,
            rng: TestRng::new(seed),
            map,
            characters: Vec::new(),
            inventories: Vec::new(),
            equipment: Vec::new(),
            monsters: Vec::new(),
            ground_items: Vec::new(),
            ground_zen: Vec::new(),
            sessions: Vec::new(),
            parties: Vec::new(),
            pending_invites: Vec::new(),
            delivered_wear: Vec::new(),
            mini_sessions: Vec::new(),
        }
    }

    /// Drains the delivered durability-wear events in occurrence order — the
    /// scenario-side read of the host's wear delivery log.
    pub fn drain_wear_events(&mut self) -> Vec<WearEvent> {
        std::mem::take(&mut self.delivered_wear)
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

    /// How many money piles lie on the ground — proves a reaped pile left the
    /// set and a surviving one stayed.
    #[must_use]
    pub fn ground_zen_count(&self) -> usize {
        self.ground_zen.len()
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

    /// Stamps an item drop's lifecycle clocks by calling the core stamping
    /// seam over the held atlas duration and the host cadence — the
    /// appearance beat, the despawn tick, and the ownership window are all
    /// core arithmetic, never re-derived here.
    #[must_use]
    pub fn stamp_item_drop(&self, origin: DropOrigin, drop_tick: Tick) -> ItemStamp {
        stamp_item(
            origin,
            drop_tick,
            self.atlas.item_drop_duration(),
            host_tick(),
        )
    }

    /// Stamps a zen pile's lifecycle clocks by calling the core stamping seam
    /// — the [`Self::stamp_item_drop`] twin for money, which carries no claim.
    #[must_use]
    pub fn stamp_zen_drop(&self, origin: DropOrigin, drop_tick: Tick) -> ZenStamp {
        stamp_zen(
            origin,
            drop_tick,
            self.atlas.item_drop_duration(),
            host_tick(),
        )
    }

    /// Lays an item on the ground at `position` with the lifecycle clocks of
    /// `stamp` — the core-computed despawn tick and ownership window —
    /// synthesising its `map` from the world's current context (the
    /// ground-drop map a host must supply), seats it through the persist
    /// seam, and returns its index. The host seats a drop at its appearance
    /// tick; before then there is nothing on the ground to pick.
    pub fn seat_ground_item(
        &mut self,
        instance: ItemInstance,
        position: WorldPos,
        stamp: ItemStamp,
    ) -> usize {
        let index = self.ground_items.len();
        let world_item = WorldItem {
            instance,
            position,
            map: self.map,
            despawn: stamp.despawn,
            claim: stamp.claim,
        };
        self.ground_items.push(persist(world_item));
        index
    }

    /// Lays a money pile on the ground at `position` with the lifecycle
    /// clocks of `stamp`, synthesising its `map` from the world's current
    /// context, seats it through the persist seam, and returns its index.
    pub fn seat_ground_zen(&mut self, amount: Zen, position: WorldPos, stamp: ZenStamp) -> usize {
        let index = self.ground_zen.len();
        let world_zen = WorldZen {
            amount,
            position,
            map: self.map,
            despawn: stamp.despawn,
        };
        self.ground_zen.push(persist(world_zen));
        index
    }

    /// Advances the ground clock to `now`: hands both live ground sets to the
    /// core despawn reaper, stores the survivors back through the persist
    /// seam, and returns the despawn events for delivery. The flip rule lives
    /// in core; this is a thin persist-and-deliver driver.
    pub fn reap_ground(&mut self, now: Tick) -> Vec<DespawnEvent> {
        let items = std::mem::take(&mut self.ground_items);
        let zen = std::mem::take(&mut self.ground_zen);
        let (items, zen, events) = reap_ground(items, zen, now);
        self.ground_items = items.into_iter().map(persist).collect();
        self.ground_zen = zen.into_iter().map(persist).collect();
        events
    }

    /// Seats a trade session through the persist seam and returns its index.
    pub fn seat_session(&mut self, session: TradeSession) -> usize {
        let index = self.sessions.len();
        self.sessions.push(persist(session));
        index
    }

    /// Drives one physical strike from the character at `attacker_index` onto
    /// the monster at `target_index`: derives the attacker's strike view as
    /// `effective_profile(equipped_profile(..))` — gear folds first, active
    /// effects onto the gear-inclusive base — and the monster's as
    /// `effective_profile(monster_profile(..))`, then resolves the strike WITH
    /// durability wear through the core [`resolve_strike_with_wear`]
    /// composition (the monster side wears [`Equipment::empty`]). The returned
    /// health is written back onto the monster and the attacker's worn set
    /// back onto the attacker, both *through* the persist seam; the wear
    /// events are routed to the delivery log. A gearless, effect-free exchange
    /// is byte-identical to the bare strike (empty fold + empty pools).
    pub fn strike(&mut self, attacker_index: usize, target_index: usize) -> AttackOutcome {
        let attacker = or_abort(self.characters.get(attacker_index).ok_or("no attacker"));
        let attacker_worn =
            or_abort(self.equipment.get(attacker_index).ok_or("no worn set")).clone();
        let attacker_view = effective_profile(
            equipped_profile(attacker, &attacker_worn, &self.atlas),
            &attacker.active_effects(),
        );

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
        let target_view = effective_profile(target_profile, &monster.active_effects);

        let (new_health, outcome, wear) = resolve_strike_with_wear(
            &attacker_view,
            attacker_worn,
            &target_view,
            Equipment::empty(),
            monster.health,
            &StrikeBasis::PlainSwing,
            &self.atlas,
            &mut self.rng,
        );

        let persisted_worn = persist(wear.attacker_worn);
        let worn_slot = or_abort(self.equipment.get_mut(attacker_index).ok_or("no worn slot"));
        *worn_slot = persisted_worn;
        self.delivered_wear.extend(wear.attacker_events);
        self.delivered_wear.extend(wear.defender_events);

        let mut updated = monster;
        updated.health = new_health;
        let persisted = persist(updated);
        let slot = or_abort(self.monsters.get_mut(target_index).ok_or("no target slot"));
        *slot = persisted;
        outcome
    }

    /// Resolves one spawn record for monster `number` on the world's current map
    /// (seam 6): looks up the definition and the map's terrain grid from the held
    /// atlas, runs [`place_spawn`] over the world's stream, and hands the
    /// `SpawnResult` back for the scenario to seat and correlate. The returned
    /// aggregate's positional pairing to its event is the delivery key (V8) — the
    /// harness never invents an id.
    pub fn spawn_from(&mut self, number: MonsterNumber, placement: SpawnPlacement) -> SpawnResult {
        let def = or_abort(self.atlas.monster(number).ok_or("unknown monster number"));
        let grid = or_abort(
            self.atlas
                .terrain_grid(self.map)
                .ok_or("no terrain grid for map"),
        );
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
                .terrain_grid(mob.placement.map)
                .ok_or("no terrain grid"),
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
    /// stream and the atlas's option-roll policy, then lays it at `position` on
    /// the world's current map with the lifecycle clocks of `stamp` — the
    /// position comes from the victim's placement (available), the map from
    /// context (available), the clocks from the core stamping seam
    /// ([`Self::stamp_item_drop`]). Returns the ground index and the rolled
    /// instance for byte-identity checks.
    pub fn drop_item_to_ground(
        &mut self,
        item: ItemRef,
        level: ItemLevel,
        rarity: ItemRarity,
        position: WorldPos,
        stamp: ItemStamp,
    ) -> (usize, ItemInstance) {
        let def = or_abort(self.atlas.item(item).ok_or("unknown dropped item"));
        let rolled = roll_dropped_item(def, level, rarity, self.atlas.option_roll(), &mut self.rng);
        let index = self.seat_ground_item(rolled.clone(), position, stamp);
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
    /// lookup (I1); the picker's locus is read from its live placement; the
    /// reach and claim-window gates are core's. `standing` is the host-resolved
    /// kill-snapshot relation (a bare fact, never a verdict) and `now` is the
    /// window clock. On `PickedUp` the world item is consumed — removed from
    /// the ground set — and the new bag is written back through the seam; on
    /// any refusal the ground set is untouched and the untouched world item
    /// rides the outcome (move-only — nothing double-credited). Returns the
    /// outcome.
    pub fn pickup(
        &mut self,
        char_index: usize,
        ground_index: usize,
        anchor: Cell,
        standing: PickerStanding,
        now: Tick,
    ) -> PickupOutcome {
        let world_item =
            or_abort(self.ground_items.get(ground_index).ok_or("no ground item")).clone();
        let footprint = footprint_of(&self.atlas, world_item.instance.item);
        let inventory = or_abort(self.inventories.get(char_index).ok_or("no bag")).clone();
        let placement = self.character(char_index).placement();
        let (new_inventory, outcome) = mu_core::services::inventory::pickup(
            world_item,
            inventory,
            anchor,
            footprint,
            placement.position,
            placement.map,
            standing,
            now,
        );
        match &outcome {
            PickupOutcome::PickedUp { .. } => {
                let persisted = persist(new_inventory);
                let slot = or_abort(self.inventories.get_mut(char_index).ok_or("no bag slot"));
                *slot = persisted;
                self.ground_items.remove(ground_index);
            }
            PickupOutcome::Rejected { .. }
            | PickupOutcome::OutOfReach { .. }
            | PickupOutcome::Refused { .. } => {
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
    /// The wearer view is derived from the seated character, so the class and
    /// requirement gates decide with real eligibility. On success the new worn
    /// set is written back through the persist seam; a non-equippable or
    /// ineligible item is handed back in the returned rejection.
    pub fn equip_first_available(&mut self, char_index: usize, item: ItemInstance) -> EquipOutcome {
        let worn = or_abort(self.equipment.get(char_index).ok_or("no worn set")).clone();
        let wearer = wearer_of(self.character(char_index));
        let def = or_abort(self.atlas.item(item.item).ok_or("unknown item to equip"));
        let (new_worn, outcome) = equip_into_first_slot(worn, item, def, &self.atlas, &wearer);
        let persisted = persist(new_worn);
        let slot = or_abort(self.equipment.get_mut(char_index).ok_or("no worn slot"));
        *slot = persisted;
        outcome
    }

    /// Equips `item` onto the character at `char_index` into a NAMED `slot`
    /// (seam 1): the core [`equip`] service decides — accepting the item, or
    /// rejecting it with the reason its kind, the slot's occupancy, or the
    /// wearer's eligibility dictate
    /// ([`EquipRejection::IncompatibleSlot`], [`EquipRejection::SlotOccupied`],
    /// [`EquipRejection::TwoHandedConflict`], [`EquipRejection::ClassMismatch`],
    /// or [`EquipRejection::RequirementsNotMet`]). On a rejection the worn set
    /// is returned unchanged and the bounced item rides the outcome. The
    /// returned worn set is written back *through* the persist seam either way.
    ///
    /// [`EquipRejection::IncompatibleSlot`]: mu_core::events::inventory::EquipRejection::IncompatibleSlot
    /// [`EquipRejection::SlotOccupied`]: mu_core::events::inventory::EquipRejection::SlotOccupied
    /// [`EquipRejection::TwoHandedConflict`]: mu_core::events::inventory::EquipRejection::TwoHandedConflict
    /// [`EquipRejection::ClassMismatch`]: mu_core::events::inventory::EquipRejection::ClassMismatch
    /// [`EquipRejection::RequirementsNotMet`]: mu_core::events::inventory::EquipRejection::RequirementsNotMet
    pub fn equip_into(
        &mut self,
        char_index: usize,
        item: ItemInstance,
        slot: EquipmentSlot,
    ) -> EquipOutcome {
        let worn = or_abort(self.equipment.get(char_index).ok_or("no worn set")).clone();
        let wearer = wearer_of(self.character(char_index));
        let def = or_abort(self.atlas.item(item.item).ok_or("unknown item to equip"));
        let (new_worn, outcome) = equip(worn, item, def, slot, &self.atlas, &wearer);
        let persisted = persist(new_worn);
        let worn_slot = or_abort(self.equipment.get_mut(char_index).ok_or("no worn slot"));
        *worn_slot = persisted;
        outcome
    }

    /// Repairs the worn item at `slot` on the character at `char_index`
    /// through the shipped W-SHOP repair service at the self-repair site
    /// (seam 2): reads the live worn set and wallet, lets the core service
    /// gate, price, and refill, and writes the returned worn set and balance
    /// back through the persist seam. The repair RULE — the refill to the
    /// item's own stored max, the wear-ledger zeroing, the 5/2 self-repair
    /// surcharge — lives in core; this is a thin persist-and-deliver driver.
    pub fn self_repair_worn(&mut self, char_index: usize, slot: EquipmentSlot) -> RepairOutcome {
        let worn = or_abort(self.equipment.get(char_index).ok_or("no worn set")).clone();
        let wallet = self.character(char_index).zen();
        let position = self.character(char_index).placement().position;
        let (subject, outcome) = repair(
            RepairSubject::Equipped {
                equipment: worn,
                slot,
            },
            wallet,
            RepairSite::SelfRepair,
            position,
            &self.atlas,
        );
        let equipment = match subject {
            RepairSubject::Equipped { equipment, .. } => equipment,
            RepairSubject::Stored { .. } => {
                return or_abort(Err::<RepairOutcome, _>(
                    "the equipped repair subject threads back as equipped",
                ));
            }
        };
        let persisted = persist(equipment);
        let worn_slot = or_abort(self.equipment.get_mut(char_index).ok_or("no worn slot"));
        *worn_slot = persisted;
        if let RepairOutcome::Repaired { balance, .. } = outcome {
            self.set_wallet(char_index, balance);
        }
        outcome
    }

    /// Casts the damaging `skill` from the caster at `caster_index`, aimed at
    /// `aim`, over the batch of monsters at `target_indices` (seam 9, V4 twin for
    /// offence): routes the skill from the held atlas (aborting on a non-damaging
    /// one), derives the caster's own combat profile and one [`CombatTarget`] per
    /// batch monster from held state, resolves [`cast`] over the map's grid and the
    /// world's stream, persists the caster's spent vitals (K1), then writes each
    /// struck target's returned health, effects, and any displacement (a push
    /// or a jiggle) back onto its monster *through* the persist seam — mapping
    /// each hit's batch-position `target_index` to the monster index the caller
    /// supplied — and writes the outcome's authoritative `caster_placement`
    /// back onto the caster (a lunge teleports the caster onto its target;
    /// every other cast returns the unchanged placement). Returns the
    /// [`SkillOutcome`]. A rejection spends nothing and touches no target.
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
            let caster_worn = or_abort(self.equipment.get(caster_index).ok_or("no worn set"));
            let grid = or_abort(
                self.atlas
                    .terrain_grid(caster.placement().map)
                    .ok_or("no terrain grid"),
            );
            // The host parses the client's force-attack modifier: a single-target
            // skill force-attacks its designated target (the batch's first entry
            // here); an area skill strikes incidentally.
            let designation = match damaging.shape() {
                DamagingSkill::DirectHit | DamagingSkill::Lunge => {
                    Designation::Forced { target_index: 0 }
                }
                DamagingSkill::Area { .. } => Designation::Incidental,
            };
            cast(
                caster,
                &equipped_profile(caster, caster_worn, &self.atlas),
                damaging.locate(aim, designation),
                &targets,
                grid,
                &mut self.rng,
            )
        };
        let vitals_value = or_abort(serde_json::to_value(spent_vitals));
        self.persist_character_with(caster_index, "vitals", vitals_value);
        if let SkillOutcome::Cast {
            caster_placement,
            hits,
        } = &outcome
        {
            let placement_value = or_abort(serde_json::to_value(caster_placement));
            self.persist_character_with(caster_index, "placement", placement_value);
            for hit in hits {
                self.write_back_target_hit(target_indices, hit);
            }
            self.wear_caster_weapon(caster_index, hits);
        }
        outcome
    }

    /// Composes the cast path's offensive wear: one [`wear_from_strike`] per
    /// landed damaging [`TargetHit`], in the cast's returned hits order (the
    /// core-produced order, so the RNG stream stays a core contract). The
    /// struck monsters carry no gear, so only the caster's weapon side wears;
    /// the threaded worn set is persisted back and the events logged. A miss
    /// wears nothing — the cast path spends no ammunition on it either (no
    /// swing resolution reaches the wear seam without a landed hit).
    fn wear_caster_weapon(&mut self, caster_index: usize, hits: &[TargetHit]) {
        let mut worn = or_abort(self.equipment.get(caster_index).ok_or("no worn set")).clone();
        for hit in hits {
            let landed = match hit {
                TargetHit::Landed { hit, .. } | TargetHit::Killed { hit, .. } => {
                    AttackOutcome::Landed { hit: *hit }
                }
                TargetHit::Missed { .. } => continue,
            };
            let wear = wear_from_strike(
                &landed,
                worn,
                Equipment::empty(),
                &self.atlas,
                &mut self.rng,
            );
            worn = wear.attacker_worn;
            self.delivered_wear.extend(wear.attacker_events);
        }
        let persisted = persist(worn);
        let worn_slot = or_abort(self.equipment.get_mut(caster_index).ok_or("no worn slot"));
        *worn_slot = persisted;
    }

    /// Writes one struck target's [`TargetHit`] back onto its monster: maps the
    /// hit's batch-position `target_index` to the monster index the caster passed,
    /// sets the returned health and effects, applies any knockback displacement,
    /// and persists the updated instance through the seam.
    fn write_back_target_hit(&mut self, target_indices: &[usize], hit: &TargetHit) {
        let (batch_index, health, active_effects, displacement) = target_hit_fields(hit);
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

    /// Casts the damaging `skill` from the caster at `caster_index`, aimed at
    /// `aim`, over a mixed `batch` of seated players and monsters, carrying the
    /// client's force-attack `designation` (the CTRL-click the host parses; a
    /// single-target strike names its target, an area strike says whether a player
    /// was deliberately targeted). This is the player-target twin of
    /// [`Self::cast_damaging`]: it builds one [`CombatTarget`] per batch entry from
    /// the set that owns it — a player's gear-inclusive profile (stamped `Player`)
    /// or a monster's base combat profile (stamped `Npc`) — resolves [`cast`] over
    /// the caster's map grid and the world's stream, persists the caster's spent
    /// vitals and any lunge placement, then writes each struck target's returned
    /// health, effects, and displacement back to its own live set through the
    /// persist seam. The caster's weapon wears once per landed hit, exactly as the
    /// monster-target cast does. A rejection spends nothing and touches no target.
    pub fn cast_at(
        &mut self,
        caster_index: usize,
        skill: SkillNumber,
        aim: WorldPos,
        batch: &[Combatant],
        designation: Designation,
    ) -> SkillOutcome {
        let (spent_vitals, outcome) = {
            let skill_def = or_abort(self.atlas.skill(skill).ok_or("unknown skill"));
            let damaging = match route(skill_def) {
                SkillRouting::Damaging(reference) => reference,
                SkillRouting::Heal(_) | SkillRouting::Buff(_) | SkillRouting::Deferred => {
                    return or_abort(Err::<SkillOutcome, _>("skill is not a damaging skill"));
                }
            };
            let mut targets = Vec::with_capacity(batch.len());
            for &combatant in batch {
                targets.push(self.combat_target_of(combatant));
            }
            let caster = or_abort(self.characters.get(caster_index).ok_or("no caster"));
            let caster_worn = or_abort(self.equipment.get(caster_index).ok_or("no worn set"));
            let grid = or_abort(
                self.atlas
                    .terrain_grid(caster.placement().map)
                    .ok_or("no terrain grid"),
            );
            cast(
                caster,
                &equipped_profile(caster, caster_worn, &self.atlas),
                damaging.locate(aim, designation),
                &targets,
                grid,
                &mut self.rng,
            )
        };
        let vitals_value = or_abort(serde_json::to_value(spent_vitals));
        self.persist_character_with(caster_index, "vitals", vitals_value);
        if let SkillOutcome::Cast {
            caster_placement,
            hits,
        } = &outcome
        {
            let placement_value = or_abort(serde_json::to_value(caster_placement));
            self.persist_character_with(caster_index, "placement", placement_value);
            for hit in hits {
                self.write_back_combatant_hit(batch, hit);
            }
            self.wear_caster_weapon(caster_index, hits);
        }
        outcome
    }

    /// The [`CombatTarget`] view of one seated combatant, exactly as the offensive
    /// combat path consumes it: a player's gear-inclusive profile from
    /// [`equipped_profile`] (stamped `Player`) or a monster's base
    /// [`monster_profile`] (stamped `Npc`), each paired with its live health,
    /// placement, and effect store. The profile carries gear but not effects — the
    /// [`cast`] service folds the target's own effects onto it internally, so this
    /// mirrors how the monster batch is built.
    fn combat_target_of(&self, combatant: Combatant) -> CombatTarget {
        match combatant {
            Combatant::Player(index) => {
                let player = or_abort(self.characters.get(index).ok_or("no player target"));
                let worn = or_abort(self.equipment.get(index).ok_or("no target worn set"));
                CombatTarget::new(
                    equipped_profile(player, worn, &self.atlas),
                    player.vitals().health,
                    player.placement(),
                    player.active_effects(),
                )
            }
            Combatant::Monster(index) => {
                let mob = *or_abort(self.monsters.get(index).ok_or("no monster target"));
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
                        return or_abort(Err::<CombatTarget, _>(
                            "cast was handed a non-combat monster",
                        ));
                    }
                };
                CombatTarget::new(profile, mob.health, mob.placement, mob.active_effects)
            }
        }
    }

    /// Writes one resolved [`TargetHit`] back onto the combatant its batch index
    /// names — a player's live health, effects, and any displacement through the
    /// character persist seam, a monster's through the monster set — so a mixed
    /// player-and-monster cast persists every struck target to the set that owns
    /// it. The twin of [`Self::write_back_target_hit`] for a batch that may hold
    /// players.
    fn write_back_combatant_hit(&mut self, batch: &[Combatant], hit: &TargetHit) {
        let (batch_index, health, active_effects, displacement) = target_hit_fields(hit);
        let combatant = *or_abort(
            batch
                .get(batch_index)
                .ok_or("hit target index outside the batch"),
        );
        match combatant {
            Combatant::Player(index) => {
                self.set_health(index, health);
                let effects_value = or_abort(serde_json::to_value(active_effects));
                self.persist_character_with(index, "active_effects", effects_value);
                if let Some(placement) = displacement {
                    let placement_value = or_abort(serde_json::to_value(placement));
                    self.persist_character_with(index, "placement", placement_value);
                }
            }
            Combatant::Monster(index) => {
                let mut updated = *or_abort(self.monsters.get(index).ok_or("no monster at index"));
                updated.health = health;
                updated.active_effects = active_effects;
                if let Some(placement) = displacement {
                    updated.placement = placement;
                }
                let persisted = persist(updated);
                let slot = or_abort(self.monsters.get_mut(index).ok_or("no monster slot"));
                *slot = persisted;
            }
        }
    }

    /// Applies a kill's `gained` experience to the character at `char_index`
    /// through the core leveling service, persists the grown character, and
    /// returns the growth events for delivery. The growth RULE (points grant, cap
    /// clamp, vitals refill) now lives in core — this is a thin persist-and-deliver
    /// driver.
    pub fn apply_growth(&mut self, char_index: usize, gained: Exp) -> Vec<GrowthEvent> {
        let character = or_abort(self.characters.get(char_index).ok_or("no character")).clone();
        let (grown, events) = apply_experience(character, gained, &self.atlas);
        let persisted = persist(grown);
        let slot = or_abort(
            self.characters
                .get_mut(char_index)
                .ok_or("no character slot"),
        );
        *slot = persisted;
        events
    }

    /// Drives one monster strike onto the player (seam 7, V6): the forwarded
    /// `MonsterIntent::Attack` is resolved with the monster's effective profile
    /// as attacker, the player's `effective_profile(equipped_profile(..))` —
    /// gear folds first, active effects onto the gear-inclusive base — as
    /// target, and the player's current health `Pool` as `target_health`. The
    /// exchange runs through the core [`resolve_strike_with_wear`] composition
    /// (the monster side wears [`Equipment::empty`]), so a landed bite wears
    /// the player's gear. The returned health is written back by serde-editing
    /// `vitals.health`, the player's worn set through the persist seam, the
    /// wear events to the delivery log. The strike service is symmetric: the
    /// same service that kills monsters kills a player, only the views swapped.
    pub fn player_struck_by_monster(
        &mut self,
        player_index: usize,
        monster_index: usize,
    ) -> AttackOutcome {
        let player = or_abort(self.characters.get(player_index).ok_or("no player"));
        let player_worn = or_abort(self.equipment.get(player_index).ok_or("no worn set")).clone();
        let target_view = effective_profile(
            equipped_profile(player, &player_worn, &self.atlas),
            &player.active_effects(),
        );
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
        let attacker_view = effective_profile(attacker_profile, &monster.active_effects);

        let (new_health, outcome, wear) = resolve_strike_with_wear(
            &attacker_view,
            Equipment::empty(),
            &target_view,
            player_worn,
            target_health,
            &StrikeBasis::PlainSwing,
            &self.atlas,
            &mut self.rng,
        );

        let persisted_worn = persist(wear.defender_worn);
        let worn_slot = or_abort(self.equipment.get_mut(player_index).ok_or("no worn slot"));
        *worn_slot = persisted_worn;
        self.delivered_wear.extend(wear.attacker_events);
        self.delivered_wear.extend(wear.defender_events);

        self.set_health(player_index, new_health);
        outcome
    }

    /// Runs the monster-kill death step on the player at `char_index` (the
    /// W-DEATH seam, V6 continuation): reads the live character, calls the core
    /// [`resolve_death`] service at `at` on the host's fixed tick base with the
    /// penalty applied (the normal-death host path), persists the returned
    /// character, and hands the death events back for delivery. The
    /// death RULE — the exp + zen penalty, the Dead-marking, leaving vitals and
    /// effects in place — lives in core; this is a thin persist-and-deliver
    /// driver, the death twin of [`Self::apply_growth`]. No penalty, gate, refill,
    /// or clear logic is authored host-side.
    pub fn resolve_player_death(&mut self, char_index: usize, at: Tick) -> Vec<DeathEvent> {
        let character = or_abort(self.characters.get(char_index).ok_or("no character")).clone();
        let (dead, events) = resolve_death(
            character,
            at,
            host_tick(),
            &self.atlas,
            DeathPenalty::Applied,
        );
        let persisted = persist(dead);
        let slot = or_abort(
            self.characters
                .get_mut(char_index)
                .ok_or("no character slot"),
        );
        *slot = persisted;
        events
    }

    /// Runs the combat-death step on the player at `char_index` killed by an
    /// attacker of `attacker_kind` (the player-kill routing): the penalty is
    /// CORE-computed by [`combat_death_penalty`] — a player killer waives the
    /// victim's experience and zen penalty, a monster killer applies it — never a
    /// host-chosen literal, so a host can neither forge "a player kill is free" nor
    /// dock a player-killed victim. The returned dead character is persisted; the
    /// death events are handed back for delivery. The always-`Applied` twin is
    /// [`Self::resolve_player_death`].
    pub fn resolve_combat_death(
        &mut self,
        char_index: usize,
        at: Tick,
        attacker_kind: TargetKind,
    ) -> Vec<DeathEvent> {
        let character = or_abort(self.characters.get(char_index).ok_or("no character")).clone();
        let (dead, events) = resolve_death(
            character,
            at,
            host_tick(),
            &self.atlas,
            combat_death_penalty(attacker_kind),
        );
        let persisted = persist(dead);
        let slot = or_abort(
            self.characters
                .get_mut(char_index)
                .ok_or("no character slot"),
        );
        *slot = persisted;
        events
    }

    /// Runs the respawn step on the player at `char_index` once its scheduled
    /// respawn beat is due (the W-DEATH seam): reads the live character, calls the
    /// core [`respawn`] service over the world's stream (the single landing draw),
    /// persists the revived character, and hands the [`Respawned`] landing back for
    /// delivery. The respawn RULE — the gate pick, the landing sample, the vitals
    /// refill, the effect clear — lives in core; this is a thin persist-and-deliver
    /// driver, the revive twin of [`Self::resolve_player_death`]. `None` only when
    /// the character was already alive (the symmetric no-op).
    pub fn respawn_player(&mut self, char_index: usize) -> Option<Respawned> {
        let character = or_abort(self.characters.get(char_index).ok_or("no character")).clone();
        let (revived, respawned) = respawn(character, &self.atlas, &mut self.rng);
        let persisted = persist(revived);
        let slot = or_abort(
            self.characters
                .get_mut(char_index)
                .ok_or("no character slot"),
        );
        *slot = persisted;
        respawned
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
    /// reads its placement, runs [`resolve_step`] over the map's terrain grid from
    /// the held atlas at a one-tile speed, and on `Resolved` writes the new
    /// placement back *through* the persist seam by serde-editing the character's
    /// `placement` field. A `Blocked` step leaves the character put. Returns the
    /// step outcome. Grounded steps are grid-checked, so a scenario walks along a
    /// walkable run ([`walkable_run`]).
    pub fn step(&mut self, char_index: usize, target: WorldPos) -> StepOutcome {
        let placement = self.character(char_index).placement();
        let grid = or_abort(
            self.atlas
                .terrain_grid(placement.map)
                .ok_or("no terrain grid"),
        );
        let outcome = resolve_step(placement, target, ONE_TILE, grid);
        if let StepOutcome::Resolved { placement } = &outcome {
            let value = or_abort(serde_json::to_value(placement));
            self.persist_character_with(char_index, "placement", value);
        }
        outcome
    }

    /// The host-derived `Wings` eligibility fact for the character at
    /// `char_index`: worn wings in the dedicated slot flip it to `Equipped`
    /// (the I1-style derivation the flight and travel drives share).
    fn wings(&self, char_index: usize) -> Wings {
        match self.equipment(char_index).get(EquipmentSlot::Wings) {
            Some(_) => Wings::Equipped,
            None => Wings::None,
        }
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
        let wings = self.wings(char_index);
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

    /// Drinks the consumable covering `cell` in the bag of the character at
    /// `char_index` (the CONSUMABLE-USE flow): reads the live character and bag,
    /// calls the real [`use_consumable`] over the held atlas, and writes BOTH the
    /// returned character and bag back *through* the persist seam. No heal, cure,
    /// reject-when-no-op, or stack-decrement rule is authored host-side — the
    /// formula, the cap, the refusal, and the decrement all live in core. Returns
    /// the consume events for delivery.
    ///
    /// [`use_consumable`]: mu_core::services::consume::use_consumable
    pub fn use_consumable(&mut self, char_index: usize, cell: Cell) -> Vec<ConsumeEvent> {
        let character = self.character(char_index).clone();
        let inventory = self.inventory(char_index).clone();
        let (new_character, new_inventory, events) =
            use_consumable(character, inventory, cell, &self.atlas);
        let persisted = persist(new_character);
        let slot = or_abort(
            self.characters
                .get_mut(char_index)
                .ok_or("no character slot"),
        );
        *slot = persisted;
        self.store_inventory(char_index, new_inventory);
        events
    }

    /// Executes the warp-menu command for the character at `char_index` (the
    /// W-WARP seam): the host-parsed menu `index` resolves to its atlas-proven
    /// `WarpView` (an unknown index is a host parse failure core never sees —
    /// `or_abort` here), the real [`resolve_warp`] decides, and the returned
    /// character — debited wallet, sampled placement, grown discovered set —
    /// is written back *through* the persist seam. No discovery, level,
    /// fraction, or fee rule is authored host-side. Returns the outcome.
    pub fn warp(&mut self, char_index: usize, index: WarpIndex) -> WarpTravelOutcome {
        let character = self.character(char_index).clone();
        let wings = self.wings(char_index);
        let entry = or_abort(self.atlas.warp_by_index(index).ok_or("unknown warp index"));
        let (moved, outcome) = resolve_warp(character, entry, &self.atlas, wings, &mut self.rng);
        let persisted = persist(moved);
        let slot = or_abort(
            self.characters
                .get_mut(char_index)
                .ok_or("no character slot"),
        );
        *slot = persisted;
        outcome
    }

    /// The warp-menu availability projection for the character at `char_index`
    /// — the pure [`warp_menu`] query over the live character, the held atlas,
    /// and the host-derived wings fact. Reads only; nothing to persist.
    #[must_use]
    pub fn warp_menu(&self, char_index: usize) -> Vec<WarpEntryStatus> {
        warp_menu(
            self.character(char_index),
            &self.atlas,
            self.wings(char_index),
        )
    }

    /// Walks the character at `char_index` through the enter gate whose
    /// trigger covers its own position (the W-WARP seam): the host's
    /// positional trigger query resolves the gate view — a character standing
    /// on no trigger is a host dispatch failure, `or_abort` here — and the
    /// real [`traverse_enter_gate`] decides. The returned character (new map,
    /// grown discovered set) is written back *through* the persist seam.
    pub fn traverse_gate(&mut self, char_index: usize) -> EnterGateOutcome {
        let character = self.character(char_index).clone();
        let wings = self.wings(char_index);
        let placement = character.placement();
        let gate = or_abort(
            self.atlas
                .enter_gate_at(placement.map, placement.position)
                .ok_or("no enter gate trigger covers the traveler"),
        );
        let (moved, outcome) =
            traverse_enter_gate(character, gate, &self.atlas, wings, &mut self.rng);
        let persisted = persist(moved);
        let slot = or_abort(
            self.characters
                .get_mut(char_index)
                .ok_or("no character slot"),
        );
        *slot = persisted;
        outcome
    }

    /// Reads the Town Portal Scroll covering `cell` in the bag of the
    /// character at `char_index` (the W-WARP seam): the real
    /// [`use_town_portal`] decides — scroll identity, the single-piece
    /// consume, the town-gate landing — and BOTH the returned character and
    /// bag are written back *through* the persist seam. Returns the outcome.
    pub fn use_town_portal(&mut self, char_index: usize, cell: Cell) -> TownPortalOutcome {
        let character = self.character(char_index).clone();
        let inventory = self.inventory(char_index).clone();
        let (moved, new_inventory, outcome) =
            use_town_portal(character, inventory, cell, &self.atlas, &mut self.rng);
        let persisted = persist(moved);
        let slot = or_abort(
            self.characters
                .get_mut(char_index)
                .ok_or("no character slot"),
        );
        *slot = persisted;
        self.store_inventory(char_index, new_inventory);
        outcome
    }

    /// Re-seats the character at `char_index` on tile `at` of its current map
    /// — the persist seam in the load direction for position (the
    /// [`Self::set_wallet`] twin): a host loading a saved character restores
    /// where it stood. Same-map only by construction (the map rides along
    /// unchanged), so the current-map discovery invariant re-proves on load.
    pub fn place_at(&mut self, char_index: usize, at: TileCoord) {
        let mut placement = self.character(char_index).placement();
        placement.position = at.to_world();
        let value = or_abort(serde_json::to_value(placement));
        self.persist_character_with(char_index, "placement", value);
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
    /// wallet and its live placement (the picker locus the core reach gate
    /// tests), runs [`pickup_zen`], and on `PickedUp` credits the returned
    /// balance and consumes the pile from the ground set. On `OverCap` or
    /// `OutOfReach` the wallet and the untouched pile both stand — the pile is
    /// never clamped or split, it stays whole on the ground. Returns the
    /// outcome.
    ///
    /// [`pickup_zen`]: mu_core::services::inventory::pickup_zen
    pub fn pickup_zen(&mut self, char_index: usize, ground_zen_index: usize) -> ZenPickupOutcome {
        let wallet = self.character(char_index).zen();
        let placement = self.character(char_index).placement();
        let pile = or_abort(
            self.ground_zen
                .get(ground_zen_index)
                .ok_or("no ground zen at index"),
        )
        .clone();
        let (balance, outcome) = mu_core::services::inventory::pickup_zen(
            pile,
            wallet,
            placement.position,
            placement.map,
        );
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

    // --- Party lifecycle and the two shares. -------------------------------------
    //
    // A party's `MemberSlot(i)` maps to the character at index `i` (the host owns
    // the account↔slot map). Each drive method reads live values, calls a pure
    // party service, and writes every returned live value back *through* the
    // persist seam before storing it.

    /// The live party at `index`.
    #[must_use]
    pub fn party(&self, index: usize) -> &PartySession {
        or_abort(self.parties.get(index).ok_or("no party at index"))
    }

    /// How many live parties the world holds — proves a disband deleted one.
    #[must_use]
    pub fn party_count(&self) -> usize {
        self.parties.len()
    }

    /// The pending invite at `index`.
    #[must_use]
    pub fn pending_invite(&self, index: usize) -> &PartyInvite {
        or_abort(
            self.pending_invites
                .get(index)
                .ok_or("no pending invite at index"),
        )
    }

    /// How many pending invites the world holds — proves a lapse/accept reaped one.
    #[must_use]
    pub fn pending_invite_count(&self) -> usize {
        self.pending_invites.len()
    }

    /// Seats a pre-built party through the persist seam and returns its index —
    /// used by scenarios that need a standing roster to leave/kick/disconnect.
    pub fn seat_party(&mut self, party: PartySession) -> usize {
        let index = self.parties.len();
        self.parties.push(persist(party));
        index
    }

    /// One member's live share fact, read from the character at slot `slot`
    /// (identity = index). `vitality`/`position` are overridable by the scenario
    /// (a dead or strayed member) after the fact is built.
    #[must_use]
    pub fn member_fact(&self, slot: MemberSlot, vitality: Vitality) -> party::MemberFact {
        let character = self.character(usize::from(slot.0));
        party::MemberFact {
            slot,
            level: character.level(),
            experience: character.experience(),
            vitality,
            map: character.placement().map,
            position: character.placement().position,
        }
    }

    /// One member's wallet, read from the character at slot `slot`.
    #[must_use]
    pub fn slot_wallet(&self, slot: MemberSlot) -> party::SlotWallet {
        party::SlotWallet {
            slot,
            wallet: self.character(usize::from(slot.0)).zen(),
        }
    }

    /// Sends a solo-inviter party invite from the character at `inviter_char` to a
    /// host-resolved `target`, seating the invite on success through the persist
    /// seam. The inviter's locus comes from its own placement.
    pub fn invite(
        &mut self,
        inviter_char: usize,
        target: party::PartyAvailability,
        now: Tick,
    ) -> party::InviteOutcome {
        let inviter = self.character(inviter_char);
        let (outcome, _events) = party::invite(
            inviter.placement().position,
            inviter.placement().map,
            target,
            None,
            now,
            host_tick(),
        );
        if let party::InviteOutcome::Sent { invite } = &outcome {
            self.pending_invites.push(persist(*invite));
        }
        outcome
    }

    /// Accepts the pending invite at `invite_index` from the solo inviter at
    /// `inviter_char` on behalf of the responder at `responder_char`: re-resolves
    /// the inviter's live presence and the responder's locus, forms the party, and
    /// on success seats it and reaps the invite (both through the persist seam).
    pub fn accept_invite(
        &mut self,
        invite_index: usize,
        inviter_char: usize,
        responder_char: usize,
    ) -> party::AcceptOutcome {
        let inviter = self.character(inviter_char);
        let presence = party::InviterPresence::Present {
            position: inviter.placement().position,
            map: inviter.placement().map,
        };
        let responder = self.character(responder_char);
        let (outcome, _events) = party::accept_invite(
            presence,
            None,
            responder.placement().position,
            responder.placement().map,
        );
        if let party::AcceptOutcome::Joined { session } = &outcome {
            self.parties.push(persist(session.clone()));
            self.pending_invites.remove(invite_index);
        }
        outcome
    }

    /// Declines the pending invite at `invite_index`, reaping it; the returned
    /// events notify both sides.
    pub fn decline_invite(&mut self, invite_index: usize) -> Vec<PartyEvent> {
        let events = party::decline_invite();
        self.pending_invites.remove(invite_index);
        events
    }

    /// Kicks `target` from the party at `party_index` on the leader `actor`'s
    /// behalf. On `Kicked` the surviving party is persisted back; on `Disbanded`
    /// it is deleted; a named refusal leaves the roster stored and untouched.
    pub fn kick(
        &mut self,
        party_index: usize,
        actor: MemberSlot,
        target: MemberSlot,
    ) -> party::KickOutcome {
        let (outcome, _events) = party::kick(self.party(party_index).clone(), actor, target);
        match &outcome {
            party::KickOutcome::Kicked { session } => {
                self.store_party(party_index, session.clone());
            }
            party::KickOutcome::Disbanded => {
                self.parties.remove(party_index);
            }
            party::KickOutcome::NotLeader
            | party::KickOutcome::NoSuchMember
            | party::KickOutcome::CannotKickSelf => {}
        }
        outcome
    }

    /// Removes `actor` from the party at `party_index`. On `Left` the surviving
    /// party is persisted; on `Disbanded` it is deleted.
    pub fn leave(&mut self, party_index: usize, actor: MemberSlot) -> party::LeaveOutcome {
        let (outcome, _events) = party::leave(self.party(party_index).clone(), actor);
        match &outcome {
            party::LeaveOutcome::Left { session } => {
                self.store_party(party_index, session.clone());
            }
            party::LeaveOutcome::Disbanded => {
                self.parties.remove(party_index);
            }
            party::LeaveOutcome::NoSuchMember => {}
        }
        outcome
    }

    /// Holds the seat of `slot` in the party at `party_index` until it lapses,
    /// persisting the (leadership-adjusted) party back.
    pub fn disconnect(
        &mut self,
        party_index: usize,
        slot: MemberSlot,
        now: Tick,
    ) -> party::DisconnectOutcome {
        let (outcome, _events) =
            party::disconnect(self.party(party_index).clone(), slot, now, host_tick());
        if let party::DisconnectOutcome::Disconnected { session } = &outcome {
            self.store_party(party_index, session.clone());
        }
        outcome
    }

    /// Restores `slot` to `Active` in the party at `party_index`, persisting the
    /// party back.
    pub fn reconnect(&mut self, party_index: usize, slot: MemberSlot) -> party::ReconnectOutcome {
        let (outcome, _events) = party::reconnect(self.party(party_index).clone(), slot);
        if let party::ReconnectOutcome::Reconnected { session } = &outcome {
            self.store_party(party_index, session.clone());
        }
        outcome
    }

    /// Reaps every lapsed held seat in the party at `party_index`. On `Continues`
    /// the shrunken party is persisted; on `Disbanded` it is deleted.
    pub fn advance_party(&mut self, party_index: usize, now: Tick) -> party::PartyOutcome {
        let (outcome, _events) = party::advance_party(self.party(party_index).clone(), now);
        match &outcome {
            party::PartyOutcome::Continues { session } => {
                self.store_party(party_index, session.clone());
            }
            party::PartyOutcome::Disbanded => {
                self.parties.remove(party_index);
            }
        }
        outcome
    }

    /// Reaps the pending invite at `invite_index` when its lease has lapsed. On
    /// `Pending` it is persisted back; on `Lapsed` it is deleted.
    pub fn advance_invite(&mut self, invite_index: usize, now: Tick) -> party::InviteSweep {
        let (outcome, _events) = party::advance_invite(*self.pending_invite(invite_index), now);
        match &outcome {
            party::InviteSweep::Pending { invite } => {
                let slot = or_abort(
                    self.pending_invites
                        .get_mut(invite_index)
                        .ok_or("no invite slot"),
                );
                *slot = persist(*invite);
            }
            party::InviteSweep::Lapsed => {
                self.pending_invites.remove(invite_index);
            }
        }
        outcome
    }

    /// Distributes one kill's experience across the party at `party_index` over
    /// the caller-built `facts`, then applies each returned award's `gained` to its
    /// member's character through the core leveling service (slot = character
    /// index). Returns each award paired with the growth events the leveling
    /// service produced, so the party path proves the delivery duty like the solo
    /// path: `MemberAward.level_ups` is the party observable, and each award's
    /// `GrowthEvent`s are the applied outcome the host delivers outward.
    pub fn distribute_kill_experience(
        &mut self,
        party_index: usize,
        facts: &[party::MemberFact],
        killer: MemberSlot,
        victim_level: Level,
    ) -> Vec<(MemberAward, Vec<GrowthEvent>)> {
        let party = self.party(party_index).clone();
        // The host owns the account↔slot map, so it resolves the killer's fact by
        // value here (a boundary lookup, `or_abort`-resolved) and hands the rest as
        // `others`; core seeds `Q` with the killer, proving `|Q| >= 1` structurally.
        let killer_fact = *or_abort(
            facts
                .iter()
                .find(|fact| fact.slot == killer)
                .ok_or("killer not among facts"),
        );
        let others: Vec<party::MemberFact> = facts
            .iter()
            .copied()
            .filter(|fact| fact.slot != killer)
            .collect();
        let grid = or_abort(
            self.atlas
                .terrain_grid(killer_fact.map)
                .ok_or("no terrain grid for the killer's map"),
        );
        let awards = party::distribute_kill_experience(
            &party,
            killer_fact,
            &others,
            victim_level,
            &self.atlas,
            grid,
            &mut self.rng,
        );
        awards
            .into_iter()
            .map(|award| {
                let events = self.apply_growth(usize::from(award.slot.0), award.gained);
                (award, events)
            })
            .collect()
    }

    /// Splits one zen pile across the party at `party_index` over the caller-built
    /// `facts`/`wallets`, then credits each qualifier's new balance back to its
    /// character and grounds any over-cap share as a fresh pile — all through the
    /// persist seam.
    pub fn split_zen_pickup(
        &mut self,
        party_index: usize,
        pile: &WorldZen,
        facts: &[party::MemberFact],
        picker: MemberSlot,
        wallets: &[party::SlotWallet],
    ) -> party::ZenSplitResult {
        let party = self.party(party_index).clone();
        // The host splits the picker's fact + wallet out by value (a boundary
        // lookup, `or_abort`-resolved) and keeps the rest co-indexed; core seeds
        // `Q` with the picker, so `|Q| >= 1` is structural, never a runtime guard.
        let index = or_abort(
            facts
                .iter()
                .position(|fact| fact.slot == picker)
                .ok_or("picker not among facts"),
        );
        let picker_fact = *or_abort(facts.get(index).ok_or("picker fact"));
        let picker_wallet = or_abort(wallets.get(index).ok_or("picker wallet")).wallet;
        let others: Vec<party::MemberFact> = facts
            .iter()
            .enumerate()
            .filter_map(|(i, fact)| (i != index).then_some(*fact))
            .collect();
        let other_wallets: Vec<party::SlotWallet> = wallets
            .iter()
            .enumerate()
            .filter_map(|(i, wallet)| (i != index).then_some(*wallet))
            .collect();
        let grid = or_abort(
            self.atlas
                .terrain_grid(picker_fact.map)
                .ok_or("no terrain grid for the picker's map"),
        );
        let result = party::split_zen_pickup(
            pile,
            &party,
            picker_fact,
            picker_wallet,
            &others,
            &other_wallets,
            grid,
        );
        for credit in &result.credits {
            self.set_wallet(usize::from(credit.slot.0), credit.wallet);
        }
        for grounded in &result.to_ground {
            self.ground_zen.push(persist(grounded.clone()));
        }
        result
    }

    /// The mini-game session at `index`.
    #[must_use]
    pub fn mini_session(&self, index: usize) -> &MiniGameSession {
        or_abort(
            self.mini_sessions
                .get(index)
                .ok_or("no mini-game session at index"),
        )
    }

    /// How many mini-game sessions are live.
    #[must_use]
    pub fn mini_session_count(&self) -> usize {
        self.mini_sessions.len()
    }

    /// Opens a fresh mini-game session for `key`, its entry window closing at the
    /// definition's enter-duration past `opened_at` — the close tick is core
    /// arithmetic ([`DurationMs::in_ticks`] over the host cadence), never
    /// re-derived here. Seats the session through the persist seam and returns
    /// its index.
    pub fn open_mini_session(&mut self, key: MiniGameKey, opened_at: Tick) -> usize {
        let handle: MiniGameHandle<'_> = or_abort(
            self.atlas
                .mini_game(key.kind, key.level)
                .ok_or("no resolved mini-game for the key"),
        );
        let closes_at = opened_at + handle.definition.enter_duration.get().in_ticks(host_tick());
        let session = MiniGameSession::open(key, opened_at, closes_at);
        let index = self.mini_sessions.len();
        self.mini_sessions.push(persist(session));
        index
    }

    /// Drives the entry gate for the character at `char_index` into the session
    /// at `session_index` with host-supplied `pk` standing: reads the live
    /// session, entrant, and bag, runs [`minigame::enter_mini_game`] over the
    /// held atlas and stream, and writes the (possibly spent) session, entrant,
    /// and bag back *through* the persist seam. On any rejection the framework
    /// returns them unchanged (reject-before-spend), so the write-back is a
    /// no-op in value. Returns the outcome.
    pub fn enter_mini_session(
        &mut self,
        session_index: usize,
        char_index: usize,
        pk: minigame::PkStanding,
    ) -> minigame::EnterOutcome {
        let session = self.mini_session(session_index).clone();
        let entrant = or_abort(self.characters.get(char_index).ok_or("no entrant")).clone();
        let bag = or_abort(self.inventories.get(char_index).ok_or("no entrant bag")).clone();
        let handle: MiniGameHandle<'_> = or_abort(
            self.atlas
                .mini_game(session.key.kind, session.key.level)
                .ok_or("no resolved mini-game"),
        );
        let (session, entrant, bag, outcome) =
            minigame::enter_mini_game(session, &handle, entrant, bag, pk, &mut self.rng);
        self.store_mini_session(session_index, session);
        let slot = or_abort(self.characters.get_mut(char_index).ok_or("no entrant slot"));
        *slot = persist(entrant);
        let slot = or_abort(self.inventories.get_mut(char_index).ok_or("no bag slot"));
        *slot = persist(bag);
        outcome
    }

    /// Advances the mini-game session at `session_index` to `now`: runs the
    /// core tick machine [`minigame::advance_mini_game`] over the held atlas and
    /// stream (its own deadlines, wave spawns and respawns, the min-player abort,
    /// the empty-roster end, the dispose warp-outs), writes the new session back
    /// *through* the persist seam, and returns the emitted events. The host reads
    /// the next deadline off the returned session's phase to decide when to
    /// advance again — it never re-derives the phase arithmetic.
    pub fn advance_mini_session(&mut self, session_index: usize, now: Tick) -> Vec<MiniGameEvent> {
        let session = self.mini_session(session_index).clone();
        let handle: MiniGameHandle<'_> = or_abort(
            self.atlas
                .mini_game(session.key.kind, session.key.level)
                .ok_or("no resolved mini-game"),
        );
        let (session, events) =
            minigame::advance_mini_game(session, &handle, now, host_tick(), &mut self.rng);
        self.store_mini_session(session_index, session);
        events
    }

    /// Reports a server-attributed kill into the session at `session_index`:
    /// credits `score` to `credit`, removes the slain instance from the session
    /// live-set, and (for a still-open respawning wave) schedules the monster's
    /// own-`respawn_ms` return — all core, over the held atlas. Persists the new
    /// session. `slain`/`credit`/`score` are the host's server-computed kill
    /// facts (invariant 6), never a client claim.
    pub fn report_mini_kill(
        &mut self,
        session_index: usize,
        slain: SessionMonsterId,
        credit: RosterSlot,
        score: Score,
        now: Tick,
    ) {
        let session = self.mini_session(session_index).clone();
        let handle: MiniGameHandle<'_> = or_abort(
            self.atlas
                .mini_game(session.key.kind, session.key.level)
                .ok_or("no resolved mini-game"),
        );
        let session =
            minigame::report_session_kill(session, &handle, slain, credit, score, now, host_tick());
        self.store_mini_session(session_index, session);
    }

    /// Marks the session's server-computed `winner` (a quest delivery / last-man
    /// fact) via [`minigame::finish_event`] and persists it; the next advance
    /// observes the marker and ends the game early.
    pub fn finish_mini_event(&mut self, session_index: usize, winner: RosterSlot) {
        let session = minigame::finish_event(self.mini_session(session_index).clone(), winner);
        self.store_mini_session(session_index, session);
    }

    /// Flips the roster status of `victim` to the bare `Dead` in the session at
    /// `session_index` ([`minigame::report_death`]) and persists it — the roster
    /// side of an in-event death. The 3 s eject clock and every penalty live on
    /// the character's own [`resolve_death`] transition, never here.
    pub fn report_mini_death(&mut self, session_index: usize, victim: RosterSlot) {
        let session = minigame::report_death(self.mini_session(session_index).clone(), victim);
        self.store_mini_session(session_index, session);
    }

    /// Removes `who` from the session's roster ([`minigame::report_leave`]) and
    /// persists it — a voluntary leave or the host-reported exit of an ejected
    /// dead member (forfeits the fee, no reward).
    pub fn report_mini_leave(&mut self, session_index: usize, who: RosterSlot) {
        let session = minigame::report_leave(self.mini_session(session_index).clone(), who);
        self.store_mini_session(session_index, session);
    }

    /// Resolves the death of the character at `char_index` with the mini-game
    /// penalty policy WAIVED — the same [`resolve_death`] transition as any
    /// death, docking no experience and no zen (pin 2). The [`Self::resolve_player_death`]
    /// twin for a death inside an event. Persists the marked-`Dead` character and
    /// returns the death events (a lone `Died`, no docks).
    pub fn resolve_waived_death_of(&mut self, char_index: usize, at: Tick) -> Vec<DeathEvent> {
        let character = or_abort(self.characters.get(char_index).ok_or("no character")).clone();
        let (dead, events) = resolve_death(
            character,
            at,
            host_tick(),
            &self.atlas,
            DeathPenalty::Waived,
        );
        let persisted = persist(dead);
        let slot = or_abort(
            self.characters
                .get_mut(char_index)
                .ok_or("no character slot"),
        );
        *slot = persisted;
        events
    }

    /// Resolves the Ended session's rewards ([`minigame::resolve_rewards`]) and
    /// APPLIES every per-finisher grant through the existing per-character seams
    /// — experience via [`apply_experience`], money via
    /// [`minigame::apply_money_grant`], an item drop via
    /// [`minigame::apply_item_drop_grant`] (seated on the ground) — each written
    /// back *through* the persist seam. A finisher's `RosterSlot(i)` addresses
    /// the character at index `i`. Returns the resolution (its ranked score table
    /// and grant events) for the scenario to read.
    pub fn pay_out_mini_rewards(
        &mut self,
        session_index: usize,
        now: Tick,
    ) -> minigame::RewardOutcome {
        let session = self.mini_session(session_index).clone();
        let handle: MiniGameHandle<'_> = or_abort(
            self.atlas
                .mini_game(session.key.kind, session.key.level)
                .ok_or("no resolved mini-game"),
        );
        let outcome = minigame::resolve_rewards(&session, &handle, host_tick());
        for award in &outcome.awards {
            let char_index = usize::from(award.slot.0);
            for grant in &award.grants {
                self.apply_mini_grant(char_index, grant, now);
            }
        }
        outcome
    }

    /// Credits `amount` back to the character at `char_index` through the
    /// balance-preserving carried-zen seam — the host applying a `FeeRefunded`
    /// decision (the min-player abort's one refund path, pin 4). Persists the
    /// credited character.
    pub fn refund_fee(&mut self, char_index: usize, amount: Zen) {
        let character = or_abort(self.characters.get(char_index).ok_or("no refundee")).clone();
        let refunded = match minigame::apply_money_grant(character, amount) {
            minigame::MoneyGrant::Credited { character }
            | minigame::MoneyGrant::OverCap { character } => character,
        };
        let persisted = persist(refunded);
        let slot = or_abort(
            self.characters
                .get_mut(char_index)
                .ok_or("no refundee slot"),
        );
        *slot = persisted;
    }

    /// Applies one grant decision to the character at `char_index` through its
    /// existing per-character seam, writing the result back through the persist
    /// seam (an item drop is seated on the ground instead). The reward fan-out's
    /// application half.
    fn apply_mini_grant(&mut self, char_index: usize, grant: &minigame::GrantDecision, now: Tick) {
        match grant {
            minigame::GrantDecision::Experience { amount } => {
                let character =
                    or_abort(self.characters.get(char_index).ok_or("no finisher")).clone();
                let (grown, _events) = apply_experience(character, *amount, &self.atlas);
                let persisted = persist(grown);
                let slot = or_abort(
                    self.characters
                        .get_mut(char_index)
                        .ok_or("no finisher slot"),
                );
                *slot = persisted;
            }
            minigame::GrantDecision::Money { amount } => {
                let character =
                    or_abort(self.characters.get(char_index).ok_or("no finisher")).clone();
                let credited = match minigame::apply_money_grant(character, *amount) {
                    minigame::MoneyGrant::Credited { character }
                    | minigame::MoneyGrant::OverCap { character } => character,
                };
                let persisted = persist(credited);
                let slot = or_abort(
                    self.characters
                        .get_mut(char_index)
                        .ok_or("no finisher slot"),
                );
                *slot = persisted;
            }
            minigame::GrantDecision::ItemDrop { group } => {
                let character =
                    or_abort(self.characters.get(char_index).ok_or("no finisher")).clone();
                match minigame::apply_item_drop_grant(
                    &character,
                    group,
                    &self.atlas,
                    now,
                    host_tick(),
                    &mut self.rng,
                ) {
                    minigame::ItemDropGrant::Dropped { item } => {
                        self.ground_items.push(persist(item));
                    }
                    minigame::ItemDropGrant::Nothing => {}
                }
            }
        }
    }

    /// Persists `session` into the mini-game session slot at `session_index`.
    fn store_mini_session(&mut self, session_index: usize, session: MiniGameSession) {
        let slot = or_abort(
            self.mini_sessions
                .get_mut(session_index)
                .ok_or("no mini-game session slot"),
        );
        *slot = persist(session);
    }

    /// Persists `party` into the party slot at `party_index`.
    fn store_party(&mut self, party_index: usize, party: PartySession) {
        let slot = or_abort(self.parties.get_mut(party_index).ok_or("no party slot"));
        *slot = persist(party);
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
            parties: &self.parties,
            pending_invites: &self.pending_invites,
            mini_sessions: &self.mini_sessions,
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

/// A plausible gearless Dark Wizard at the given level and energy, seated at a
/// tile — the spellcaster whose Energy-scaled wizardry span the skill-damage
/// scenarios drive — built the only way a character can be, by deserialising
/// its wire form.
#[must_use]
pub fn dark_wizard(level: u16, energy: u16, at: TileCoord) -> Character {
    let position = or_abort(serde_json::to_value(at.to_world()));
    let json = serde_json::json!({
        "class": "dark_wizard",
        "level": level,
        "experience": 0,
        "stats": {"kind": "standard", "strength": 40, "agility": 40, "vitality": 60, "energy": energy},
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

/// A plausible gearless Magic Gladiator at the given level and tile — the
/// class whose 2/3 warp fraction the travel scenarios exercise — built the
/// only way a character can be, by deserialising its wire form.
#[must_use]
pub fn magic_gladiator(level: u16, at: TileCoord) -> Character {
    let position = or_abort(serde_json::to_value(at.to_world()));
    let json = serde_json::json!({
        "class": "magic_gladiator",
        "level": level,
        "experience": 0,
        "stats": {"kind": "standard", "strength": 90, "agility": 60, "vitality": 60, "energy": 60},
        "unspent_points": 0,
        "zen": 0,
        "placement": {"position": position, "facing": {"x": 1, "y": 0}, "movement": "grounded", "map": 0},
        "vitals": {
            "health": {"current": 400, "max": 400},
            "mana": {"current": 300, "max": 300},
            "ability": {"current": 300, "max": 300}
        }
    });
    or_abort(serde_json::from_value(json))
}

/// A gearless Dark Knight seeded mid-band in its own level's experience band and
/// carrying `zen`, seated on `map` at tile `at` with a drainable health pool —
/// the W-DEATH loop subject. Its experience is read from the real exp curve
/// (the level start plus half the band), so a death docks a real, non-zero 1% of
/// the band; its zen is seeded so the bracket percentage docks too. A knight's
/// defense is agility-derived, so this level-100 knight drains to zero exactly as
/// the level-6 case does — the death is combat-driven, not fabricated. `level`
/// must sit below the cap so a next-level band exists to seed into.
#[must_use]
pub fn dark_knight_in_band(
    atlas: &Atlas,
    level: u16,
    zen: u64,
    map: MapNumber,
    at: TileCoord,
) -> Character {
    let curve = atlas.exp_curve();
    let floor = or_abort(curve.level(level)).total_to_hold().0;
    let next = or_abort(curve.level(level + 1)).total_to_hold().0;
    let experience = floor + (next - floor) / 2;
    let position = or_abort(serde_json::to_value(at.to_world()));
    let map = or_abort(serde_json::to_value(map));
    let json = serde_json::json!({
        "class": "dark_knight",
        "level": level,
        "experience": experience,
        "stats": {"kind": "standard", "strength": 150, "agility": 120, "vitality": 100, "energy": 30},
        "unspent_points": 0,
        "zen": zen,
        "placement": {"position": position, "facing": {"x": 1, "y": 0}, "movement": "grounded", "map": map},
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

/// The number and combat block of the first fighting monster whose defense is
/// at least `min_defense` — the armored kill target the geared-vs-gearless
/// standing gate pins the gearless level-floor on. Re-found from the roster on
/// every run, never a hard-coded number.
#[must_use]
pub fn armored_monster_from(atlas: &Atlas, min_defense: u16) -> (MonsterNumber, MonsterCombat) {
    or_abort(
        atlas
            .monsters()
            .find_map(|definition| match &definition.role {
                MonsterRole::Monster { combat, .. } => (combat.defense >= min_defense
                    && combat.hp > 0)
                    .then_some((definition.number, *combat)),
                MonsterRole::Guard { .. }
                | MonsterRole::Trap { .. }
                | MonsterRole::Npc { .. }
                | MonsterRole::SoccerBall => None,
            })
            .ok_or("the dataset has no fighting monster at that defense"),
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

/// The number of the first damaging skill matching `predicate` — the shared
/// scaffold of the pattern-found finders below: walk the catalog, keep the
/// damaging routes, and abort with `err` when no record matches.
fn find_damaging_skill(
    atlas: &Atlas,
    predicate: impl Fn(&Skill, DamagingSkillRef<'_>) -> bool,
    err: &str,
) -> SkillNumber {
    or_abort(
        atlas
            .skills()
            .find_map(|skill| match route(skill) {
                SkillRouting::Damaging(reference) => {
                    predicate(skill, reference).then_some(skill.number)
                }
                SkillRouting::Buff(_) | SkillRouting::Heal(_) | SkillRouting::Deferred => None,
            })
            .ok_or(err),
    )
}

/// The number of the first non-elemental single-target damaging skill — a clean
/// `DirectHit` carrying a real mana cost, re-found from the shipped catalog on
/// every run (the router is the source, never a hard-coded skill id). Chosen
/// non-elemental so a landed hit inflicts no ailment and triggers no knockback,
/// keeping the kill-chain writeback a pure health drain.
#[must_use]
pub fn direct_hit_skill(atlas: &Atlas) -> SkillNumber {
    find_damaging_skill(
        atlas,
        |skill, reference| {
            matches!(reference.shape(), DamagingSkill::DirectHit)
                && skill.element.is_none()
                && skill.cost.mana > 0
        },
        "the dataset has no non-elemental direct-hit skill",
    )
}

/// The number of the first non-elemental wizardry direct-hit skill — the spell
/// whose Energy-scaled span the wizard-kill gate drives, chosen non-elemental so
/// a landed hit inflicts no ailment and triggers no knockback. Re-found from the
/// catalog, never hard-coded.
#[must_use]
pub fn wizardry_direct_skill(atlas: &Atlas) -> SkillNumber {
    find_damaging_skill(
        atlas,
        |skill, reference| {
            matches!(reference.shape(), DamagingSkill::DirectHit)
                && reference.damage_type() == DamageType::Wizardry
                && skill.element.is_none()
        },
        "the dataset has no non-elemental wizardry direct-hit skill",
    )
}

/// The number of the first lightning-element wizardry direct hit — the
/// jiggling bolt whose landed strike nudges its target within one tile per
/// axis. Re-found from the catalog, never hard-coded.
#[must_use]
pub fn lightning_direct_skill(atlas: &Atlas) -> SkillNumber {
    find_damaging_skill(
        atlas,
        |skill, reference| {
            matches!(reference.shape(), DamagingSkill::DirectHit)
                && skill.element == Some(mu_core::components::element::Element::Lightning)
                && reference.damage_type() == DamageType::Wizardry
        },
        "the dataset has no lightning direct-hit skill",
    )
}

/// The number of the first None-type damaging skill (record 50, the monster
/// Flame of Evil) — the skill whose authored damage is discarded and whose cast
/// lands the floor-times-multiplier scratch. Re-found from the catalog.
#[must_use]
pub fn none_type_skill(atlas: &Atlas) -> SkillNumber {
    find_damaging_skill(
        atlas,
        |_, reference| reference.damage_type() == DamageType::None,
        "the dataset has no None-type damaging skill",
    )
}

/// The number of the Nova release — found by pattern, never hard-coded: the
/// fire-element wizardry caster-circle at the authored r=6 (`radius_x2 == 12`)
/// is uniquely Nova (Evil Spirit is non-elemental; Hellfire/Inferno are r=2/4),
/// a strike whose region is a disc around the caster, so one cast sweeps every
/// seated mob within it.
#[must_use]
pub fn nova_skill(atlas: &Atlas) -> SkillNumber {
    find_damaging_skill(
        atlas,
        |skill, reference| {
            matches!(
                reference.shape(),
                DamagingSkill::Area {
                    geometry: AreaGeometry::CasterCircle { radius_x2: 12 },
                    ..
                }
            ) && skill.damage_type == DamageType::Wizardry
                && skill.element == Some(mu_core::components::element::Element::Fire)
        },
        "the dataset has no nova area skill",
    )
}

/// The number of the Flame release — found by pattern, never hard-coded: the
/// fire-element wizardry AIM circle at the authored r=1 (`radius_x2 == 2`) is
/// uniquely Flame (the other aim circles are lightning / ice / wind / plain),
/// the pinpoint burst whose one-tile disc the AOE-size gate drives.
#[must_use]
pub fn flame_skill(atlas: &Atlas) -> SkillNumber {
    find_damaging_skill(
        atlas,
        |skill, reference| {
            matches!(
                reference.shape(),
                DamagingSkill::Area {
                    geometry: AreaGeometry::AimCircle { radius_x2: 2 },
                    ..
                }
            ) && skill.damage_type == DamageType::Wizardry
                && skill.element == Some(mu_core::components::element::Element::Fire)
        },
        "the dataset has no flame aim-circle skill",
    )
}

/// The number of the Hellfire eruption — found by pattern, never hard-coded:
/// the fire-element CASTER circle at the authored r=2 (`radius_x2 == 4`) is
/// uniquely Hellfire (Twisting Slash is the wind r=2 circle; Inferno/Nova are
/// r=4/6), the revived range-0 skill the AOE-size gate drives.
#[must_use]
pub fn hellfire_skill(atlas: &Atlas) -> SkillNumber {
    find_damaging_skill(
        atlas,
        |skill, reference| {
            matches!(
                reference.shape(),
                DamagingSkill::Area {
                    geometry: AreaGeometry::CasterCircle { radius_x2: 4 },
                    ..
                }
            ) && skill.element == Some(mu_core::components::element::Element::Fire)
        },
        "the dataset has no hellfire caster-circle skill",
    )
}

/// The number of the Earthshake quake — found by pattern, never hard-coded:
/// the one area record whose authored displacement is the directional push.
#[must_use]
pub fn earthshake_skill(atlas: &Atlas) -> SkillNumber {
    find_damaging_skill(
        atlas,
        |_, reference| {
            matches!(
                reference.shape(),
                DamagingSkill::Area {
                    displacement: AreaDisplacement::DirectionalPush,
                    ..
                }
            )
        },
        "the dataset has no directional-push area skill",
    )
}

/// The number of the first lunge-shaped skill — found by pattern, never
/// hard-coded: the DK weapon dash whose caster teleports onto its target.
#[must_use]
pub fn lunge_skill(atlas: &Atlas) -> SkillNumber {
    find_damaging_skill(
        atlas,
        |_, reference| matches!(reference.shape(), DamagingSkill::Lunge),
        "the dataset has no lunge skill",
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
/// the service is the compatibility AND eligibility oracle, so no kind→slot or
/// class/requirement rule is re-derived here. When no slot accepts (a
/// non-equippable drop, or one the wearer is ineligible for), the item rides
/// back the last rejection.
fn equip_into_first_slot(
    worn: Equipment,
    item: ItemInstance,
    def: &ItemDefinition,
    atlas: &Atlas,
    wearer: &Wearer,
) -> (Equipment, EquipOutcome) {
    let mut worn = worn;
    let mut item = item;
    let mut last_reason = EquipRejection::IncompatibleSlot;
    for slot in EQUIP_SLOTS {
        let (updated, outcome) = equip(worn, item, def, slot, atlas, wearer);
        match outcome {
            EquipOutcome::Equipped { slot } => return (updated, EquipOutcome::Equipped { slot }),
            EquipOutcome::Rejected {
                item: bounced,
                reason,
            } => {
                worn = updated;
                item = bounced;
                last_reason = reason;
            }
        }
    }
    (
        worn,
        EquipOutcome::Rejected {
            reason: last_reason,
            item,
        },
    )
}

/// The eligibility view a host derives from a seated character before an
/// equip: class, raw level, and total stats (base stats pre-S3 — no worn item
/// grants a stat).
#[must_use]
pub fn wearer_of(character: &Character) -> Wearer {
    Wearer {
        class: character.class(),
        level: character.level(),
        stats: character.stats(),
    }
}

/// Whether the core equip service accepts item `id` into any worn slot FOR
/// `wearer` — the host's "is this drop wearable by this character" test, with
/// the service itself as the slot and eligibility oracle (a jewel or
/// consumable is a `Drop::Item` too, but wearable by none; an elf bow is
/// wearable but not by a knight).
#[must_use]
pub fn is_equippable(atlas: &Atlas, id: ItemRef, wearer: &Wearer) -> bool {
    let def = or_abort(atlas.item(id).ok_or("unknown item"));
    let (_, outcome) = equip_into_first_slot(
        Equipment::empty(),
        item_instance(atlas, id),
        def,
        atlas,
        wearer,
    );
    matches!(outcome, EquipOutcome::Equipped { .. })
}

/// One-tile-per-step movement grain — the speed the step drive method walks a
/// character at, so each [`resolve_step`] lands exactly on the next tile centre.
const ONE_TILE: StepMagnitude = StepMagnitude::ONE_TILE;

/// The host's fixed 50 ms tick cadence — the clock the host owns (U1), fed to
/// every service that converts millisecond durations to absolute ticks (the AI
/// reschedule, effect application and advance). One place decides the cadence.
fn host_tick() -> TickDuration {
    or_abort(TickDuration::new(50))
}

/// The first horizontal run of `length` consecutive walkable tiles on `map`,
/// discovered by scanning the real terrain grid in row-major order — the
/// walkable corridor a grounded character is stepped along, so no step is ever
/// `Blocked` off the run. Returned as tile coordinates left-to-right; the caller
/// walks from the first toward the last. Data-driven, never a hard-coded tile:
/// the run is re-found from the shipped terrain on every run.
#[must_use]
pub fn walkable_run(atlas: &Atlas, map: MapNumber, length: usize) -> Vec<TileCoord> {
    let grid = or_abort(atlas.terrain_grid(map).ok_or("no terrain grid for map"));
    or_abort(first_walkable_run(grid, length).ok_or("no walkable run of that length"))
}

/// Scans `grid` row by row for the first run of `length` consecutive walkable
/// tiles, returning them left-to-right. `None` when no row carries such a run.
fn first_walkable_run(grid: &TerrainGrid, length: usize) -> Option<Vec<TileCoord>> {
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

// --- Mini-game fixtures over the real dataset (found by pattern, never a
// --- hard-coded id or tile). ------------------------------------------------

/// The real Devil Square event-ticket item, found by pattern in the shipped
/// catalog — the only [`ItemKind::EventTicket`] carrying
/// [`EventKind::DevilSquare`]. Never a hard-coded id, so a catalog renumber
/// cannot silently drift the fixture.
#[must_use]
pub fn devil_square_ticket_ref(atlas: &Atlas) -> ItemRef {
    or_abort(
        atlas
            .items()
            .find(|def| {
                matches!(
                    &def.kind,
                    ItemKind::EventTicket {
                        event: EventKind::DevilSquare
                    }
                )
            })
            .map(|def| def.id)
            .ok_or("no Devil Square event ticket in the catalog"),
    )
}

/// A Devil Square ticket instance carrying `charges` at plus-`level` — the entry
/// item a scenario seeds into an entrant's bag. `ticket_ref` is resolved by
/// pattern; only its ref, level, and remaining durability matter to the entry
/// scan.
#[must_use]
pub fn devil_square_ticket(ticket_ref: ItemRef, charges: u8, level: ItemLevel) -> ItemInstance {
    ItemInstance {
        item: ticket_ref,
        level,
        roll: RarityRoll::Normal,
        normal_option: None,
        luck: LuckRoll::Plain,
        skill: SkillRoll::NoSkill,
        durability: or_abort(Durability::new(charges, 5)),
        augment: CraftedAugment::None,
    }
}

/// The first fully-walkable `width`x1 tile strip on `map`, as a [`TileArea`] —
/// an event entrance or wave floor found by scanning the real terrain, so the
/// spawn/landing rectangles are never hard-coded coordinates. A shorter strip
/// still seats the authored quantity: the `Area` placement samples walkable
/// tiles with replacement.
#[must_use]
pub fn walkable_area(atlas: &Atlas, map: MapNumber, width: usize) -> TileArea {
    let run = walkable_run(atlas, map, width);
    let first = *or_abort(run.first().ok_or("empty walkable run"));
    let last = *or_abort(run.last().ok_or("empty walkable run"));
    or_abort(TileArea::new(first.x(), first.y(), last.x(), last.y()))
}

/// A reward-drop group of a single catalog `item` at a fixed plus-`level` — the
/// self-contained group a [`RewardKind::ItemDrop`] rolls at the finisher's feet.
#[must_use]
pub fn reward_drop_group(item: ItemRef, level: ItemLevel) -> RewardDropGroup {
    RewardDropGroup {
        items: or_abort(OneOrMore::new(vec![item])),
        item_level: level,
    }
}

/// One reward-table row: an optional `rank` filter, a success-flag conjunction,
/// and the `reward` payload — the `entry` shorthand the reward scenarios author.
#[must_use]
pub fn reward_entry(rank: Option<u16>, flags: Vec<SuccessFlag>, reward: RewardKind) -> RewardEntry {
    RewardEntry {
        rank: rank.map(Rank),
        flags: or_abort(SuccessFlags::new(flags)),
        reward,
    }
}

/// One authored spawn wave over `area`: its `number`, the game-relative window
/// `[start_ms, end_ms]`, its respawn policy, and `quantity` of `monster`.
#[must_use]
pub fn spawn_wave(
    number: u8,
    start_ms: u32,
    end_ms: u32,
    respawn: WaveRespawn,
    monster: MonsterNumber,
    quantity: u16,
    area: TileArea,
) -> SpawnWave {
    SpawnWave {
        number: WaveNumber(number),
        window: or_abort(Interval::new(DurationMs(start_ms), DurationMs(end_ms))),
        respawn,
        areas: vec![WaveSpawnArea {
            monster,
            area,
            quantity: or_abort(NonZeroU16::new(quantity).ok_or("wave quantity is nonzero")),
        }],
    }
}

/// The `(DevilSquare, level)` key of a [`devil_square_definition`].
#[must_use]
pub fn devil_square_key(level: EventLevel) -> MiniGameKey {
    MiniGameKey {
        kind: MiniGameKind::DevilSquare,
        level,
    }
}

/// A test-authored Devil Square definition over the REAL map-9 terrain at event
/// `level`: normal bracket 15..130, special 10..110, the real DS ticket at +2, a
/// 25,000-zen fee, `min_players`..`max_players`, a 2-minute enter window, a
/// 5-minute game, a 1-minute raw exit (folding to the 30 s floor), its entrance
/// resolved by pattern from the real terrain, and `rewards`/`waves`. Map 9's
/// real town hop (Noria) resolves the alive warp-outs, so the definition passes
/// the Atlas parse proof over the shipped dataset. The generous game span leaves
/// room for authored wave windows to open, overlap, and close well inside it.
#[must_use]
pub fn devil_square_definition(
    atlas: &Atlas,
    level: EventLevel,
    min_players: u16,
    max_players: u16,
    waves: Vec<SpawnWave>,
    rewards: Vec<RewardEntry>,
) -> MiniGameDefinition {
    MiniGameDefinition {
        kind: MiniGameKind::DevilSquare,
        level,
        normal_bracket: or_abort(Interval::new(
            or_abort(Level::new(15)),
            or_abort(Level::new(130)),
        )),
        special_bracket: or_abort(Interval::new(
            or_abort(Level::new(10)),
            or_abort(Level::new(110)),
        )),
        ticket: TicketRequirement {
            item: devil_square_ticket_ref(atlas),
            item_level: or_abort(ItemLevel::new(2)),
        },
        entrance_fee: Zen(25_000),
        players: or_abort(PlayerBounds::new(
            or_abort(NonZeroU16::new(min_players).ok_or("min players is nonzero")),
            or_abort(NonZeroU16::new(max_players).ok_or("max players is nonzero")),
        )),
        enter_duration: PhaseSpan::floored(DurationMs(120_000)),
        game_duration: PhaseSpan::floored(DurationMs(300_000)),
        exit_duration: PhaseSpan::floored_less_countdown(DurationMs(60_000)),
        entrance: EntranceGate {
            map: MapNumber(9),
            area: walkable_area(atlas, MapNumber(9), 8),
        },
        spawn_waves: waves,
        reward_table: rewards,
    }
}

/// A fighting monster carrying a positive respawn delay, found by pattern — the
/// wave populace whose own `MobBehavior.respawn_ms` a respawning wave reuses
/// (ruling D). Returns its number.
#[must_use]
pub fn respawning_wave_monster(atlas: &Atlas) -> MonsterNumber {
    or_abort(
        atlas
            .monsters()
            .find_map(|def| match &def.role {
                MonsterRole::Monster {
                    combat, behavior, ..
                } => (combat.hp > 0 && behavior.respawn_ms.0 > 0).then_some(def.number),
                MonsterRole::Guard { .. }
                | MonsterRole::Trap { .. }
                | MonsterRole::Npc { .. }
                | MonsterRole::SoccerBall => None,
            })
            .ok_or("no fighting monster with a positive respawn delay"),
    )
}
