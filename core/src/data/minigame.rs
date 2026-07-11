//! Record shape of `minigame.json` — the shared mini-game framework's
//! definition catalog (Devil Square, Blood Castle, Chaos Castle) plus the
//! event-scoped value vocabulary the session, events, and services share.
//!
//! One record per `(kind, event level)`. Every bound is proven at deserialize
//! (smart constructors and wire mirrors); the phase-duration folds are
//! parse-time invariants computed once with saturating arithmetic, never
//! re-checked at read. The family ships schema-only this era: the file is
//! absent and the Atlas is total over zero records until the per-game waves
//! W-DS/W-BC/W-CC extract the rows.

use core::num::{NonZeroU8, NonZeroU16};
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::components::collections::OneOrMore;
use crate::components::interval::Interval;
use crate::components::tile::TileArea;
use crate::components::units::{DurationMs, Exp, ItemLevel, Level, Zen};

use super::common::{ItemRef, MapNumber, MonsterNumber};

/// The three mini-games the shared framework runs — the framework
/// discriminator, independent of the item `EventKind` (which carries no
/// Chaos Castle). `Ord` for the Atlas key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MiniGameKind {
    /// Devil Square.
    DevilSquare,
    /// Blood Castle.
    BloodCastle,
    /// Chaos Castle.
    ChaosCastle,
}

/// The 1-based event tier (DS/BC/CC level, the definition selector). Distinct
/// from a character [`crate::components::units::Level`]; a mini-game level is
/// a small tier index, never compared against a character level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct EventLevel(NonZeroU8);

impl EventLevel {
    /// Builds an event tier; zero is rejected.
    ///
    /// # Errors
    /// Returns [`MiniGameError::EventLevelZero`] when `value` is zero.
    pub fn new(value: u8) -> Result<Self, MiniGameError> {
        NonZeroU8::new(value)
            .map(Self)
            .ok_or(MiniGameError::EventLevelZero)
    }

    /// The tier value, 1-based.
    #[must_use]
    pub fn get(self) -> u8 {
        self.0.get()
    }
}

impl TryFrom<u8> for EventLevel {
    type Error = MiniGameError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<EventLevel> for u8 {
    fn from(level: EventLevel) -> Self {
        level.0.get()
    }
}

/// The definition key: which `(kind, level)` a session runs. The Atlas index
/// key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MiniGameKey {
    /// The mini-game.
    pub kind: MiniGameKind,
    /// The event tier.
    pub level: EventLevel,
}

/// A spawn-wave number, distinct per definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WaveNumber(
    /// The wave number.
    pub u8,
);

/// A 1-based finishing rank (rank 1 = highest score). Newtype so a reward's
/// rank filter cannot be confused with a level or a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Rank(
    /// The rank, 1-based.
    pub u16,
);

/// A member's positional identity inside one event roster — never a host
/// account id (the party [`crate::components::party::MemberSlot`] grain,
/// re-minted for the event so its bound is the definition's `players.max`,
/// not the party cap of 5). The host owns the account↔slot map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RosterSlot(
    /// The seat index, `0..players.max`.
    pub u8,
);

/// A roster member's liveness — a bare `Alive | Dead` discriminator. The
/// death *clock* lives on the character's
/// [`crate::components::life::LifeState::Dead`]`{respawn_at}`, never
/// duplicated as a roster deadline; the `Dead` variant carries no field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RosterStatus {
    /// Alive on the event map — counts toward the alive start gate, eligible
    /// for `Alive`-gated rewards.
    Alive,
    /// Dead in-event, awaiting the `LifeState` respawn eject. Still occupies
    /// its slot (counts toward capacity), excluded from the alive count, and
    /// — if still present at game end — a `Dead`-flagged finisher.
    Dead,
}

/// A member's accumulated event score. Per-game the credited value is the
/// square level (DS) or 2/1 (CC) — the framework never invents it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Score(
    /// Accumulated score points.
    pub u32,
);

/// Whether a session has a winner — a proper sum, never an `Option`-flag.
/// `Won` carries the winning slot only on that variant; the framework carries
/// the marker, the per-game trigger (BC delivery / CC last-man) sets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WinnerStanding {
    /// No winner yet — `WinnerNotExists` rewards apply, `Winner` /
    /// `WinnerExists` never.
    None,
    /// The session has a winner at this slot.
    Won {
        /// The winning roster slot.
        by: RosterSlot,
    },
}

/// The frozen player-count snapshot: the entered roster size at the Playing
/// transition, dead included, never recomputed. Carried on the Playing/Ended
/// phase variants, read through
/// [`crate::entities::minigame_session::MiniGameSession::start_snapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PlayerCount(
    /// The entered count.
    pub u16,
);

/// A session-local instanced-monster id — a monotonic counter the session
/// assigns (never a host engine id). A server-computed fact the host
/// references when it reports a kill; never reused across respawns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SessionMonsterId(
    /// The session-local id.
    pub u32,
);

/// A `[min, max]` inclusive level window; `min <= max` proven at parse. One
/// per bracket (normal / special).
pub type LevelBracket = Interval<Level>;

/// The ticket a definition requires: an exact item at an exact plus-level. A
/// valid ticket is this item, at this level exactly, with durability > 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketRequirement {
    /// The required ticket item.
    pub item: ItemRef,
    /// The exact required plus-level.
    pub item_level: ItemLevel,
}

/// The player bounds: a nonzero minimum and a nonzero maximum, `max >= min`
/// proven at parse. `min` gates the alive start check; `max` gates capacity
/// (all entered, dead included). Parsed through a wire mirror.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawPlayerBounds", into = "RawPlayerBounds")]
pub struct PlayerBounds {
    min: NonZeroU16,
    max: NonZeroU16,
}

impl PlayerBounds {
    /// Builds the bounds; an inverted pair is rejected.
    ///
    /// # Errors
    /// Returns [`MiniGameError::PlayerBoundsInverted`] when `max < min`.
    pub fn new(min: NonZeroU16, max: NonZeroU16) -> Result<Self, MiniGameError> {
        if max < min {
            return Err(MiniGameError::PlayerBoundsInverted {
                min: min.get(),
                max: max.get(),
            });
        }
        Ok(Self { min, max })
    }

    /// The minimum alive players required to start.
    #[must_use]
    pub fn min(self) -> NonZeroU16 {
        self.min
    }

    /// The roster capacity (all entered, dead included).
    #[must_use]
    pub fn max(self) -> NonZeroU16 {
        self.max
    }
}

/// Wire mirror of [`PlayerBounds`]; edge order checked on the way in.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct RawPlayerBounds {
    min: NonZeroU16,
    max: NonZeroU16,
}

impl TryFrom<RawPlayerBounds> for PlayerBounds {
    type Error = MiniGameError;

    fn try_from(raw: RawPlayerBounds) -> Result<Self, Self::Error> {
        Self::new(raw.min, raw.max)
    }
}

impl From<PlayerBounds> for RawPlayerBounds {
    fn from(bounds: PlayerBounds) -> Self {
        Self {
            min: bounds.min,
            max: bounds.max,
        }
    }
}

/// An already-floored phase span, proven `>= COUNTDOWN` (30 s) at
/// construction — so a sub-floor span is unrepresentable, not guarded. The
/// three folds differ: enter/game are `max(raw, 30 s)`; exit is
/// `max(raw - 30 s, 30 s)` (saturating). All three land `>= 30 s`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(from = "DurationMs", into = "DurationMs")]
pub struct PhaseSpan(DurationMs);

impl PhaseSpan {
    /// The 30 s floor every span shares, and the fixed pre-start countdown.
    pub const COUNTDOWN: DurationMs = DurationMs(30_000);

    /// The enter/game fold: `max(raw, 30 s)`.
    #[must_use]
    pub fn floored(raw: DurationMs) -> Self {
        Self(DurationMs(raw.0.max(Self::COUNTDOWN.0)))
    }

    /// The exit fold: `max(raw - 30 s, 30 s)`, the subtraction saturating so
    /// a sub-30 s exit lands exactly at the 30 s floor, never underflowing.
    #[must_use]
    pub fn floored_less_countdown(raw: DurationMs) -> Self {
        Self(DurationMs(
            raw.0
                .saturating_sub(Self::COUNTDOWN.0)
                .max(Self::COUNTDOWN.0),
        ))
    }

    /// The floored span.
    #[must_use]
    pub const fn get(self) -> DurationMs {
        self.0
    }
}

impl From<DurationMs> for PhaseSpan {
    fn from(raw: DurationMs) -> Self {
        Self::floored(raw)
    }
}

impl From<PhaseSpan> for DurationMs {
    fn from(span: PhaseSpan) -> Self {
        span.0
    }
}

/// The event's arrival gate: a rectangle on a map. Resolved at parse against
/// the map's terrain into a non-empty walkable landing set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntranceGate {
    /// The event map.
    pub map: MapNumber,
    /// The arrival rectangle.
    pub area: TileArea,
}

/// The window a wave is live, in game-relative offsets; `start <= end` proven
/// at parse. Absolute ticks are computed at the Playing transition. Windows
/// may overlap.
pub type WaveWindow = Interval<DurationMs>;

/// Whether a wave's monsters return after death — sustained vs one-shot only.
/// No per-wave delay: a sustained monster respawns after its own
/// `MobBehavior::respawn_ms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WaveRespawn {
    /// Killed wave-monsters respawn while the window is open (DS normal
    /// waves).
    RespawningWhileOpen,
    /// Spawns once at wave start, never respawns (DS bosses).
    OnceAtWaveStart,
}

/// One spawn area of a wave: a monster over a rectangle, in quantity. No
/// facing: `place_spawn`'s `Area` arm draws a random cardinal, so an authored
/// facing would be silently discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaveSpawnArea {
    /// The monster to place.
    pub monster: MonsterNumber,
    /// The spawn rectangle.
    pub area: TileArea,
    /// How many instances (concurrently maintained while respawning).
    pub quantity: NonZeroU16,
}

/// One spawn wave: a distinct number, its window, its respawn policy, and its
/// areas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnWave {
    /// The wave number, distinct per definition (proven at parse).
    pub number: WaveNumber,
    /// The live window, game-relative.
    pub window: WaveWindow,
    /// Sustained vs one-shot.
    pub respawn: WaveRespawn,
    /// The spawn areas fired at wave start.
    pub areas: Vec<WaveSpawnArea>,
}

/// A single required success flag — the conjunction element. This wave
/// carries exactly the framework-evaluable flags; the party flags and the
/// required-kill gate are W-BC's, which break-and-extends this enum and every
/// match on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuccessFlag {
    /// The finisher is alive on the event map at reward time.
    Alive,
    /// The finisher is a still-present dead member (not yet ejected).
    Dead,
    /// The finisher is the winner.
    Winner,
    /// The finisher is not the winner.
    Loser,
    /// A winner exists this game.
    WinnerExists,
    /// No winner exists this game.
    WinnerNotExists,
}

/// The conjunction of required flags for a reward — a total set structure: a
/// reward applies iff every set flag holds. The empty set is the
/// always-applies reward. Wire: an array of `snake_case` flag names; a
/// duplicate is a parse error; the empty array is the legal always-set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<SuccessFlag>", into = "Vec<SuccessFlag>")]
pub struct SuccessFlags {
    alive: bool,
    dead: bool,
    winner: bool,
    loser: bool,
    winner_exists: bool,
    winner_not_exists: bool,
}

impl SuccessFlags {
    /// The always-applies empty set.
    pub const NONE: Self = Self {
        alive: false,
        dead: false,
        winner: false,
        loser: false,
        winner_exists: false,
        winner_not_exists: false,
    };

    /// Builds the set from listed flags; each flag at most once.
    ///
    /// # Errors
    /// Returns [`MiniGameError::DuplicateSuccessFlag`] when a flag repeats.
    pub fn new(flags: Vec<SuccessFlag>) -> Result<Self, MiniGameError> {
        let mut built = Self::NONE;
        for flag in flags {
            let seat = match flag {
                SuccessFlag::Alive => &mut built.alive,
                SuccessFlag::Dead => &mut built.dead,
                SuccessFlag::Winner => &mut built.winner,
                SuccessFlag::Loser => &mut built.loser,
                SuccessFlag::WinnerExists => &mut built.winner_exists,
                SuccessFlag::WinnerNotExists => &mut built.winner_not_exists,
            };
            if *seat {
                return Err(MiniGameError::DuplicateSuccessFlag { flag });
            }
            *seat = true;
        }
        Ok(built)
    }

    /// The total conjunction test: whether every set flag holds for a
    /// finisher at `slot` with `status`, against the session's `winner`
    /// marker. The empty set always holds.
    #[must_use]
    pub fn holds(self, status: RosterStatus, winner: WinnerStanding, slot: RosterSlot) -> bool {
        let is_alive = matches!(status, RosterStatus::Alive);
        let is_winner = winner == WinnerStanding::Won { by: slot };
        let winner_exists = matches!(winner, WinnerStanding::Won { .. });
        (!self.alive || is_alive)
            && (!self.dead || !is_alive)
            && (!self.winner || is_winner)
            && (!self.loser || !is_winner)
            && (!self.winner_exists || winner_exists)
            && (!self.winner_not_exists || !winner_exists)
    }
}

impl TryFrom<Vec<SuccessFlag>> for SuccessFlags {
    type Error = MiniGameError;

    fn try_from(flags: Vec<SuccessFlag>) -> Result<Self, Self::Error> {
        Self::new(flags)
    }
}

impl From<SuccessFlags> for Vec<SuccessFlag> {
    fn from(set: SuccessFlags) -> Self {
        let mut flags = Self::new();
        if set.alive {
            flags.push(SuccessFlag::Alive);
        }
        if set.dead {
            flags.push(SuccessFlag::Dead);
        }
        if set.winner {
            flags.push(SuccessFlag::Winner);
        }
        if set.loser {
            flags.push(SuccessFlag::Loser);
        }
        if set.winner_exists {
            flags.push(SuccessFlag::WinnerExists);
        }
        if set.winner_not_exists {
            flags.push(SuccessFlag::WinnerNotExists);
        }
        flags
    }
}

/// A self-contained reward drop group: a uniform pick among these items at a
/// fixed plus-level, dropped at the finisher's feet on a match.
/// Self-contained — the framework ships no drop-group catalog. Rolled at
/// application through loot's public group-roll seam, never at resolve time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewardDropGroup {
    /// The uniform-pick item set (non-empty by construction).
    pub items: OneOrMore<ItemRef>,
    /// The plus-level of the dropped item.
    pub item_level: ItemLevel,
}

/// One reward's payload, kind-tagged by type. The amount lives on the variant
/// that needs it. `ExperiencePerRemainingSecond` is folded to a concrete
/// experience amount at resolve time (integer `seconds * amount`, 0 on
/// timeout).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RewardKind {
    /// Flat experience.
    Experience {
        /// The experience granted.
        amount: Exp,
    },
    /// Experience per whole remaining second at finish (`seconds * amount`; 0
    /// when the game timed out).
    ExperiencePerRemainingSecond {
        /// The per-second experience rate.
        amount: Exp,
    },
    /// Zen money.
    Money {
        /// The zen granted.
        amount: Zen,
    },
    /// An item dropped at the finisher's feet.
    ItemDrop {
        /// The drop group rolled on a match.
        group: RewardDropGroup,
    },
    /// A bonus added to the finisher's final score (reflected in the table,
    /// not re-ranking — ranking is by pre-reward score).
    Score {
        /// The bonus score.
        amount: Score,
    },
}

/// One reward-table row: an optional rank filter, a flag conjunction, and the
/// reward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewardEntry {
    /// The rank this reward is gated to; `None` = any rank (genuine
    /// optionality).
    pub rank: Option<Rank>,
    /// The success-flag conjunction; the empty set = always applies.
    pub flags: SuccessFlags,
    /// The reward payload.
    pub reward: RewardKind,
}

/// One mini-game definition — the whole schema for one `(kind, level)`. Data
/// only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawMiniGameDefinition", into = "RawMiniGameDefinition")]
pub struct MiniGameDefinition {
    /// The mini-game.
    pub kind: MiniGameKind,
    /// The event tier.
    pub level: EventLevel,
    /// The default level bracket.
    pub normal_bracket: LevelBracket,
    /// The reduced bracket for MG/DL entrants.
    pub special_bracket: LevelBracket,
    /// The entry ticket.
    pub ticket: TicketRequirement,
    /// The zen entrance fee (0 = free).
    pub entrance_fee: Zen,
    /// The player bounds.
    pub players: PlayerBounds,
    /// Effective enter span (folded `max(raw, 30 s)`).
    pub enter_duration: PhaseSpan,
    /// Effective game span (folded `max(raw, 30 s)`).
    pub game_duration: PhaseSpan,
    /// Effective exit span (folded `max(raw - 30 s, 30 s)`).
    pub exit_duration: PhaseSpan,
    /// The arrival gate.
    pub entrance: EntranceGate,
    /// The spawn waves (distinct numbers; windows may overlap).
    pub spawn_waves: Vec<SpawnWave>,
    /// The reward table.
    pub reward_table: Vec<RewardEntry>,
}

/// Wire mirror of [`MiniGameDefinition`]: the enter/game spans fold through
/// [`PhaseSpan`]'s own `max(raw, 30 s)` seam, the exit span carries the raw
/// duration so its distinct `max(raw - 30 s, 30 s)` fold applies here, and a
/// duplicate wave number is rejected.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawMiniGameDefinition {
    kind: MiniGameKind,
    level: EventLevel,
    normal_bracket: LevelBracket,
    special_bracket: LevelBracket,
    ticket: TicketRequirement,
    entrance_fee: Zen,
    players: PlayerBounds,
    enter_duration: PhaseSpan,
    game_duration: PhaseSpan,
    exit_duration: DurationMs,
    entrance: EntranceGate,
    spawn_waves: Vec<SpawnWave>,
    reward_table: Vec<RewardEntry>,
}

impl TryFrom<RawMiniGameDefinition> for MiniGameDefinition {
    type Error = MiniGameError;

    fn try_from(raw: RawMiniGameDefinition) -> Result<Self, Self::Error> {
        let mut numbers = BTreeSet::new();
        for wave in &raw.spawn_waves {
            if !numbers.insert(wave.number) {
                return Err(MiniGameError::DuplicateWaveNumber {
                    number: wave.number,
                });
            }
        }
        Ok(Self {
            kind: raw.kind,
            level: raw.level,
            normal_bracket: raw.normal_bracket,
            special_bracket: raw.special_bracket,
            ticket: raw.ticket,
            entrance_fee: raw.entrance_fee,
            players: raw.players,
            enter_duration: raw.enter_duration,
            game_duration: raw.game_duration,
            exit_duration: PhaseSpan::floored_less_countdown(raw.exit_duration),
            entrance: raw.entrance,
            spawn_waves: raw.spawn_waves,
            reward_table: raw.reward_table,
        })
    }
}

impl From<MiniGameDefinition> for RawMiniGameDefinition {
    fn from(definition: MiniGameDefinition) -> Self {
        Self {
            kind: definition.kind,
            level: definition.level,
            normal_bracket: definition.normal_bracket,
            special_bracket: definition.special_bracket,
            ticket: definition.ticket,
            entrance_fee: definition.entrance_fee,
            players: definition.players,
            enter_duration: definition.enter_duration,
            game_duration: definition.game_duration,
            // The stored exit span already absorbed the -30 s fold; writing
            // back the raw it canonicalizes to keeps the fold idempotent
            // across round-trips.
            exit_duration: DurationMs(
                definition
                    .exit_duration
                    .get()
                    .0
                    .saturating_add(PhaseSpan::COUNTDOWN.0),
            ),
            entrance: definition.entrance,
            spawn_waves: definition.spawn_waves,
            reward_table: definition.reward_table,
        }
    }
}

/// Why a mini-game record does not parse — the schema's 1:1 parse-error enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiniGameError {
    /// An event tier of zero (tiers are 1-based).
    EventLevelZero,
    /// Player bounds whose minimum exceeds the maximum.
    PlayerBoundsInverted {
        /// The rejected minimum.
        min: u16,
        /// The rejected maximum.
        max: u16,
    },
    /// Two spawn waves of one definition share a number.
    DuplicateWaveNumber {
        /// The repeated wave number.
        number: WaveNumber,
    },
    /// A success-flag conjunction lists a flag twice.
    DuplicateSuccessFlag {
        /// The repeated flag.
        flag: SuccessFlag,
    },
}

impl core::fmt::Display for MiniGameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EventLevelZero => write!(f, "event level zero (tiers are 1-based)"),
            Self::PlayerBoundsInverted { min, max } => {
                write!(f, "player bounds min {min} exceeds max {max}")
            }
            Self::DuplicateWaveNumber { number } => {
                write!(f, "duplicate wave number {number:?}")
            }
            Self::DuplicateSuccessFlag { flag } => {
                write!(f, "duplicate success flag {flag:?}")
            }
        }
    }
}

impl core::error::Error for MiniGameError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn bracket(min: u16, max: u16) -> LevelBracket {
        Interval::new(Level::new(min).unwrap(), Level::new(max).unwrap()).unwrap()
    }

    fn bounds(min: u16, max: u16) -> PlayerBounds {
        PlayerBounds::new(NonZeroU16::new(min).unwrap(), NonZeroU16::new(max).unwrap()).unwrap()
    }

    fn wave(number: u8, start_ms: u32, end_ms: u32, respawn: WaveRespawn) -> SpawnWave {
        SpawnWave {
            number: WaveNumber(number),
            window: Interval::new(DurationMs(start_ms), DurationMs(end_ms)).unwrap(),
            respawn,
            areas: vec![WaveSpawnArea {
                monster: MonsterNumber(17),
                area: TileArea::new(10, 10, 20, 20).unwrap(),
                quantity: NonZeroU16::new(35).unwrap(),
            }],
        }
    }

    fn definition() -> MiniGameDefinition {
        MiniGameDefinition {
            kind: MiniGameKind::DevilSquare,
            level: EventLevel::new(3).unwrap(),
            normal_bracket: bracket(15, 130),
            special_bracket: bracket(10, 110),
            ticket: TicketRequirement {
                item: ItemRef {
                    group: 14,
                    number: 19,
                },
                item_level: ItemLevel::new(3).unwrap(),
            },
            entrance_fee: Zen(25_000),
            players: bounds(2, 20),
            enter_duration: PhaseSpan::floored(DurationMs(60_000)),
            game_duration: PhaseSpan::floored(DurationMs(1_200_000)),
            exit_duration: PhaseSpan::floored_less_countdown(DurationMs(180_000)),
            entrance: EntranceGate {
                map: MapNumber(0),
                area: TileArea::new(120, 120, 126, 126).unwrap(),
            },
            spawn_waves: vec![
                wave(1, 0, 420_000, WaveRespawn::RespawningWhileOpen),
                wave(2, 300_000, 840_000, WaveRespawn::OnceAtWaveStart),
            ],
            reward_table: vec![
                RewardEntry {
                    rank: Some(Rank(1)),
                    flags: SuccessFlags::new(vec![SuccessFlag::Alive]).unwrap(),
                    reward: RewardKind::Experience { amount: Exp(6000) },
                },
                RewardEntry {
                    rank: Some(Rank(2)),
                    flags: SuccessFlags::NONE,
                    reward: RewardKind::ExperiencePerRemainingSecond { amount: Exp(160) },
                },
                RewardEntry {
                    rank: None,
                    flags: SuccessFlags::new(vec![SuccessFlag::Dead]).unwrap(),
                    reward: RewardKind::Money { amount: Zen(300) },
                },
                RewardEntry {
                    rank: None,
                    flags: SuccessFlags::new(vec![SuccessFlag::Winner]).unwrap(),
                    reward: RewardKind::ItemDrop {
                        group: RewardDropGroup {
                            items: OneOrMore::new(vec![
                                ItemRef {
                                    group: 12,
                                    number: 15,
                                },
                                ItemRef {
                                    group: 14,
                                    number: 13,
                                },
                            ])
                            .unwrap(),
                            item_level: ItemLevel::ZERO,
                        },
                    },
                },
                RewardEntry {
                    rank: None,
                    flags: SuccessFlags::new(vec![SuccessFlag::WinnerNotExists]).unwrap(),
                    reward: RewardKind::Score { amount: Score(600) },
                },
            ],
        }
    }

    #[test]
    fn event_level_rejects_zero_and_round_trips_as_a_bare_integer() {
        assert_eq!(EventLevel::new(0), Err(MiniGameError::EventLevelZero));
        assert!(serde_json::from_str::<EventLevel>("0").is_err());
        let level = EventLevel::new(3).unwrap();
        assert_eq!(level.get(), 3);
        assert_eq!(serde_json::to_string(&level).unwrap(), "3");
        assert_eq!(serde_json::from_str::<EventLevel>("3").unwrap(), level);
    }

    #[test]
    fn mini_game_key_round_trips_with_a_snake_case_kind() {
        let key = MiniGameKey {
            kind: MiniGameKind::DevilSquare,
            level: EventLevel::new(3).unwrap(),
        };
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, r#"{"kind":"devil_square","level":3}"#);
        assert_eq!(serde_json::from_str::<MiniGameKey>(&json).unwrap(), key);
    }

    #[test]
    fn player_bounds_reject_inversion_and_zero() {
        assert_eq!(
            PlayerBounds::new(NonZeroU16::new(5).unwrap(), NonZeroU16::new(2).unwrap()),
            Err(MiniGameError::PlayerBoundsInverted { min: 5, max: 2 })
        );
        assert!(serde_json::from_str::<PlayerBounds>(r#"{"min":5,"max":2}"#).is_err());
        assert!(serde_json::from_str::<PlayerBounds>(r#"{"min":0,"max":2}"#).is_err());
        let parsed = serde_json::from_str::<PlayerBounds>(r#"{"min":2,"max":20}"#).unwrap();
        assert_eq!(parsed.min().get(), 2);
        assert_eq!(parsed.max().get(), 20);
        // A single-seat event is legal: min == max.
        assert!(serde_json::from_str::<PlayerBounds>(r#"{"min":1,"max":1}"#).is_ok());
    }

    #[test]
    fn level_bracket_rejects_inversion_and_both_edges_are_inclusive() {
        let window = bracket(15, 130);
        assert!(window.contains(Level::new(15).unwrap()));
        assert!(window.contains(Level::new(130).unwrap()));
        assert!(!window.contains(Level::new(14).unwrap()));
        assert!(!window.contains(Level::new(131).unwrap()));
        assert!(Interval::new(Level::new(130).unwrap(), Level::new(15).unwrap()).is_err());
    }

    #[test]
    fn phase_span_folds_are_computed_once_with_saturating_arithmetic() {
        assert_eq!(PhaseSpan::COUNTDOWN, DurationMs(30_000));
        // Enter/game fold: max(raw, 30 s).
        assert_eq!(
            PhaseSpan::floored(DurationMs(60_000)).get(),
            DurationMs(60_000)
        );
        assert_eq!(
            PhaseSpan::floored(DurationMs(10_000)).get(),
            DurationMs(30_000)
        );
        assert_eq!(
            PhaseSpan::floored(DurationMs(30_000)).get(),
            DurationMs(30_000)
        );
        // Exit fold: max(raw - 30 s, 30 s), saturating.
        assert_eq!(
            PhaseSpan::floored_less_countdown(DurationMs(180_000)).get(),
            DurationMs(150_000)
        );
        assert_eq!(
            PhaseSpan::floored_less_countdown(DurationMs(20_000)).get(),
            DurationMs(30_000)
        );
        assert_eq!(
            PhaseSpan::floored_less_countdown(DurationMs(0)).get(),
            DurationMs(30_000)
        );
    }

    #[test]
    fn phase_span_wire_applies_the_enter_fold_on_the_way_in() {
        let folded = serde_json::from_str::<PhaseSpan>("10000").unwrap();
        assert_eq!(folded.get(), DurationMs(30_000));
        assert_eq!(serde_json::to_string(&folded).unwrap(), "30000");
        let kept = serde_json::from_str::<PhaseSpan>("60000").unwrap();
        assert_eq!(serde_json::to_string(&kept).unwrap(), "60000");
    }

    #[test]
    fn definition_round_trips_with_snake_case_kind_tags() {
        let definition = definition();
        let json = serde_json::to_string(&definition).unwrap();
        assert!(json.contains(r#""kind":"devil_square""#));
        assert!(json.contains(r#""kind":"respawning_while_open""#));
        assert!(json.contains(r#""kind":"once_at_wave_start""#));
        assert!(json.contains(r#""kind":"experience_per_remaining_second""#));
        assert!(json.contains(r#""kind":"item_drop""#));
        assert_eq!(
            serde_json::from_str::<MiniGameDefinition>(&json).unwrap(),
            definition
        );
    }

    #[test]
    fn definition_wire_folds_the_exit_span_and_stays_idempotent() {
        let definition = definition();
        // Built from a 180 s raw exit: stored folded to 150 s.
        assert_eq!(definition.exit_duration.get(), DurationMs(150_000));
        // The wire writes the canonical raw back (folded + 30 s)...
        let json = serde_json::to_string(&definition).unwrap();
        assert!(json.contains(r#""exit_duration":180000"#));
        // ...so a second trip folds to the identical value.
        let reparsed = serde_json::from_str::<MiniGameDefinition>(&json).unwrap();
        assert_eq!(reparsed.exit_duration.get(), DurationMs(150_000));
    }

    #[test]
    fn a_sub_floor_exit_duration_saturates_to_the_thirty_second_minimum() {
        let mut raw = serde_json::to_value(definition()).unwrap();
        raw["exit_duration"] = serde_json::json!(20_000);
        let parsed: MiniGameDefinition = serde_json::from_value(raw).unwrap();
        assert_eq!(parsed.exit_duration.get(), DurationMs(30_000));
    }

    #[test]
    fn sub_floor_enter_and_game_durations_fold_up_to_thirty_seconds() {
        let mut raw = serde_json::to_value(definition()).unwrap();
        raw["enter_duration"] = serde_json::json!(1_000);
        raw["game_duration"] = serde_json::json!(29_999);
        let parsed: MiniGameDefinition = serde_json::from_value(raw).unwrap();
        assert_eq!(parsed.enter_duration.get(), DurationMs(30_000));
        assert_eq!(parsed.game_duration.get(), DurationMs(30_000));
    }

    #[test]
    fn a_duplicate_wave_number_is_a_parse_error_but_overlapping_windows_parse() {
        let mut duplicated = definition();
        duplicated.spawn_waves = vec![
            wave(1, 0, 420_000, WaveRespawn::RespawningWhileOpen),
            wave(1, 300_000, 840_000, WaveRespawn::OnceAtWaveStart),
        ];
        let json = serde_json::to_string(&duplicated).unwrap();
        assert!(serde_json::from_str::<MiniGameDefinition>(&json).is_err());
        // Distinct numbers with overlapping windows are legal.
        let overlapping = definition();
        let json = serde_json::to_string(&overlapping).unwrap();
        assert!(serde_json::from_str::<MiniGameDefinition>(&json).is_ok());
    }

    #[test]
    fn wave_spawn_area_wire_carries_monster_area_quantity_and_no_facing() {
        let area = WaveSpawnArea {
            monster: MonsterNumber(17),
            area: TileArea::new(10, 10, 20, 20).unwrap(),
            quantity: NonZeroU16::new(35).unwrap(),
        };
        let json = serde_json::to_string(&area).unwrap();
        assert_eq!(
            json,
            r#"{"monster":17,"area":{"x1":10,"y1":10,"x2":20,"y2":20},"quantity":35}"#
        );
        assert_eq!(serde_json::from_str::<WaveSpawnArea>(&json).unwrap(), area);
    }

    #[test]
    fn wave_respawn_is_a_bare_two_variant_policy() {
        assert_eq!(
            serde_json::to_string(&WaveRespawn::RespawningWhileOpen).unwrap(),
            r#"{"kind":"respawning_while_open"}"#
        );
        assert_eq!(
            serde_json::to_string(&WaveRespawn::OnceAtWaveStart).unwrap(),
            r#"{"kind":"once_at_wave_start"}"#
        );
    }

    #[test]
    fn success_flags_wire_is_an_array_and_a_duplicate_is_rejected() {
        let set = SuccessFlags::new(vec![SuccessFlag::Alive, SuccessFlag::Winner]).unwrap();
        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(json, r#"["alive","winner"]"#);
        assert_eq!(serde_json::from_str::<SuccessFlags>(&json).unwrap(), set);
        assert_eq!(
            serde_json::from_str::<SuccessFlags>("[]").unwrap(),
            SuccessFlags::NONE
        );
        assert!(serde_json::from_str::<SuccessFlags>(r#"["alive","alive"]"#).is_err());
        assert_eq!(
            SuccessFlags::new(vec![SuccessFlag::Dead, SuccessFlag::Dead]),
            Err(MiniGameError::DuplicateSuccessFlag {
                flag: SuccessFlag::Dead
            })
        );
    }

    #[test]
    fn the_empty_flag_set_always_holds() {
        for status in [RosterStatus::Alive, RosterStatus::Dead] {
            for winner in [
                WinnerStanding::None,
                WinnerStanding::Won { by: RosterSlot(1) },
            ] {
                assert!(SuccessFlags::NONE.holds(status, winner, RosterSlot(0)));
            }
        }
    }

    #[test]
    fn each_flag_gates_on_its_own_fact() {
        let alive = SuccessFlags::new(vec![SuccessFlag::Alive]).unwrap();
        assert!(alive.holds(RosterStatus::Alive, WinnerStanding::None, RosterSlot(0)));
        assert!(!alive.holds(RosterStatus::Dead, WinnerStanding::None, RosterSlot(0)));

        let dead = SuccessFlags::new(vec![SuccessFlag::Dead]).unwrap();
        assert!(dead.holds(RosterStatus::Dead, WinnerStanding::None, RosterSlot(0)));
        assert!(!dead.holds(RosterStatus::Alive, WinnerStanding::None, RosterSlot(0)));

        let won = WinnerStanding::Won { by: RosterSlot(1) };
        let winner = SuccessFlags::new(vec![SuccessFlag::Winner]).unwrap();
        assert!(winner.holds(RosterStatus::Alive, won, RosterSlot(1)));
        assert!(!winner.holds(RosterStatus::Alive, won, RosterSlot(2)));
        assert!(!winner.holds(RosterStatus::Alive, WinnerStanding::None, RosterSlot(1)));

        let loser = SuccessFlags::new(vec![SuccessFlag::Loser]).unwrap();
        assert!(loser.holds(RosterStatus::Alive, won, RosterSlot(2)));
        assert!(!loser.holds(RosterStatus::Alive, won, RosterSlot(1)));
        assert!(loser.holds(RosterStatus::Alive, WinnerStanding::None, RosterSlot(1)));

        let exists = SuccessFlags::new(vec![SuccessFlag::WinnerExists]).unwrap();
        assert!(exists.holds(RosterStatus::Alive, won, RosterSlot(2)));
        assert!(!exists.holds(RosterStatus::Alive, WinnerStanding::None, RosterSlot(2)));

        let absent = SuccessFlags::new(vec![SuccessFlag::WinnerNotExists]).unwrap();
        assert!(absent.holds(RosterStatus::Alive, WinnerStanding::None, RosterSlot(2)));
        assert!(!absent.holds(RosterStatus::Alive, won, RosterSlot(2)));
    }

    #[test]
    fn a_flag_conjunction_requires_every_set_flag() {
        let set = SuccessFlags::new(vec![SuccessFlag::Alive, SuccessFlag::Winner]).unwrap();
        let won = WinnerStanding::Won { by: RosterSlot(1) };
        assert!(set.holds(RosterStatus::Alive, won, RosterSlot(1)));
        assert!(!set.holds(RosterStatus::Dead, won, RosterSlot(1)));
        assert!(!set.holds(RosterStatus::Alive, won, RosterSlot(2)));
    }

    #[test]
    fn roster_status_and_winner_standing_wire_forms() {
        assert_eq!(
            serde_json::to_string(&RosterStatus::Alive).unwrap(),
            r#"{"kind":"alive"}"#
        );
        assert_eq!(
            serde_json::to_string(&RosterStatus::Dead).unwrap(),
            r#"{"kind":"dead"}"#
        );
        assert_eq!(
            serde_json::to_string(&WinnerStanding::None).unwrap(),
            r#"{"kind":"none"}"#
        );
        let won = WinnerStanding::Won { by: RosterSlot(1) };
        let json = serde_json::to_string(&won).unwrap();
        assert_eq!(json, r#"{"kind":"won","by":1}"#);
        assert_eq!(serde_json::from_str::<WinnerStanding>(&json).unwrap(), won);
    }

    #[test]
    fn a_reward_entry_with_no_rank_filter_round_trips() {
        let any_rank = RewardEntry {
            rank: None,
            flags: SuccessFlags::NONE,
            reward: RewardKind::Money { amount: Zen(300) },
        };
        let json = serde_json::to_string(&any_rank).unwrap();
        assert_eq!(
            json,
            r#"{"rank":null,"flags":[],"reward":{"kind":"money","amount":300}}"#
        );
        assert_eq!(
            serde_json::from_str::<RewardEntry>(&json).unwrap(),
            any_rank
        );
        let ranked = RewardEntry {
            rank: Some(Rank(1)),
            ..any_rank
        };
        let json = serde_json::to_string(&ranked).unwrap();
        assert_eq!(serde_json::from_str::<RewardEntry>(&json).unwrap(), ranked);
    }
}
