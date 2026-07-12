//! The W-MINIGAME framework contract, driven cross-file over the real `/data`
//! Atlas: test-authored definitions (no rows ship this era) resolved against
//! the real Devil Square terrain (map 9, its 58-61 spawn-gate rectangles) and
//! the real monster/item rosters, exercising the real services end to end —
//! the full entry-gate matrix, the tick-driven lifecycle walk with its
//! authentic 30 s countdown and min-player abort, the overlapping wave
//! schedule with wave-scoped respawn, the descending-rank reward algebra with
//! its success-flag conjunctions and application seams, and the
//! [`DeathPenalty`] Waived/Applied contrast at the death-service boundary.
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]`
//! body so `unwrap` is exempt.

#[path = "common/dataset.rs"]
mod dataset;
#[path = "common/rng.rs"]
mod rng;

use core::num::NonZeroU16;

use serde_json::{Value, json};

use mu_core::components::active_effect::ActiveEffects;
use mu_core::components::class::CharacterClass;
use mu_core::components::interval::Interval;
use mu_core::components::inventory::{Cell, Footprint, Inventory};
use mu_core::components::item_instance::{
    CraftedAugment, Durability, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
};
use mu_core::components::life::LifeState;
use mu_core::components::tile::{TileArea, TileCoord};
use mu_core::components::units::{
    DurationMs, Exp, ItemLevel, Level, MapNumber, Tick, TickDuration, Ticks, Zen,
};
use mu_core::data::atlas::{Atlas, MiniGameHandle};
use mu_core::data::common::{ItemRef, MonsterNumber};
use mu_core::data::minigame::{
    EntranceGate, EventLevel, MiniGameDefinition, MiniGameKey, MiniGameKind, PhaseSpan,
    PlayerBounds, PlayerCount, Rank, RewardDropGroup, RewardEntry, RewardKind, RosterSlot,
    RosterStatus, Score, SessionMonsterId, SpawnWave, SuccessFlag, SuccessFlags, TicketRequirement,
    WaveNumber, WaveRespawn, WaveSpawnArea, WinnerStanding,
};
use mu_core::entities::character::Character;
use mu_core::entities::minigame_session::{MiniGamePhase, MiniGameSession, RosterMember};
use mu_core::events::death::DeathEvent;
use mu_core::events::minigame::{GrantRecord, MiniGameEvent};
use mu_core::services::death::{DeathPenalty, resolve_death};
use mu_core::services::minigame::{
    EnterOutcome, GrantDecision, ItemDropGrant, MoneyGrant, advance_mini_game,
    apply_item_drop_grant, apply_money_grant, enter_mini_game, finish_event, report_death,
    report_leave, report_session_kill, resolve_rewards,
};

use dataset::{or_abort, real_atlas, real_static_data};
use rng::TestRng;

/// The suite tick cadence: 100 ms, so one second is 10 ticks and one minute
/// 600 — the shipped `game_config.json` cadence.
fn tick() -> TickDuration {
    or_abort(TickDuration::new(100))
}

/// The real Devil's Invitation record — the shipped DS event ticket.
const TICKET_ITEM: ItemRef = ItemRef {
    group: 14,
    number: 19,
};

/// The plus-level the authored definitions demand the ticket at.
fn required_ticket_level() -> ItemLevel {
    or_abort(ItemLevel::new(2))
}

fn event_level() -> EventLevel {
    or_abort(EventLevel::new(3))
}

fn key() -> MiniGameKey {
    MiniGameKey {
        kind: MiniGameKind::DevilSquare,
        level: event_level(),
    }
}

fn bracket(min: u16, max: u16) -> Interval<Level> {
    or_abort(Interval::new(
        or_abort(Level::new(min)),
        or_abort(Level::new(max)),
    ))
}

fn area(x1: u8, y1: u8, x2: u8, y2: u8) -> TileArea {
    or_abort(TileArea::new(x1, y1, x2, y2))
}

/// The real map-9 spawn-gate 58 rectangle — fully walkable, the entrance.
fn entrance_area() -> TileArea {
    area(133, 91, 141, 99)
}

/// The real map-9 spawn-gate 59 rectangle — fully walkable, a wave floor.
fn wave_area() -> TileArea {
    area(135, 162, 142, 170)
}

/// One authored spawn wave over the real square.
fn wave(
    number: u8,
    start_ms: u32,
    end_ms: u32,
    respawn: WaveRespawn,
    monster: u16,
    quantity: u16,
    floor: TileArea,
) -> SpawnWave {
    SpawnWave {
        number: WaveNumber(number),
        window: or_abort(Interval::new(DurationMs(start_ms), DurationMs(end_ms))),
        respawn,
        areas: vec![WaveSpawnArea {
            monster: MonsterNumber(monster),
            area: floor,
            quantity: or_abort(NonZeroU16::new(quantity).ok_or("quantity is nonzero")),
        }],
    }
}

/// A test-authored definition over the REAL Devil Square (map 9): normal
/// bracket 15..130, special 10..110, the real DS ticket at +2, a 25,000-zen
/// fee, a 5-minute enter window, a 20-minute game, and a 3-minute raw exit
/// (folding to 2 min 30 s).
fn definition(
    min_players: u16,
    max_players: u16,
    waves: Vec<SpawnWave>,
    rewards: Vec<RewardEntry>,
) -> MiniGameDefinition {
    MiniGameDefinition {
        kind: MiniGameKind::DevilSquare,
        level: event_level(),
        normal_bracket: bracket(15, 130),
        special_bracket: bracket(10, 110),
        ticket: TicketRequirement {
            item: TICKET_ITEM,
            item_level: required_ticket_level(),
        },
        entrance_fee: Zen(25_000),
        players: or_abort(PlayerBounds::new(
            or_abort(NonZeroU16::new(min_players).ok_or("min is nonzero")),
            or_abort(NonZeroU16::new(max_players).ok_or("max is nonzero")),
        )),
        enter_duration: PhaseSpan::floored(DurationMs(300_000)),
        game_duration: PhaseSpan::floored(DurationMs(1_200_000)),
        exit_duration: PhaseSpan::floored_less_countdown(DurationMs(180_000)),
        entrance: EntranceGate {
            map: MapNumber(9),
            area: entrance_area(),
        },
        spawn_waves: waves,
        reward_table: rewards,
    }
}

/// The real dataset with the authored definitions loaded — the whole
/// cross-file resolution (entrance landing over real terrain, the Noria town
/// hop, wave monsters joined from the real roster) runs at parse.
fn atlas_with(definitions: Vec<MiniGameDefinition>) -> Atlas {
    let mut data = real_static_data();
    data.mini_games.records = definitions;
    or_abort(Atlas::parse(data))
}

/// The authored definition's resolved handle.
fn handle(atlas: &Atlas) -> MiniGameHandle<'_> {
    or_abort(
        atlas
            .mini_game(MiniGameKind::DevilSquare, event_level())
            .ok_or("the authored definition resolves"),
    )
}

/// A session opened at tick 0 whose 5-minute enter window closes at tick 3000
/// — the authored enter duration at the 100 ms cadence.
fn open_session() -> MiniGameSession {
    MiniGameSession::open(key(), Tick(0), Tick(3000))
}

/// A gearless character built the only way one can be — by deserialising its
/// wire form — of `class` at `level`, carrying `zen`, standing on the square.
fn character(class: CharacterClass, level: u16, zen: u64, effects: &Value) -> Character {
    let stats = if class.has_command() {
        json!({
            "kind": "with_command",
            "strength": 30, "agility": 30, "vitality": 30, "energy": 30, "command": 30
        })
    } else {
        json!({
            "kind": "standard",
            "strength": 30, "agility": 30, "vitality": 30, "energy": 30
        })
    };
    or_abort(serde_json::from_value(json!({
        "class": or_abort(serde_json::to_value(class)),
        "level": level,
        "experience": 0,
        "stats": stats,
        "unspent_points": 0,
        "zen": zen,
        "placement": {
            "position": or_abort(serde_json::to_value(TileCoord::new(137, 95).to_world())),
            "facing": {"x": 0, "y": 1},
            "movement": "grounded",
            "map": 9
        },
        "vitals": {
            "health": {"current": 100, "max": 100},
            "mana": {"current": 50, "max": 50},
            "ability": {"current": 20, "max": 20}
        },
        "active_effects": effects,
    })))
}

fn no_effects() -> Value {
    json!([])
}

fn a_buff() -> Value {
    json!([{"kind": "defense", "expiry": 9999}])
}

/// A Devil's Invitation instance at `level` carrying `charges` entries.
fn ticket(charges: u8, level: ItemLevel) -> ItemInstance {
    ItemInstance {
        item: TICKET_ITEM,
        level,
        roll: RarityRoll::Normal,
        normal_option: None,
        luck: LuckRoll::Plain,
        skill: SkillRoll::NoSkill,
        durability: or_abort(Durability::new(charges, 5)),
        augment: CraftedAugment::None,
    }
}

/// A bag holding exactly `instance` at its origin cell.
fn bag_with(instance: ItemInstance) -> Inventory {
    or_abort(
        Inventory::empty(8, 8)
            .place(
                Cell { row: 0, col: 0 },
                or_abort(Footprint::new(1, 1)),
                instance,
            )
            .map_err(|(_, _, rejection)| format!("the fixture bag has room: {rejection:?}")),
    )
}

/// The baseline entrant every admission test seeds: a level-60 Dark Knight
/// carrying 100,000 zen and one buffed effect.
fn entrant() -> Character {
    character(CharacterClass::DarkKnight, 60, 100_000, &a_buff())
}

/// Admits the baseline entrant with a fresh 2-charge ticket, or aborts — the
/// roster-seeding shortcut for the lifecycle tests.
fn admit(
    session: MiniGameSession,
    handle: &MiniGameHandle<'_>,
    seed: u64,
) -> (MiniGameSession, Character, RosterSlot) {
    let entrant = entrant();
    let bag = bag_with(ticket(2, required_ticket_level()));
    let mut rng = TestRng::new(seed);
    let (session, admitted, _bag, outcome) =
        enter_mini_game(session, handle, entrant, bag, &mut rng);
    let EnterOutcome::Entered { slot, .. } = outcome else {
        return or_abort(Err(format!("expected admission, got {outcome:?}")));
    };
    (session, admitted, slot)
}

/// One advance under a fresh seeded stream.
fn advance(
    session: MiniGameSession,
    handle: &MiniGameHandle<'_>,
    now: u64,
    seed: u64,
) -> (MiniGameSession, Vec<MiniGameEvent>) {
    let mut rng = TestRng::new(seed);
    advance_mini_game(session, handle, Tick(now), tick(), &mut rng)
}

/// A session already in its Ended phase over authored roster members —
/// `(slot, status, score)` — with `remaining` whole ticks left at end.
fn ended(
    members: &[(u8, RosterStatus, u32)],
    remaining: u64,
    winner: WinnerStanding,
) -> MiniGameSession {
    let mut session = open_session();
    for (slot, status, score) in members {
        session = session.with_member(RosterMember {
            slot: RosterSlot(*slot),
            status: *status,
            score: Score(*score),
        });
    }
    session
        .with_winner(winner)
        .with_phase(MiniGamePhase::Ended {
            disposes_at: Tick(999_999),
            snapshot: PlayerCount(or_abort(u16::try_from(members.len()))),
            remaining: Ticks(remaining),
        })
}

fn entry(rank: Option<u16>, flags: Vec<SuccessFlag>, reward: RewardKind) -> RewardEntry {
    RewardEntry {
        rank: rank.map(Rank),
        flags: or_abort(SuccessFlags::new(flags)),
        reward,
    }
}

// --- The entry-gate matrix (consult §2; E1/E2/E3) over the real square.

#[test]
fn a_funded_ticketed_in_bracket_entrant_is_admitted_paying_exactly_once() {
    let atlas = atlas_with(vec![definition(2, 3, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    let bag = bag_with(ticket(2, required_ticket_level()));
    let mut rng = TestRng::new(7);
    let (session, admitted, bag, outcome) =
        enter_mini_game(open_session(), &handle, entrant(), bag, &mut rng);
    let EnterOutcome::Entered { slot, placement } = outcome else {
        panic!("expected admission, got {outcome:?}");
    };
    // The ordered side effects: fee debited once, one charge consumed, every
    // effect cleared, warped onto the real entrance landing.
    assert_eq!(slot, RosterSlot(0));
    assert_eq!(admitted.zen().get(), 75_000);
    assert_eq!(bag.placed().len(), 1);
    assert_eq!(bag.placed()[0].item.durability.current(), 1);
    assert_eq!(admitted.active_effects(), ActiveEffects::EMPTY);
    assert_eq!(placement.map, MapNumber(9));
    assert!(entrance_area().to_world().contains(placement.position));
    let grid = atlas.terrain_grid(MapNumber(9)).unwrap();
    assert!(grid.walkable(placement.position));
    assert_eq!(admitted.placement(), placement);
    let member = session.member(RosterSlot(0)).unwrap();
    assert_eq!(member.status, RosterStatus::Alive);
    assert_eq!(member.score, Score(0));
}

#[test]
fn a_ticket_at_its_last_charge_is_consumed_whole_on_entry() {
    let atlas = atlas_with(vec![definition(2, 3, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    let bag = bag_with(ticket(1, required_ticket_level()));
    let mut rng = TestRng::new(7);
    let (_, admitted, bag, outcome) =
        enter_mini_game(open_session(), &handle, entrant(), bag, &mut rng);
    assert!(matches!(outcome, EnterOutcome::Entered { .. }));
    // The whole item is removed at durability zero (the consume-one seam).
    assert!(bag.placed().is_empty());
    assert_eq!(admitted.zen().get(), 75_000);
}

#[test]
fn bracket_edges_are_inclusive_and_one_past_each_edge_rejects() {
    let atlas = atlas_with(vec![definition(2, 20, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    for (level, expected_entered) in [(15, true), (130, true)] {
        let who = character(CharacterClass::DarkKnight, level, 100_000, &no_effects());
        let mut rng = TestRng::new(1);
        let (_, _, _, outcome) = enter_mini_game(
            open_session(),
            &handle,
            who,
            bag_with(ticket(2, required_ticket_level())),
            &mut rng,
        );
        assert_eq!(
            matches!(outcome, EnterOutcome::Entered { .. }),
            expected_entered,
            "level {level}"
        );
    }
    for (level, expected) in [
        (14, EnterOutcome::LevelTooLow),
        (131, EnterOutcome::LevelTooHigh),
    ] {
        let session = open_session();
        let who = character(CharacterClass::DarkKnight, level, 100_000, &a_buff());
        let bag = bag_with(ticket(2, required_ticket_level()));
        let mut rng = TestRng::new(1);
        let (session_after, who_after, bag_after, outcome) =
            enter_mini_game(session.clone(), &handle, who.clone(), bag.clone(), &mut rng);
        assert_eq!(outcome, expected, "level {level}");
        // Reject-before-spend: everything comes back unchanged.
        assert_eq!(session_after, session);
        assert_eq!(who_after, who);
        assert_eq!(bag_after, bag);
    }
}

#[test]
fn special_classes_are_gated_by_the_reduced_bracket() {
    let atlas = atlas_with(vec![definition(2, 20, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    // Level 112 fits the normal 15..130 but exceeds the special 10..110.
    for (class, expected_special) in [
        (CharacterClass::MagicGladiator, true),
        (CharacterClass::DarkLord, true),
        (CharacterClass::DarkWizard, false),
    ] {
        let who = character(class, 112, 100_000, &no_effects());
        let mut rng = TestRng::new(1);
        let (_, _, _, outcome) = enter_mini_game(
            open_session(),
            &handle,
            who,
            bag_with(ticket(2, required_ticket_level())),
            &mut rng,
        );
        if expected_special {
            assert_eq!(outcome, EnterOutcome::LevelTooHigh, "{class:?}");
        } else {
            assert!(matches!(outcome, EnterOutcome::Entered { .. }), "{class:?}");
        }
    }
    // Level 12 fits the special 10..110 but sits below the normal 15.
    for (class, expected) in [
        (CharacterClass::MagicGladiator, true),
        (CharacterClass::DarkKnight, false),
    ] {
        let who = character(class, 12, 100_000, &no_effects());
        let mut rng = TestRng::new(1);
        let (_, _, _, outcome) = enter_mini_game(
            open_session(),
            &handle,
            who,
            bag_with(ticket(2, required_ticket_level())),
            &mut rng,
        );
        if expected {
            assert!(matches!(outcome, EnterOutcome::Entered { .. }), "{class:?}");
        } else {
            assert_eq!(outcome, EnterOutcome::LevelTooLow, "{class:?}");
        }
    }
}

#[test]
fn a_wrong_level_or_spent_ticket_rejects_with_nothing_spent() {
    let atlas = atlas_with(vec![definition(2, 3, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    let wrong_level = ticket(2, or_abort(ItemLevel::new(1)));
    let spent = ticket(0, required_ticket_level());
    for instance in [wrong_level, spent] {
        let who = entrant();
        let bag = bag_with(instance);
        let mut rng = TestRng::new(1);
        let (_, who_after, bag_after, outcome) =
            enter_mini_game(open_session(), &handle, who.clone(), bag.clone(), &mut rng);
        assert_eq!(outcome, EnterOutcome::NoTicket);
        assert_eq!(who_after, who);
        assert_eq!(bag_after, bag);
    }
}

#[test]
fn an_unaffordable_fee_is_checked_never_partially_charged() {
    let atlas = atlas_with(vec![definition(2, 3, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    let poor = character(CharacterClass::DarkKnight, 60, 24_999, &a_buff());
    let bag = bag_with(ticket(2, required_ticket_level()));
    let mut rng = TestRng::new(1);
    let (_, who_after, bag_after, outcome) =
        enter_mini_game(open_session(), &handle, poor.clone(), bag.clone(), &mut rng);
    assert_eq!(outcome, EnterOutcome::NotEnoughZen);
    assert_eq!(who_after.zen().get(), 24_999);
    assert_eq!(who_after, poor);
    assert_eq!(bag_after, bag);
}

#[test]
fn a_hunted_entrant_is_barred_and_a_clean_one_enters() {
    let atlas = atlas_with(vec![definition(2, 3, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    let bag = bag_with(ticket(2, required_ticket_level()));
    // A first-stage murderer, seeded through the wire gate — the entry bar
    // reads the entrant's own authoritative reputation, not a host claim.
    let mut wire = or_abort(serde_json::to_value(entrant()));
    wire["reputation"] = json!({
        "standing": {"kind": "flagged", "stage": "first_stage", "decays_at": 9},
        "kills": 1
    });
    let hunted: Character = or_abort(serde_json::from_value(wire));
    let mut rng = TestRng::new(1);
    let (_, who_after, bag_after, outcome) = enter_mini_game(
        open_session(),
        &handle,
        hunted.clone(),
        bag.clone(),
        &mut rng,
    );
    assert_eq!(outcome, EnterOutcome::PlayerKillerBarred);
    assert_eq!(who_after, hunted);
    assert_eq!(bag_after, bag);
    let mut rng = TestRng::new(1);
    let (_, _, _, outcome) = enter_mini_game(open_session(), &handle, entrant(), bag, &mut rng);
    assert!(matches!(outcome, EnterOutcome::Entered { .. }));
}

#[test]
fn entry_past_the_open_window_is_rejected() {
    let atlas = atlas_with(vec![definition(2, 3, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    let closed = open_session().with_phase(MiniGamePhase::Closing {
        starts_at: Tick(3300),
    });
    let mut rng = TestRng::new(1);
    let (_, _, _, outcome) = enter_mini_game(
        closed,
        &handle,
        entrant(),
        bag_with(ticket(2, required_ticket_level())),
        &mut rng,
    );
    assert_eq!(outcome, EnterOutcome::NotOpen);
}

#[test]
fn capacity_counts_every_entered_member_dead_included() {
    let atlas = atlas_with(vec![definition(2, 3, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    let (session, _, _) = admit(open_session(), &handle, 1);
    let (session, _, _) = admit(session, &handle, 2);
    let (session, _, slot) = admit(session, &handle, 3);
    // A dead member still occupies its seat.
    let session = report_death(session, slot);
    let mut rng = TestRng::new(4);
    let (_, _, _, outcome) = enter_mini_game(
        session,
        &handle,
        entrant(),
        bag_with(ticket(2, required_ticket_level())),
        &mut rng,
    );
    assert_eq!(outcome, EnterOutcome::Full);
}

#[test]
fn the_first_failing_check_in_the_fixed_order_reports() {
    let atlas = atlas_with(vec![definition(2, 3, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    // Fails BOTH the bracket and the ticket check: the bracket wins (E1).
    let too_low = character(CharacterClass::DarkKnight, 14, 100_000, &no_effects());
    let mut rng = TestRng::new(1);
    let (_, _, _, outcome) = enter_mini_game(
        open_session(),
        &handle,
        too_low,
        Inventory::empty(8, 8),
        &mut rng,
    );
    assert_eq!(outcome, EnterOutcome::LevelTooLow);
    // Fails BOTH the ticket and the fee check: the ticket wins.
    let broke = character(CharacterClass::DarkKnight, 60, 0, &no_effects());
    let mut rng = TestRng::new(1);
    let (_, _, _, outcome) = enter_mini_game(
        open_session(),
        &handle,
        broke,
        Inventory::empty(8, 8),
        &mut rng,
    );
    assert_eq!(outcome, EnterOutcome::NoTicket);
}

// --- The lifecycle walk (consult §1; pins 4/6) over the authored windows.

#[test]
fn the_enter_window_broadcasts_each_remaining_minute_then_counts_down_into_the_game() {
    let atlas = atlas_with(vec![definition(2, 3, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    let (session, _, _) = admit(open_session(), &handle, 1);
    let (mut session, _, _) = admit(session, &handle, 2);
    // One EntranceClosing per whole remaining minute, at the minute edge.
    for (now, minutes) in [(0, 5), (600, 4), (1200, 3), (1800, 2), (2400, 1)] {
        let (advanced, events) = advance(session, &handle, now, 9);
        session = advanced;
        assert_eq!(
            events,
            vec![MiniGameEvent::EntranceClosing {
                minutes_left: minutes
            }],
            "now {now}"
        );
    }
    // Between edges nothing fires — each broadcast fires exactly once.
    let (session, events) = advance(session, &handle, 2700, 9);
    assert!(events.is_empty());
    // The entrance closes into the fixed 30 s countdown (min players met).
    let (session, events) = advance(session, &handle, 3000, 9);
    assert_eq!(
        events,
        vec![MiniGameEvent::CountdownStarted { seconds: 30 }]
    );
    assert_eq!(
        session.phase,
        MiniGamePhase::Closing {
            starts_at: Tick(3300)
        }
    );
    assert_eq!(session.start_snapshot(), None);
    // The countdown deadline starts the game, freezing the snapshot onto the
    // Playing variant: 20 minutes at 100 ms is 12,000 ticks.
    let (session, events) = advance(session, &handle, 3300, 9);
    assert_eq!(
        events,
        vec![MiniGameEvent::GameStarted {
            players: PlayerCount(2)
        }]
    );
    assert_eq!(
        session.phase,
        MiniGamePhase::Playing {
            ends_at: Tick(15_300),
            snapshot: PlayerCount(2)
        }
    );
    assert_eq!(session.start_snapshot(), Some(PlayerCount(2)));
}

#[test]
fn too_few_alive_players_at_entrance_close_aborts_with_the_only_fee_refund() {
    let atlas = atlas_with(vec![definition(2, 3, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    let (session, admitted, slot) = admit(open_session(), &handle, 1);
    assert_eq!(admitted.zen().get(), 75_000);
    // One catch-up call to the close: the broadcasts fire, then the
    // min-player gate fails at the Open -> Closing boundary — before the
    // countdown — refunding the fee and disposing.
    let (session, events) = advance(session, &handle, 3000, 9);
    let boundary = events
        .iter()
        .position(|event| matches!(event, MiniGameEvent::MinPlayersAbort { .. }))
        .unwrap();
    assert_eq!(
        events[boundary],
        MiniGameEvent::MinPlayersAbort {
            present: PlayerCount(1),
            required: PlayerCount(2),
        }
    );
    assert_eq!(
        events[boundary + 1],
        MiniGameEvent::FeeRefunded {
            slot,
            amount: Zen(25_000),
        }
    );
    // The lone alive entrant is warped to the real town hop: map 9's
    // respawn_map is Noria (map 3), its landing walkable on Noria terrain.
    let warp = events
        .iter()
        .find_map(|event| match event {
            MiniGameEvent::WarpedOut { to, .. } => Some(*to),
            MiniGameEvent::EntranceClosing { .. }
            | MiniGameEvent::CountdownStarted { .. }
            | MiniGameEvent::GameStarted { .. }
            | MiniGameEvent::MinPlayersAbort { .. }
            | MiniGameEvent::FeeRefunded { .. }
            | MiniGameEvent::WaveStarted { .. }
            | MiniGameEvent::MonsterSpawned { .. }
            | MiniGameEvent::GameEnded { .. }
            | MiniGameEvent::ScoreTable { .. }
            | MiniGameEvent::RewardGranted { .. }
            | MiniGameEvent::Disposed => None,
        })
        .unwrap();
    assert_eq!(warp.map, MapNumber(3));
    let noria = atlas.terrain_grid(MapNumber(3)).unwrap();
    assert!(noria.walkable(warp.position));
    // Neither the countdown nor the game is ever entered.
    assert!(!events.iter().any(|event| matches!(
        event,
        MiniGameEvent::CountdownStarted { .. } | MiniGameEvent::GameStarted { .. }
    )));
    assert_eq!(session.phase, MiniGamePhase::Disposed);
    assert!(session.roster.is_empty());
    assert_eq!(session.start_snapshot(), None);
}

#[test]
fn a_game_runs_to_its_duration_and_the_remaining_roster_finish() {
    let atlas = atlas_with(vec![definition(2, 3, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    let (session, _, _) = admit(open_session(), &handle, 1);
    let (session, _, _) = admit(session, &handle, 2);
    let (session, _, _) = admit(session, &handle, 3);
    let (session, _) = advance(session, &handle, 3300, 9);
    // No early-end trigger: the game ends exactly at its duration.
    let (session, events) = advance(session, &handle, 15_300, 10);
    assert_eq!(
        events,
        vec![MiniGameEvent::GameEnded {
            finishers: vec![RosterSlot(0), RosterSlot(1), RosterSlot(2)],
        }]
    );
    // Exit window: 2 min 30 s folded exit + the 30 s shutdown beat.
    assert_eq!(
        session.phase,
        MiniGamePhase::Ended {
            disposes_at: Tick(17_100),
            snapshot: PlayerCount(3),
            remaining: Ticks(0),
        }
    );
}

#[test]
fn the_exit_window_elapses_warping_only_alive_members_to_the_real_town() {
    let atlas = atlas_with(vec![definition(2, 3, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    let (session, _, _) = admit(open_session(), &handle, 1);
    let (session, _, dead_slot) = admit(session, &handle, 2);
    let (session, _) = advance(session, &handle, 3300, 9);
    // One member dies mid-game and is still present (not yet ejected) at end.
    let session = report_death(session, dead_slot);
    let (session, _) = advance(session, &handle, 15_300, 10);
    let (session, events) = advance(session, &handle, 17_100, 11);
    // Exactly one WarpedOut — the alive member; the dead member's relocation
    // is the host-composed respawn's, never a framework warp.
    let warps: Vec<&MiniGameEvent> = events
        .iter()
        .filter(|event| matches!(event, MiniGameEvent::WarpedOut { .. }))
        .collect();
    assert_eq!(warps.len(), 1);
    let MiniGameEvent::WarpedOut { slot, to } = warps[0] else {
        panic!("filtered to WarpedOut");
    };
    assert_eq!(*slot, RosterSlot(0));
    assert_eq!(to.map, MapNumber(3));
    assert!(
        atlas
            .terrain_grid(MapNumber(3))
            .unwrap()
            .walkable(to.position)
    );
    assert_eq!(events.last(), Some(&MiniGameEvent::Disposed));
    assert_eq!(session.phase, MiniGamePhase::Disposed);
    assert!(session.roster.is_empty());
}

#[test]
fn deadline_arithmetic_saturates_at_the_tick_ceiling() {
    let atlas = atlas_with(vec![definition(2, 3, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    let (session, _, _) = admit(open_session(), &handle, 1);
    let session = session.with_phase(MiniGamePhase::Playing {
        ends_at: Tick(u64::MAX - 100),
        snapshot: PlayerCount(1),
    });
    // The end and the dispose both land in one call: the exit deadline
    // saturates at the ceiling instead of wrapping, and the transitions still
    // fire in order — end first, then the warp-out and the dispose.
    let (session, events) = advance(session, &handle, u64::MAX, 5);
    assert!(matches!(
        events.first(),
        Some(MiniGameEvent::GameEnded { .. })
    ));
    assert_eq!(events.last(), Some(&MiniGameEvent::Disposed));
    assert_eq!(session.phase, MiniGamePhase::Disposed);
}

#[test]
fn the_snapshot_freezes_at_start_dead_included_and_leavers_excluded() {
    let atlas = atlas_with(vec![definition(2, 4, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    let (session, _, _) = admit(open_session(), &handle, 1);
    let (session, _, dead_slot) = admit(session, &handle, 2);
    let (session, _, _) = admit(session, &handle, 3);
    let (session, _, leaver) = admit(session, &handle, 4);
    // A pre-start leaver is removed from the roster before the freeze.
    let session = report_leave(session, leaver);
    let (session, _) = advance(session, &handle, 3000, 9);
    // A death during the countdown keeps its seat: the snapshot counts it.
    let session = report_death(session, dead_slot);
    let (session, events) = advance(session, &handle, 3300, 9);
    assert_eq!(
        events,
        vec![MiniGameEvent::GameStarted {
            players: PlayerCount(3)
        }]
    );
    assert_eq!(session.start_snapshot(), Some(PlayerCount(3)));
    // Post-start attrition never shrinks the frozen count.
    let session = report_leave(session, RosterSlot(2));
    let session = report_death(session, RosterSlot(0));
    let (session, events) = advance(session, &handle, 4000, 9);
    assert_eq!(session.start_snapshot(), Some(PlayerCount(3)));
    // No fee ever returns outside the min-player abort.
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, MiniGameEvent::FeeRefunded { .. }))
    );
}

#[test]
fn an_emptied_roster_ends_the_game_immediately() {
    let atlas = atlas_with(vec![definition(2, 3, Vec::new(), Vec::new())]);
    let handle = handle(&atlas);
    let (session, _, _) = admit(open_session(), &handle, 1);
    let (session, _, _) = admit(session, &handle, 2);
    let (session, _) = advance(session, &handle, 3300, 9);
    // Both members leave mid-game (fees forfeit — the only refund path is
    // the abort); the next advance ends the game with no finishers, well
    // before the 20-minute duration.
    let session = report_leave(session, RosterSlot(0));
    let session = report_leave(session, RosterSlot(1));
    let (session, events) = advance(session, &handle, 5000, 9);
    assert_eq!(
        events,
        vec![MiniGameEvent::GameEnded {
            finishers: Vec::new()
        }]
    );
    let MiniGamePhase::Ended { remaining, .. } = session.phase else {
        panic!("expected Ended, got {:?}", session.phase);
    };
    assert_eq!(remaining, Ticks(10_300));
    assert!(session.monsters.live.is_empty());
    assert!(session.waves.waves.is_empty());
}

// --- The wave schedule (consult §5; rulings C/D) against authored windows.

#[test]
fn a_wave_spawns_its_areas_over_real_terrain_at_window_start() {
    let atlas = atlas_with(vec![definition(
        2,
        3,
        vec![wave(
            1,
            0,
            420_000,
            WaveRespawn::RespawningWhileOpen,
            1,
            4,
            wave_area(),
        )],
        Vec::new(),
    )]);
    let handle = handle(&atlas);
    let (session, _, _) = admit(open_session(), &handle, 1);
    let (session, _, _) = admit(session, &handle, 2);
    // The game-start advance also fires the zero-offset wave.
    let (session, events) = advance(session, &handle, 3300, 9);
    assert!(events.contains(&MiniGameEvent::WaveStarted {
        number: WaveNumber(1)
    }));
    let grid = atlas.terrain_grid(MapNumber(9)).unwrap();
    let spawns: Vec<&MiniGameEvent> = events
        .iter()
        .filter(|event| matches!(event, MiniGameEvent::MonsterSpawned { .. }))
        .collect();
    assert_eq!(spawns.len(), 4);
    for event in spawns {
        let MiniGameEvent::MonsterSpawned { number, at, .. } = event else {
            panic!("filtered to MonsterSpawned");
        };
        assert_eq!(*number, MonsterNumber(1));
        assert!(wave_area().to_world().contains(*at));
        assert!(grid.walkable(*at));
    }
    // The spawns land in the session's OWN instanced live-set, on the square.
    assert_eq!(session.monsters.live.len(), 4);
    assert_eq!(session.monsters.next_id, 4);
    for instanced in &session.monsters.live {
        assert_eq!(instanced.origin, WaveNumber(1));
        assert_eq!(instanced.instance.placement.map, MapNumber(9));
    }
}

#[test]
fn overlapping_windows_run_both_waves_and_respawns_stay_wave_scoped() {
    // Wave 1 over 0..7 min (monster 1, its own real respawn_ms 10 s); wave 2
    // over 5..14 min — the windows overlap between 5 and 7 minutes.
    let atlas = atlas_with(vec![definition(
        2,
        3,
        vec![
            wave(
                1,
                0,
                420_000,
                WaveRespawn::RespawningWhileOpen,
                1,
                2,
                wave_area(),
            ),
            wave(
                2,
                300_000,
                840_000,
                WaveRespawn::RespawningWhileOpen,
                2,
                2,
                area(62, 150, 70, 158),
            ),
        ],
        Vec::new(),
    )]);
    let handle = handle(&atlas);
    let (session, _, _) = admit(open_session(), &handle, 1);
    let (session, _, _) = admit(session, &handle, 2);
    let (session, _) = advance(session, &handle, 3300, 9);
    // At 5 min into the game wave 2 opens while wave 1 is still live.
    let (session, events) = advance(session, &handle, 6300, 10);
    assert!(events.contains(&MiniGameEvent::WaveStarted {
        number: WaveNumber(2)
    }));
    assert_eq!(session.monsters.live.len(), 4);
    // A wave-1 kill at 6 min schedules the monster's OWN 10 s respawn_ms
    // (ruling D — no per-wave delay): due 100 ticks later.
    let slain = session
        .monsters
        .live
        .iter()
        .find(|m| m.origin == WaveNumber(1))
        .unwrap()
        .id;
    let session = report_session_kill(
        session,
        &handle,
        slain,
        RosterSlot(0),
        Score(3),
        Tick(6900),
        tick(),
    );
    assert_eq!(session.monsters.live.len(), 3);
    assert_eq!(session.waves.pending_respawns.len(), 1);
    assert_eq!(session.waves.pending_respawns[0].due, Tick(7000));
    // The window is still open at the due tick: the monster returns.
    let (session, events) = advance(session, &handle, 7000, 11);
    let respawned: Vec<&MiniGameEvent> = events
        .iter()
        .filter(|event| matches!(event, MiniGameEvent::MonsterSpawned { .. }))
        .collect();
    assert_eq!(respawned.len(), 1);
    assert_eq!(session.monsters.live.len(), 4);
    assert!(session.waves.pending_respawns.is_empty());
    // Ids never recycle: the respawned instance takes the next id.
    assert!(
        session
            .monsters
            .live
            .iter()
            .any(|m| m.id == SessionMonsterId(4))
    );
    // A kill at 6 min 55 s is due after wave 1's 7-minute end: dropped.
    let slain = session
        .monsters
        .live
        .iter()
        .find(|m| m.origin == WaveNumber(1))
        .unwrap()
        .id;
    let session = report_session_kill(
        session,
        &handle,
        slain,
        RosterSlot(0),
        Score(3),
        Tick(7450),
        tick(),
    );
    assert_eq!(session.waves.pending_respawns.len(), 1);
    let (session, events) = advance(session, &handle, 7550, 12);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, MiniGameEvent::MonsterSpawned { .. }))
    );
    assert_eq!(session.monsters.live.len(), 3);
    assert!(session.waves.pending_respawns.is_empty());
    // Wave 1 closed at its window end; wave 2 runs on.
    let closed = session
        .waves
        .waves
        .iter()
        .find(|track| track.number == WaveNumber(1))
        .unwrap();
    assert!(matches!(
        closed.state,
        mu_core::entities::minigame_session::WaveState::Closed
    ));
    let running = session
        .waves
        .waves
        .iter()
        .find(|track| track.number == WaveNumber(2))
        .unwrap();
    assert!(matches!(
        running.state,
        mu_core::entities::minigame_session::WaveState::Running { .. }
    ));
}

#[test]
fn a_one_shot_boss_wave_spawns_once_and_never_returns() {
    let atlas = atlas_with(vec![definition(
        2,
        3,
        vec![wave(
            1,
            0,
            420_000,
            WaveRespawn::OnceAtWaveStart,
            1,
            3,
            wave_area(),
        )],
        Vec::new(),
    )]);
    let handle = handle(&atlas);
    let (session, _, _) = admit(open_session(), &handle, 1);
    let (session, _, _) = admit(session, &handle, 2);
    let (session, _) = advance(session, &handle, 3300, 9);
    assert_eq!(session.monsters.live.len(), 3);
    // A one-shot kill schedules nothing.
    let slain = session.monsters.live[0].id;
    let session = report_session_kill(
        session,
        &handle,
        slain,
        RosterSlot(0),
        Score(25),
        Tick(4000),
        tick(),
    );
    assert!(session.waves.pending_respawns.is_empty());
    let (session, events) = advance(session, &handle, 6000, 10);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, MiniGameEvent::MonsterSpawned { .. }))
    );
    assert_eq!(session.monsters.live.len(), 2);
}

#[test]
fn a_wave_area_of_only_blocked_tiles_spawns_zero_without_error() {
    // Map 9's (0,0)..(2,2) corner is fully unwalkable on the real terrain.
    let atlas = atlas_with(vec![definition(
        2,
        3,
        vec![wave(
            1,
            0,
            420_000,
            WaveRespawn::RespawningWhileOpen,
            1,
            4,
            area(0, 0, 2, 2),
        )],
        Vec::new(),
    )]);
    let handle = handle(&atlas);
    let (session, _, _) = admit(open_session(), &handle, 1);
    let (session, _, _) = admit(session, &handle, 2);
    let (session, events) = advance(session, &handle, 3300, 9);
    assert!(events.contains(&MiniGameEvent::WaveStarted {
        number: WaveNumber(1)
    }));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, MiniGameEvent::MonsterSpawned { .. }))
    );
    assert!(session.monsters.live.is_empty());
}

#[test]
fn kills_credit_the_supplied_score_and_stop_scoring_after_the_end() {
    let atlas = atlas_with(vec![definition(
        2,
        3,
        vec![wave(
            1,
            0,
            420_000,
            WaveRespawn::RespawningWhileOpen,
            1,
            3,
            wave_area(),
        )],
        Vec::new(),
    )]);
    let handle = handle(&atlas);
    let (session, _, _) = admit(open_session(), &handle, 1);
    let (session, _, _) = admit(session, &handle, 2);
    let (session, _) = advance(session, &handle, 3300, 9);
    // Two kills across consecutive ticks accumulate on the crediting seat —
    // the score is the caller's server-computed per-game value.
    let first = session.monsters.live[0].id;
    let session = report_session_kill(
        session,
        &handle,
        first,
        RosterSlot(0),
        Score(5),
        Tick(4000),
        tick(),
    );
    let second = session.monsters.live[0].id;
    let session = report_session_kill(
        session,
        &handle,
        second,
        RosterSlot(0),
        Score(5),
        Tick(4001),
        tick(),
    );
    assert_eq!(session.member(RosterSlot(0)).unwrap().score, Score(10));
    assert_eq!(session.member(RosterSlot(1)).unwrap().score, Score(0));
    assert_eq!(session.monsters.live.len(), 1);
    // A double report of the same instance cannot double-score.
    let replay = report_session_kill(
        session.clone(),
        &handle,
        second,
        RosterSlot(0),
        Score(5),
        Tick(4002),
        tick(),
    );
    assert_eq!(replay, session);
    // A kill reported after the game ends scores nothing.
    let (session, _) = advance(session, &handle, 15_300, 10);
    assert!(matches!(session.phase, MiniGamePhase::Ended { .. }));
    let stale = session.monsters.live[0].id;
    let after = report_session_kill(
        session.clone(),
        &handle,
        stale,
        RosterSlot(0),
        Score(5),
        Tick(15_400),
        tick(),
    );
    assert_eq!(after, session);
}

// --- The reward algebra (consult §8; pin 1; E5) and its application seams.

#[test]
fn finishers_rank_by_score_descending_with_stable_slot_ties() {
    let atlas = atlas_with(vec![definition(
        2,
        20,
        Vec::new(),
        vec![entry(
            None,
            Vec::new(),
            RewardKind::Money { amount: Zen(300) },
        )],
    )]);
    let handle = handle(&atlas);
    let session = ended(
        &[
            (0, RosterStatus::Alive, 40),
            (1, RosterStatus::Alive, 90),
            (2, RosterStatus::Alive, 10),
            (3, RosterStatus::Alive, 40),
        ],
        0,
        WinnerStanding::None,
    );
    let outcome = resolve_rewards(&session, &handle, tick());
    let Some(MiniGameEvent::ScoreTable { rows }) = outcome.events.last() else {
        panic!("the score table rides last");
    };
    let ranked: Vec<(u8, u16, u32)> = rows
        .iter()
        .map(|row| (row.slot.0, row.rank.0, row.final_score.0))
        .collect();
    // Highest score is rank 1; the 40-point tie is stable on slot order.
    assert_eq!(ranked, vec![(1, 1, 90), (0, 2, 40), (3, 3, 40), (2, 4, 10)]);
}

#[test]
fn rank_gated_rewards_stay_on_their_ranks() {
    let atlas = atlas_with(vec![definition(
        2,
        20,
        Vec::new(),
        vec![
            entry(
                Some(1),
                Vec::new(),
                RewardKind::Experience { amount: Exp(6000) },
            ),
            entry(
                Some(2),
                Vec::new(),
                RewardKind::Experience { amount: Exp(4000) },
            ),
        ],
    )]);
    let handle = handle(&atlas);
    let session = ended(
        &[(0, RosterStatus::Alive, 40), (1, RosterStatus::Alive, 90)],
        0,
        WinnerStanding::None,
    );
    let outcome = resolve_rewards(&session, &handle, tick());
    // Rank 1 (slot 1, score 90) gets 6000; rank 2 gets 4000; no crossover.
    assert_eq!(outcome.awards.len(), 2);
    assert_eq!(outcome.awards[0].slot, RosterSlot(1));
    assert_eq!(
        outcome.awards[0].grants,
        vec![GrantDecision::Experience { amount: Exp(6000) }]
    );
    assert_eq!(outcome.awards[1].slot, RosterSlot(0));
    assert_eq!(
        outcome.awards[1].grants,
        vec![GrantDecision::Experience { amount: Exp(4000) }]
    );
    let Some(MiniGameEvent::ScoreTable { rows }) = outcome.events.last() else {
        panic!("the score table rides last");
    };
    assert_eq!(rows[0].granted_experience, Exp(6000));
    assert_eq!(rows[1].granted_experience, Exp(4000));
}

#[test]
fn status_flags_gate_rewards_by_the_finishers_fate() {
    let group = RewardDropGroup {
        items: or_abort(mu_core::components::collections::OneOrMore::new(vec![
            ItemRef {
                group: 0,
                number: 0,
            },
        ])),
        item_level: ItemLevel::ZERO,
    };
    let atlas = atlas_with(vec![definition(
        2,
        20,
        Vec::new(),
        vec![
            entry(
                None,
                vec![SuccessFlag::Alive],
                RewardKind::ItemDrop {
                    group: group.clone(),
                },
            ),
            entry(
                None,
                vec![SuccessFlag::Dead],
                RewardKind::Money { amount: Zen(300) },
            ),
        ],
    )]);
    let handle = handle(&atlas);
    let session = ended(
        &[(0, RosterStatus::Alive, 10), (1, RosterStatus::Dead, 20)],
        0,
        WinnerStanding::None,
    );
    let outcome = resolve_rewards(&session, &handle, tick());
    // The dead-but-present finisher ranks first on score and takes ONLY the
    // Dead-gated money; the alive finisher takes ONLY the item drop.
    assert_eq!(outcome.awards[0].slot, RosterSlot(1));
    assert_eq!(
        outcome.awards[0].grants,
        vec![GrantDecision::Money { amount: Zen(300) }]
    );
    assert_eq!(outcome.awards[1].slot, RosterSlot(0));
    assert_eq!(
        outcome.awards[1].grants,
        vec![GrantDecision::ItemDrop { group }]
    );
}

#[test]
fn winner_flags_resolve_against_the_marker_set_by_finish_event() {
    let table = vec![
        entry(
            None,
            vec![SuccessFlag::Winner],
            RewardKind::Money {
                amount: Zen(10_000),
            },
        ),
        entry(
            None,
            vec![SuccessFlag::Loser],
            RewardKind::Money { amount: Zen(300) },
        ),
        entry(
            None,
            vec![SuccessFlag::WinnerExists],
            RewardKind::Score { amount: Score(50) },
        ),
        entry(
            None,
            vec![SuccessFlag::WinnerNotExists],
            RewardKind::Score { amount: Score(600) },
        ),
    ];
    let atlas = atlas_with(vec![definition(2, 3, Vec::new(), table)]);
    let handle = handle(&atlas);
    let (session, _, _) = admit(open_session(), &handle, 1);
    let (session, _, _) = admit(session, &handle, 2);
    let (session, _) = advance(session, &handle, 3300, 9);
    // finish_event outside Playing is meaningless — proven on the way in.
    assert_eq!(
        finish_event(open_session(), RosterSlot(0)).winner,
        WinnerStanding::None
    );
    // The server-computed winner is marked mid-game; the next advance ends
    // the game early with the remaining time frozen.
    let session = finish_event(session, RosterSlot(1));
    assert_eq!(session.winner, WinnerStanding::Won { by: RosterSlot(1) });
    let (session, events) = advance(session, &handle, 14_400, 10);
    assert!(matches!(
        events.first(),
        Some(MiniGameEvent::GameEnded { .. })
    ));
    let outcome = resolve_rewards(&session, &handle, tick());
    // The winner takes the Winner money; every other finisher the Loser
    // money; WinnerExists holds for all; WinnerNotExists for none.
    assert_eq!(outcome.awards[0].slot, RosterSlot(0));
    assert_eq!(
        outcome.awards[0].grants,
        vec![GrantDecision::Money { amount: Zen(300) }]
    );
    assert_eq!(outcome.awards[1].slot, RosterSlot(1));
    assert_eq!(
        outcome.awards[1].grants,
        vec![GrantDecision::Money {
            amount: Zen(10_000)
        }]
    );
    let Some(MiniGameEvent::ScoreTable { rows }) = outcome.events.last() else {
        panic!("the score table rides last");
    };
    for row in rows {
        assert_eq!(row.final_score, Score(50));
    }
    // With no winner, the loser and winner-not-exists rewards apply instead.
    let unwon = ended(
        &[(0, RosterStatus::Alive, 10), (1, RosterStatus::Alive, 20)],
        0,
        WinnerStanding::None,
    );
    let outcome = resolve_rewards(&unwon, &handle, tick());
    for award in &outcome.awards {
        assert_eq!(
            award.grants,
            vec![GrantDecision::Money { amount: Zen(300) }]
        );
    }
    let Some(MiniGameEvent::ScoreTable { rows }) = outcome.events.last() else {
        panic!("the score table rides last");
    };
    let ranked: Vec<(u8, u16, u32)> = rows
        .iter()
        .map(|row| (row.slot.0, row.rank.0, row.final_score.0))
        .collect();
    assert_eq!(ranked, vec![(1, 1, 620), (0, 2, 610)]);
}

#[test]
fn per_remaining_second_experience_floors_seconds_and_skips_on_timeout() {
    let atlas = atlas_with(vec![definition(
        2,
        20,
        Vec::new(),
        vec![entry(
            None,
            Vec::new(),
            RewardKind::ExperiencePerRemainingSecond { amount: Exp(160) },
        )],
    )]);
    let handle = handle(&atlas);
    // 909 ticks at 100 ms is 90.9 s — floored to 90 whole seconds.
    let early = ended(&[(0, RosterStatus::Alive, 10)], 909, WinnerStanding::None);
    let outcome = resolve_rewards(&early, &handle, tick());
    assert_eq!(
        outcome.awards[0].grants,
        vec![GrantDecision::Experience {
            amount: Exp(90 * 160)
        }]
    );
    // A timeout leaves zero remaining seconds: the reward is skipped whole.
    let timeout = ended(&[(0, RosterStatus::Alive, 10)], 0, WinnerStanding::None);
    let outcome = resolve_rewards(&timeout, &handle, tick());
    assert!(outcome.awards[0].grants.is_empty());
    assert!(
        !outcome
            .events
            .iter()
            .any(|event| matches!(event, MiniGameEvent::RewardGranted { .. }))
    );
}

#[test]
fn a_money_grant_over_the_carry_cap_is_reported_not_lost() {
    let near_cap = character(CharacterClass::DarkKnight, 60, 1_999_999_900, &no_effects());
    match apply_money_grant(near_cap, Zen(500_000)) {
        MoneyGrant::OverCap { character } => {
            // Nothing credited, the balance preserved — the grant itself
            // stays recorded in the resolve events.
            assert_eq!(character.zen().get(), 1_999_999_900);
        }
        MoneyGrant::Credited { .. } => panic!("a credit past the cap reports OverCap"),
    }
    let modest = character(CharacterClass::DarkKnight, 60, 100, &no_effects());
    match apply_money_grant(modest, Zen(500_000)) {
        MoneyGrant::Credited { character } => assert_eq!(character.zen().get(), 500_100),
        MoneyGrant::OverCap { .. } => panic!("a fitting credit lands"),
    }
}

#[test]
fn an_item_drop_grant_lands_a_world_item_at_the_finishers_feet() {
    let atlas = real_atlas();
    let finisher = character(CharacterClass::DarkKnight, 60, 0, &no_effects());
    let group = RewardDropGroup {
        items: or_abort(mu_core::components::collections::OneOrMore::new(vec![
            ItemRef {
                group: 0,
                number: 0,
            },
        ])),
        item_level: ItemLevel::ZERO,
    };
    let mut rng = TestRng::new(7);
    match apply_item_drop_grant(&finisher, &group, &atlas, Tick(15_300), tick(), &mut rng) {
        ItemDropGrant::Dropped { item } => {
            // Stamped exactly at the finisher's feet, on the event map.
            assert_eq!(item.position, finisher.placement().position);
            assert_eq!(item.map, finisher.placement().map);
            assert_eq!(
                item.instance.item,
                ItemRef {
                    group: 0,
                    number: 0
                }
            );
            assert!(Tick(15_300).0 < item.despawn.0);
        }
        ItemDropGrant::Nothing => panic!("a stocked catalog pick drops"),
    }
}

#[test]
fn a_score_bonus_lifts_the_final_score_without_re_ranking() {
    let atlas = atlas_with(vec![definition(
        2,
        20,
        Vec::new(),
        vec![entry(
            None,
            vec![SuccessFlag::Alive],
            RewardKind::Score { amount: Score(600) },
        )],
    )]);
    let handle = handle(&atlas);
    // The dead slot 1 out-scored the alive slot 0; only the alive finisher
    // takes the bonus, yet the ranks stay fixed by pre-reward score.
    let session = ended(
        &[(0, RosterStatus::Alive, 40), (1, RosterStatus::Dead, 90)],
        0,
        WinnerStanding::None,
    );
    let outcome = resolve_rewards(&session, &handle, tick());
    let Some(MiniGameEvent::ScoreTable { rows }) = outcome.events.last() else {
        panic!("the score table rides last");
    };
    let ranked: Vec<(u8, u16, u32)> = rows
        .iter()
        .map(|row| (row.slot.0, row.rank.0, row.final_score.0))
        .collect();
    assert_eq!(ranked, vec![(1, 1, 90), (0, 2, 640)]);
    // A score bonus mutates the table, never a character.
    assert!(outcome.awards.iter().all(|award| award.grants.is_empty()));
}

#[test]
fn the_score_table_rides_last_with_one_grant_event_per_applied_reward() {
    let atlas = atlas_with(vec![definition(
        2,
        20,
        Vec::new(),
        vec![
            entry(
                Some(1),
                Vec::new(),
                RewardKind::Experience { amount: Exp(6000) },
            ),
            entry(None, Vec::new(), RewardKind::Money { amount: Zen(300) }),
        ],
    )]);
    let handle = handle(&atlas);
    let session = ended(
        &[(0, RosterStatus::Alive, 40), (1, RosterStatus::Alive, 90)],
        0,
        WinnerStanding::None,
    );
    let outcome = resolve_rewards(&session, &handle, tick());
    // Three applied rewards (rank-1 exp + two moneys) ride as three grant
    // events, then the table closes the stream.
    let grants: Vec<(RosterSlot, GrantRecord)> = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            MiniGameEvent::RewardGranted { slot, grant } => Some((*slot, *grant)),
            MiniGameEvent::EntranceClosing { .. }
            | MiniGameEvent::CountdownStarted { .. }
            | MiniGameEvent::GameStarted { .. }
            | MiniGameEvent::MinPlayersAbort { .. }
            | MiniGameEvent::FeeRefunded { .. }
            | MiniGameEvent::WaveStarted { .. }
            | MiniGameEvent::MonsterSpawned { .. }
            | MiniGameEvent::GameEnded { .. }
            | MiniGameEvent::ScoreTable { .. }
            | MiniGameEvent::WarpedOut { .. }
            | MiniGameEvent::Disposed => None,
        })
        .collect();
    assert_eq!(
        grants,
        vec![
            (RosterSlot(1), GrantRecord::Experience { amount: Exp(6000) }),
            (RosterSlot(1), GrantRecord::Money { amount: Zen(300) }),
            (RosterSlot(0), GrantRecord::Money { amount: Zen(300) }),
        ]
    );
    assert!(matches!(
        outcome.events.last(),
        Some(MiniGameEvent::ScoreTable { .. })
    ));
    assert_eq!(outcome.events.len(), 4);
    // Outside the Ended phase nothing resolves.
    let open = open_session();
    let empty = resolve_rewards(&open, &handle, tick());
    assert!(empty.awards.is_empty());
    assert!(empty.events.is_empty());
}

// --- The DeathPenalty boundary (pins 2/3; ruling A).

#[test]
fn an_in_event_death_waives_the_penalty_the_same_transition_docks_elsewhere() {
    let atlas = real_atlas();
    // Plenty of exp headroom above the level-60 floor and a real zen balance,
    // so the Applied dock is observably non-zero.
    let exp = atlas.exp_curve().level(61).unwrap().total_to_hold().0 - 1;
    let victim: Character = or_abort(serde_json::from_value(json!({
        "class": "dark_knight",
        "level": 60,
        "experience": exp,
        "stats": {"kind": "standard", "strength": 30, "agility": 30, "vitality": 30, "energy": 30},
        "unspent_points": 0,
        "zen": 100_000,
        "placement": {
            "position": or_abort(serde_json::to_value(TileCoord::new(137, 95).to_world())),
            "facing": {"x": 0, "y": 1},
            "movement": "grounded",
            "map": 9
        },
        "vitals": {
            "health": {"current": 0, "max": 100},
            "mana": {"current": 50, "max": 50},
            "ability": {"current": 20, "max": 20}
        },
    })));

    // The mini-game truce: the SAME transition, zero dock, `Died` alone.
    let (waived, events) = resolve_death(
        victim.clone(),
        Tick(100),
        tick(),
        &atlas,
        DeathPenalty::Waived,
    );
    assert_eq!(events.len(), 1);
    let DeathEvent::Died { respawn_at } = events[0] else {
        panic!("a waived death emits Died alone, got {events:?}");
    };
    assert_eq!(waived.life(), LifeState::Dead { respawn_at });
    assert_eq!(waived.experience().0, exp);
    assert_eq!(waived.zen().get(), 100_000);

    // The normal-world contrast: the identical Dead marking, penalties on.
    let (docked, events) = resolve_death(
        victim.clone(),
        Tick(100),
        tick(),
        &atlas,
        DeathPenalty::Applied,
    );
    assert_eq!(docked.life(), LifeState::Dead { respawn_at });
    assert!(docked.experience().0 < exp);
    assert!(docked.zen().get() < 100_000);
    assert!(events.len() > 1);

    // The session sees only the bare roster flip — no clock, no penalty.
    let session = open_session()
        .with_member(RosterMember {
            slot: RosterSlot(0),
            status: RosterStatus::Alive,
            score: Score(7),
        })
        .with_phase(MiniGamePhase::Playing {
            ends_at: Tick(15_300),
            snapshot: PlayerCount(1),
        });
    let session = report_death(session, RosterSlot(0));
    let member = session.member(RosterSlot(0)).unwrap();
    assert_eq!(member.status, RosterStatus::Dead);
    assert_eq!(member.score, Score(7));
    assert_eq!(session.start_snapshot(), Some(PlayerCount(1)));
}

#[test]
fn a_dead_but_present_member_finishes_under_the_dead_flag() {
    let atlas = atlas_with(vec![definition(
        2,
        3,
        Vec::new(),
        vec![
            entry(
                None,
                vec![SuccessFlag::Dead],
                RewardKind::Money { amount: Zen(300) },
            ),
            entry(
                None,
                vec![SuccessFlag::Alive],
                RewardKind::Experience { amount: Exp(6000) },
            ),
        ],
    )]);
    let handle = handle(&atlas);
    let (session, _, _) = admit(open_session(), &handle, 1);
    let (session, _, dead_slot) = admit(session, &handle, 2);
    let (session, _) = advance(session, &handle, 3300, 9);
    // Dying in the final stretch, the game ends before the LifeState eject:
    // the member is still present and finishes classified by the Dead flag.
    let session = report_death(session, dead_slot);
    let (session, events) = advance(session, &handle, 15_300, 10);
    assert_eq!(
        events,
        vec![MiniGameEvent::GameEnded {
            finishers: vec![RosterSlot(0), dead_slot],
        }]
    );
    let outcome = resolve_rewards(&session, &handle, tick());
    let dead_award = outcome
        .awards
        .iter()
        .find(|award| award.slot == dead_slot)
        .unwrap();
    assert_eq!(
        dead_award.grants,
        vec![GrantDecision::Money { amount: Zen(300) }]
    );
    let alive_award = outcome
        .awards
        .iter()
        .find(|award| award.slot == RosterSlot(0))
        .unwrap();
    assert_eq!(
        alive_award.grants,
        vec![GrantDecision::Experience { amount: Exp(6000) }]
    );
}
