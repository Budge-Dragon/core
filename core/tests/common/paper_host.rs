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

use mu_core::components::active_effect::ActiveEffects;
use mu_core::components::element::PerElement;
use mu_core::components::inventory::Footprint;
use mu_core::components::item_instance::{
    CraftedAugment, Durability, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
};
use mu_core::components::item_ref::ItemRef;
use mu_core::components::movement::Movement;
use mu_core::components::placement::Placement;
use mu_core::components::pool::Pool;
use mu_core::components::spatial::{Facing, WorldPos};
use mu_core::components::tile::TileCoord;
use mu_core::components::units::{CarriedZen, ItemLevel, MapNumber, Resistance, Tick, Zen};
use mu_core::data::atlas::Atlas;
use mu_core::data::common::MonsterNumber;
use mu_core::data::monster_definitions::{MonsterCombat, MonsterRole};
use mu_core::entities::character::Character;
use mu_core::entities::monster_instance::MonsterInstance;
use mu_core::entities::trade_session::TradeSession;
use mu_core::entities::world_item::WorldItem;
use mu_core::entities::world_zen::WorldZen;
use mu_core::events::combat::AttackOutcome;
use mu_core::services::combat::resolve_attack;
use mu_core::services::profile::{character_profile, monster_profile};

use dataset::{or_abort, real_atlas};
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

/// The live sets alone, serialised for a snapshot: the atlas, the stream, and
/// the (fixed-per-run) map are all excluded, so a snapshot is total over the
/// observable persisted state and nothing else.
#[derive(Serialize)]
struct LiveSnapshot<'a> {
    characters: &'a [Character],
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
        Self {
            atlas: real_atlas(),
            rng: TestRng::new(seed),
            map,
            characters: Vec::new(),
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

    /// Seats a character through the persist seam and returns its index.
    pub fn seat_character(&mut self, character: Character) -> usize {
        let index = self.characters.len();
        self.characters.push(persist(character));
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

    /// Serialises exactly the live sets — the atlas, the stream, and the map are
    /// excluded — so a replay divergence cannot hide in an unpersisted field.
    #[must_use]
    pub fn snapshot(&self) -> String {
        or_abort(serde_json::to_string(&LiveSnapshot {
            characters: &self.characters,
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
