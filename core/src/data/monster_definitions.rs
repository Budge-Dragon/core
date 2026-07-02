//! Record shape of `monster_definitions.json` — the classic Monster.txt
//! roster: monsters, NPCs, guards, traps, and the soccer ball.

use serde::{Deserialize, Serialize};

use crate::components::element::PerElement;
use crate::components::units::{DurationMs, Level, Resistance};

use super::common::{MonsterNumber, Provenance, SkillNumber};

/// One monster/NPC definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonsterDefinition {
    /// Monster number as the client knows it (model/NPC id).
    pub number: MonsterNumber,
    /// Display name (Monster.txt name column); informational, never a key.
    pub name: String,
    /// Extraction provenance: dataset era plus optional curation note.
    #[serde(flatten)]
    pub provenance: Provenance,
    /// What the entity is, carrying only the data that kind has.
    pub role: MonsterRole,
}

/// What a monster-file entity is, kind-tagged. Fighting kinds carry the
/// Monster.txt combat columns; passive kinds carry none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MonsterRole {
    /// Aggressive monster.
    Monster {
        /// Monster.txt combat columns.
        combat: MonsterCombat,
        /// Elemental resistance bytes, total over `Element`.
        resistances: PerElement<Resistance>,
        /// Movement/timing columns shared by every fighting kind.
        behavior: MobBehavior,
        /// How it attacks, kind-tagged.
        attack: MonsterAttack,
    },
    /// Passive town NPC.
    Npc {
        /// Dialog window opened on talk; absent = opens nothing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<NpcWindow>,
    },
    /// Town guard; attacks aggressors with plain attacks (a rule, so no
    /// `attack` field — the absence is structural).
    Guard {
        /// Monster.txt combat columns.
        combat: MonsterCombat,
        /// Elemental resistance bytes, total over `Element`.
        resistances: PerElement<Resistance>,
        /// Movement/timing columns shared by every fighting kind.
        behavior: MobBehavior,
    },
    /// Trap; sits on its spawn tile, facing per its spawn row.
    Trap {
        /// How the trap picks victims when it fires.
        targeting: TrapTargeting,
        /// Monster.txt combat columns (trap defense is 0 in the source).
        combat: MonsterCombat,
        /// Elemental resistance bytes, total over `Element`.
        resistances: PerElement<Resistance>,
        /// Movement/timing columns (trap `move_range` is 0 in the source).
        behavior: MobBehavior,
        /// How it fires, kind-tagged.
        attack: MonsterAttack,
    },
    /// The Arena battle-soccer ball.
    SoccerBall,
}

/// The Monster.txt combat columns, integer-typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonsterCombat {
    /// Monster level (1-based; the shared guarded newtype).
    pub level: Level,
    /// Maximum health.
    pub hp: u32,
    /// Minimum physical damage.
    pub min_phys_damage: u16,
    /// Maximum physical damage.
    pub max_phys_damage: u16,
    /// Defense.
    pub defense: u16,
    /// Attack success rate.
    pub attack_rate: u16,
    /// Defense success rate.
    pub defense_rate: u16,
}

/// The Monster.txt movement/timing columns shared by every fighting kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobBehavior {
    /// Random-movement radius in tiles.
    pub move_range: u8,
    /// Attack reach in tiles.
    pub attack_range: u8,
    /// Target-recognition radius in tiles.
    pub view_range: u8,
    /// Delay between movement steps.
    pub move_delay_ms: DurationMs,
    /// Delay between attacks.
    pub attack_delay_ms: DurationMs,
    /// Delay before a dead instance respawns.
    pub respawn_ms: DurationMs,
}

/// How a fighting entity attacks, kind-tagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MonsterAttack {
    /// Plain physical attacks.
    Plain,
    /// Casts a skill on attack (its magic effect applies, not its damage
    /// bonus).
    Skill {
        /// The skill; the Atlas proves it resolves in `skills.json` at load.
        skill: SkillNumber,
    },
}

/// Dialog window an NPC opens on talk (classic talk-response window byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NpcWindow {
    /// Item shop.
    Merchant,
    /// The vault (Baz #240).
    Vault,
    /// Chaos machine crafting (Chaos Goblin #238).
    ChaosMachine,
    /// Guild creation (#241).
    GuildMaster,
    /// Devil Square entrance (Charon #237).
    DevilSquare,
    /// Classic quest dialog (Sevina #235).
    Quest,
}

/// How a trap picks victims when it fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrapTargeting {
    /// Strikes the single entity that pressed it (Lance #100, Iron Stick #101).
    SingleWhenPressed,
    /// Strikes every entity on the trap when pressed (Meteorite #103).
    AreaWhenPressed,
    /// Fires along its fixed facing at anything in range (Fire Trap #102).
    Directional,
}
