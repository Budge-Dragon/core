//! Record shape of `monster_definitions.json` — monsters, NPCs, guards,
//! and traps.

use serde::{Deserialize, Serialize};

use super::common::{DropGroupId, MonsterNumber, SkillNumber, SourceVersion, StatValue};

/// One monster/NPC definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonsterDefinition {
    /// Monster number as the client knows it.
    pub number: MonsterNumber,
    /// Display name.
    pub name: String,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// What the entity is and how it behaves, kind-tagged.
    pub role: MonsterRole,
    /// Movement range in tiles.
    pub move_range: u8,
    /// Attack range in tiles.
    pub attack_range: u8,
    /// Aggro/view range in tiles.
    pub view_range: u8,
    /// Delay between movement steps.
    pub move_delay_ms: u32,
    /// Delay between attacks.
    pub attack_delay_ms: u32,
    /// Delay before respawning after death.
    pub respawn_ms: u32,
    /// Maximum items dropped per kill.
    pub max_item_drops: u8,
    /// Skill used when attacking; absent = plain attacks.
    pub attack_skill: Option<SkillNumber>,
    /// The monster's stat block.
    pub stats: Vec<StatValue>,
    /// Monster-specific drop groups.
    pub drop_groups: Vec<DropGroupId>,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}

/// What a monster-file entity is, kind-tagged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MonsterRole {
    /// Default aggressive monster AI.
    Monster,
    /// Non-fighting NPC.
    Npc {
        /// Dialog window opened on interaction; absent = none.
        window: Option<NpcWindow>,
    },
    /// Guard AI.
    Guard,
    /// Stationary trap.
    Trap {
        /// Trap trigger/attack behavior.
        ai: TrapAi,
    },
    /// The Arena battle soccer ball.
    SoccerBall,
}

/// Dialog window an NPC opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NpcWindow {
    /// Item shop.
    Merchant,
    /// Extra storage.
    Storage,
    /// Money vault.
    Vault,
    /// Chaos machine crafting.
    ChaosMachine,
    /// Guild creation.
    GuildMaster,
    /// Devil Square entrance.
    DevilSquare,
    /// Legacy quest dialog.
    LegacyQuest,
}

/// Trap trigger/attack behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrapAi {
    /// Attacks the single entity standing on it.
    AttackSinglePressed,
    /// Attacks everyone standing on it.
    AttackAreaPressed,
    /// Attacks a random entity in range.
    RandomInRange,
    /// Attacks targets in a fixed direction.
    AreaTargetInDirection,
}
