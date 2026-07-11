//! Shared white-box fixtures for the mini-game service tests: a deterministic
//! `SplitMix64`, a draw-counting wrapper, and hand-built definitions,
//! resolved handles, characters, tickets, and monsters over synthetic
//! terrain.

use core::num::NonZeroU16;

use rand_core::RngCore;

use crate::components::active_effect::{ActiveEffect, ActiveEffects};
use crate::components::class::CharacterClass;
use crate::components::collections::OneOrMore;
use crate::components::element::PerElement;
use crate::components::interval::Interval;
use crate::components::inventory::{Cell, Footprint, Inventory};
use crate::components::item_instance::{
    CraftedAugment, Durability, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
};
use crate::components::spatial::WorldPos;
use crate::components::tile::{TerrainGrid, TileArea, TileCoord};
use crate::components::units::{
    DurationMs, Exp, ItemLevel, Level, MapNumber, Resistance, Tick, TickDuration, Zen,
};
use crate::data::atlas::{MiniGameHandle, ResolvedWave, ResolvedWaveArea, SpawnGateView};
use crate::data::common::{ItemRef, MonsterNumber, Provenance, SourceVersion};
use crate::data::map_definitions::MapEnvironment;
use crate::data::minigame::{
    EntranceGate, EventLevel, MiniGameDefinition, MiniGameKey, MiniGameKind, PhaseSpan,
    PlayerBounds, RewardEntry, RewardKind, SuccessFlag, SuccessFlags, TicketRequirement,
    WaveNumber, WaveRespawn,
};
use crate::data::monster_definitions::{
    MobBehavior, MonsterAttack, MonsterCombat, MonsterDefinition, MonsterRole, SafezoneDisposition,
};
use crate::entities::character::Character;
use crate::entities::minigame_session::MiniGameSession;

/// Deterministic `SplitMix64` for replayable tests; cast-free extraction of
/// the low 32 bits keeps clippy's cast lints quiet in test code too.
pub(super) struct TestRng {
    state: u64,
}

impl TestRng {
    pub(super) fn new(seed: u64) -> Self {
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

/// A [`TestRng`] that counts every word drawn — the exactly-N-draws probe.
pub(super) struct CountingRng {
    inner: TestRng,
    draws: u64,
}

impl CountingRng {
    pub(super) fn new(seed: u64) -> Self {
        Self {
            inner: TestRng::new(seed),
            draws: 0,
        }
    }

    pub(super) fn draws(&self) -> u64 {
        self.draws
    }
}

impl RngCore for CountingRng {
    fn next_u64(&mut self) -> u64 {
        self.draws += 1;
        self.inner.next_u64()
    }

    fn next_u32(&mut self) -> u32 {
        self.draws += 1;
        self.inner.next_u32()
    }

    fn fill_bytes(&mut self, dst: &mut [u8]) {
        self.draws += 1;
        self.inner.fill_bytes(dst);
    }
}

/// The host cadence every lifecycle test runs at: 100 ms per tick, so one
/// second is 10 ticks and one minute 600.
pub(super) fn tick100() -> TickDuration {
    TickDuration::new(100).unwrap()
}

pub(super) fn key() -> MiniGameKey {
    MiniGameKey {
        kind: MiniGameKind::DevilSquare,
        level: EventLevel::new(3).unwrap(),
    }
}

/// A session opened at tick 100 whose 5-minute enter window closes at 3100.
pub(super) fn open_session() -> MiniGameSession {
    MiniGameSession::open(key(), Tick(100), Tick(3100))
}

pub(super) const TICKET_ITEM: ItemRef = ItemRef {
    group: 14,
    number: 19,
};

pub(super) fn ticket_requirement() -> TicketRequirement {
    TicketRequirement {
        item: TICKET_ITEM,
        item_level: ItemLevel::new(3).unwrap(),
    }
}

/// A valid ticket instance carrying `charges` remaining entries.
pub(super) fn ticket_instance(charges: u8) -> ItemInstance {
    ItemInstance {
        item: TICKET_ITEM,
        level: ItemLevel::new(3).unwrap(),
        roll: RarityRoll::Normal,
        normal_option: None,
        luck: LuckRoll::Plain,
        skill: SkillRoll::NoSkill,
        durability: Durability::new(charges, 5).unwrap(),
        augment: CraftedAugment::None,
    }
}

/// Places one 1x1 item at the bag's origin cell.
pub(super) fn place_ticket(bag: Inventory, instance: ItemInstance) -> Inventory {
    bag.place(
        Cell { row: 0, col: 0 },
        Footprint::new(1, 1).unwrap(),
        instance,
    )
    .unwrap_or_else(|_| panic!("the fixture bag has room at its origin"))
}

fn provenance() -> Provenance {
    Provenance {
        source_version: SourceVersion::V075,
        review: None,
    }
}

/// A fighting monster definition whose own respawn delay is `respawn_ms`.
pub(super) fn monster(number: u16, respawn_ms: u32) -> MonsterDefinition {
    MonsterDefinition {
        number: MonsterNumber(number),
        provenance: provenance(),
        role: MonsterRole::Monster {
            combat: MonsterCombat {
                level: Level::MIN,
                hp: 60,
                min_phys_damage: 0,
                max_phys_damage: 0,
                defense: 0,
                attack_rate: 0,
                defense_rate: 0,
            },
            resistances: PerElement {
                ice: Resistance(0),
                poison: Resistance(0),
                lightning: Resistance(0),
                fire: Resistance(0),
                earth: Resistance(0),
                wind: Resistance(0),
                water: Resistance(0),
            },
            behavior: MobBehavior {
                move_range: 0,
                attack_range: 0,
                view_range: 0,
                move_delay_ms: DurationMs(0),
                attack_delay_ms: DurationMs(0),
                respawn_ms: DurationMs(respawn_ms),
                disposition: SafezoneDisposition::Excluded,
            },
            attack: MonsterAttack::Plain,
        },
    }
}

/// One resolved wave over one area of `quantity` instances of `monster`.
pub(super) fn resolved_wave(
    number: u8,
    start_ms: u32,
    end_ms: u32,
    respawn: WaveRespawn,
    monster: MonsterDefinition,
    area: TileArea,
    quantity: u16,
) -> ResolvedWave {
    let respawn_ms = match monster.role {
        MonsterRole::Monster { behavior, .. }
        | MonsterRole::Guard { behavior, .. }
        | MonsterRole::Trap { behavior, .. } => behavior.respawn_ms,
        MonsterRole::Npc { .. } | MonsterRole::SoccerBall => DurationMs(0),
    };
    ResolvedWave {
        number: WaveNumber(number),
        window: Interval::new(DurationMs(start_ms), DurationMs(end_ms)).unwrap(),
        respawn,
        areas: vec![ResolvedWaveArea {
            monster,
            area,
            quantity: NonZeroU16::new(quantity).unwrap(),
            respawn_ms,
        }],
    }
}

fn bracket(min: u16, max: u16) -> Interval<Level> {
    Interval::new(Level::new(min).unwrap(), Level::new(max).unwrap()).unwrap()
}

/// The baseline definition every service test runs: normal bracket 15..130,
/// special 10..110, a 25,000-zen fee, 2..3 players, 5-minute enter window,
/// 20-minute game, 2 min 30 s exit, and a five-row reward table.
pub(super) fn definition() -> MiniGameDefinition {
    MiniGameDefinition {
        kind: MiniGameKind::DevilSquare,
        level: EventLevel::new(3).unwrap(),
        normal_bracket: bracket(15, 130),
        special_bracket: bracket(10, 110),
        ticket: ticket_requirement(),
        entrance_fee: Zen(25_000),
        players: PlayerBounds::new(NonZeroU16::new(2).unwrap(), NonZeroU16::new(3).unwrap())
            .unwrap(),
        enter_duration: PhaseSpan::floored(DurationMs(300_000)),
        game_duration: PhaseSpan::floored(DurationMs(1_200_000)),
        exit_duration: PhaseSpan::floored_less_countdown(DurationMs(180_000)),
        entrance: EntranceGate {
            map: MapNumber(0),
            area: TileArea::new(10, 10, 12, 12).unwrap(),
        },
        spawn_waves: Vec::new(),
        reward_table: vec![
            RewardEntry {
                rank: Some(crate::data::minigame::Rank(1)),
                flags: SuccessFlags::new(vec![SuccessFlag::Alive]).unwrap(),
                reward: RewardKind::Experience { amount: Exp(6000) },
            },
            RewardEntry {
                rank: None,
                flags: SuccessFlags::NONE,
                reward: RewardKind::Money { amount: Zen(300) },
            },
        ],
    }
}

/// Owned backing storage for a hand-built [`MiniGameHandle`]: the definition,
/// the resolved entrance/town landing sets, synthetic terrain, and the
/// resolved waves. Tests mutate the parts, then borrow a handle.
pub(super) struct HandleFixture {
    pub(super) definition: MiniGameDefinition,
    pub(super) entrance_landing: OneOrMore<WorldPos>,
    pub(super) terrain: TerrainGrid,
    pub(super) town_landing: OneOrMore<WorldPos>,
    pub(super) waves: Vec<ResolvedWave>,
}

impl HandleFixture {
    pub(super) fn handle(&self) -> MiniGameHandle<'_> {
        MiniGameHandle {
            definition: &self.definition,
            entrance_landing: &self.entrance_landing,
            terrain: &self.terrain,
            town: SpawnGateView {
                map: MapNumber(0),
                landing: &self.town_landing,
                facing: None,
            },
            town_env: MapEnvironment::Ground,
            waves: &self.waves,
        }
    }
}

/// The baseline fixture: everything walkable, the entrance landing at the
/// definition's gate tiles, the town landing at (200, 200).
pub(super) fn fixture() -> HandleFixture {
    HandleFixture {
        definition: definition(),
        entrance_landing: OneOrMore::new(vec![
            TileCoord::new(10, 10).to_world(),
            TileCoord::new(11, 10).to_world(),
            TileCoord::new(12, 10).to_world(),
        ])
        .unwrap(),
        terrain: TerrainGrid::from_words([u64::MAX; 1024]),
        town_landing: OneOrMore::new(vec![TileCoord::new(200, 200).to_world()]).unwrap(),
        waves: Vec::new(),
    }
}

/// A character of `class` at `level` carrying `zen`, standing on map 0.
pub(super) fn character(class: CharacterClass, level: u16, zen: u64) -> Character {
    let stats = if class.has_command() {
        serde_json::json!({
            "kind": "with_command",
            "strength": 30, "agility": 30, "vitality": 30, "energy": 30, "command": 30
        })
    } else {
        serde_json::json!({
            "kind": "standard",
            "strength": 30, "agility": 30, "vitality": 30, "energy": 30
        })
    };
    let json = serde_json::json!({
        "class": serde_json::to_value(class).unwrap(),
        "level": level,
        "experience": 0,
        "stats": stats,
        "unspent_points": 0,
        "zen": zen,
        "placement": {
            "position": serde_json::to_value(TileCoord::new(10, 10).to_world()).unwrap(),
            "facing": {"x": 0, "y": 1},
            "movement": "grounded",
            "map": 0
        },
        "vitals": {
            "health": {"current": 100, "max": 100},
            "mana": {"current": 50, "max": 50},
            "ability": {"current": 20, "max": 20}
        },
    });
    serde_json::from_value(json).unwrap()
}

/// The same character carrying one live timed effect.
pub(super) fn character_with_effect(class: CharacterClass, level: u16, zen: u64) -> Character {
    let base = character(class, level, zen);
    let effects = ActiveEffects::EMPTY.with(ActiveEffect::Defense {
        expiry: Tick(9_999),
    });
    base.with_effects(effects)
}
