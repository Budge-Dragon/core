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

/// Whether a fighting entity acts inside a safezone. A pure function of the
/// role (a guard patrols and attacks on safe tiles; a monster or trap is
/// suppressed there), stored on [`MobBehavior`] so the monster-AI decision
/// reads it in the existing behavior slot. The role⟺disposition law is proven
/// at parse (`check_monster_dispositions`), so an inconsistent value cannot
/// load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafezoneDisposition {
    /// Basic monsters and traps: never attack from, never step onto, a safe
    /// tile.
    Excluded,
    /// Guards: patrol across and attack from safe town tiles.
    Patrols,
}

impl MonsterRole {
    /// The safezone disposition this role confers — the single source the
    /// parse-time reconciliation holds every stored
    /// [`MobBehavior::disposition`] against. `None` for the two roles that
    /// carry no [`MobBehavior`] (`Npc`, `SoccerBall`). Total over
    /// [`MonsterRole`].
    #[must_use]
    pub fn safezone_disposition(&self) -> Option<SafezoneDisposition> {
        match self {
            MonsterRole::Guard { .. } => Some(SafezoneDisposition::Patrols),
            MonsterRole::Monster { .. } | MonsterRole::Trap { .. } => {
                Some(SafezoneDisposition::Excluded)
            }
            MonsterRole::Npc { .. } | MonsterRole::SoccerBall => None,
        }
    }
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
    /// The safezone disposition this kind's role confers (Guard `Patrols`,
    /// Monster/Trap `Excluded`), reconciled against the role at parse.
    pub disposition: SafezoneDisposition,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn behavior(disposition: SafezoneDisposition) -> MobBehavior {
        MobBehavior {
            move_range: 3,
            attack_range: 1,
            view_range: 5,
            move_delay_ms: DurationMs(400),
            attack_delay_ms: DurationMs(1600),
            respawn_ms: DurationMs(10_000),
            disposition,
        }
    }

    fn combat() -> MonsterCombat {
        MonsterCombat {
            level: Level::MIN,
            hp: 60,
            min_phys_damage: 4,
            max_phys_damage: 7,
            defense: 2,
            attack_rate: 20,
            defense_rate: 4,
        }
    }

    fn resistances() -> PerElement<Resistance> {
        PerElement {
            ice: Resistance(0),
            poison: Resistance(0),
            lightning: Resistance(0),
            fire: Resistance(0),
            earth: Resistance(0),
            wind: Resistance(0),
            water: Resistance(0),
        }
    }

    #[test]
    fn disposition_round_trips_as_bare_snake_case() {
        for (disposition, wire) in [
            (SafezoneDisposition::Excluded, "\"excluded\""),
            (SafezoneDisposition::Patrols, "\"patrols\""),
        ] {
            assert_eq!(serde_json::to_string(&disposition).unwrap(), wire);
            assert_eq!(
                serde_json::from_str::<SafezoneDisposition>(wire).unwrap(),
                disposition
            );
        }
    }

    #[test]
    fn mob_behavior_carries_its_disposition_on_the_wire() {
        for disposition in [SafezoneDisposition::Excluded, SafezoneDisposition::Patrols] {
            let value = behavior(disposition);
            let wire = serde_json::to_string(&value).unwrap();
            assert_eq!(serde_json::from_str::<MobBehavior>(&wire).unwrap(), value);
        }
        let wire = serde_json::to_string(&behavior(SafezoneDisposition::Patrols)).unwrap();
        assert!(wire.contains("\"disposition\":\"patrols\""));
    }

    #[test]
    fn each_role_confers_its_disposition() {
        let monster = MonsterRole::Monster {
            combat: combat(),
            resistances: resistances(),
            behavior: behavior(SafezoneDisposition::Excluded),
            attack: MonsterAttack::Plain,
        };
        let guard = MonsterRole::Guard {
            combat: combat(),
            resistances: resistances(),
            behavior: behavior(SafezoneDisposition::Patrols),
        };
        let trap = MonsterRole::Trap {
            targeting: TrapTargeting::Directional,
            combat: combat(),
            resistances: resistances(),
            behavior: behavior(SafezoneDisposition::Excluded),
            attack: MonsterAttack::Plain,
        };
        let npc = MonsterRole::Npc { window: None };

        assert_eq!(
            monster.safezone_disposition(),
            Some(SafezoneDisposition::Excluded)
        );
        assert_eq!(
            trap.safezone_disposition(),
            Some(SafezoneDisposition::Excluded)
        );
        assert_eq!(
            guard.safezone_disposition(),
            Some(SafezoneDisposition::Patrols)
        );
        assert_eq!(npc.safezone_disposition(), None);
        assert_eq!(MonsterRole::SoccerBall.safezone_disposition(), None);
    }
}
