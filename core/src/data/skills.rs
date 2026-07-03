//! Record shape of `skills.json` — the pre-S3 skill roster.

use serde::{Deserialize, Serialize};

use crate::components::class::ClassSet;
use crate::components::element::Element;

use super::common::{MonsterNumber, Provenance, SkillNumber};
use super::effects::{Ailment, Buff};

/// One skill definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    /// Skill number as the client knows it (Skill.txt index).
    pub number: SkillNumber,
    /// Extraction provenance: dataset era plus optional curation note.
    #[serde(flatten)]
    pub provenance: Provenance,
    /// Base skill damage (Skill.txt Damage column); 0 for weapon-carried and
    /// non-damage skills.
    pub attack_damage: u16,
    /// Which damage calculation the skill uses.
    pub damage_type: DamageType,
    /// Elemental affinity; absent = non-elemental.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<Element>,
    /// Status ailment a successful hit may inflict, gated by the target's
    /// elemental resistance; absent = the hit inflicts nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inflicts: Option<Ailment>,
    /// Maximum cast distance in tiles; 0 = caster-centered / self.
    pub range: u8,
    /// What the skill does, kind-tagged.
    pub shape: SkillShape,
    /// Resources consumed per cast.
    pub cost: CastCost,
    /// Minimum stats required to learn the skill.
    pub learn: LearnRequirement,
    /// Classes able to learn the skill — total per-class set, serialized as a
    /// list of class names; all-false = monster-only.
    pub classes: ClassSet,
}

/// Which damage calculation a skill uses (Skill.txt damage-type fact).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DamageType {
    /// Deals no damage.
    None,
    /// Physical damage.
    Physical,
    /// Wizardry damage.
    Wizardry,
}

/// Resources consumed per cast (Skill.txt Mana and BP/AG columns).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CastCost {
    /// Mana consumed; 0 = none.
    pub mana: u16,
    /// Ability (AG) consumed; 0 = none.
    pub ability: u16,
}

/// Minimum stats required to learn a skill (Skill.txt `ReqLevel` /
/// `ReqEnergy` / `ReqCharisma` columns; `ReqCharisma` is the Command stat).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearnRequirement {
    /// Minimum character level; 0 = none.
    pub level: u16,
    /// Minimum energy; 0 = none.
    pub energy: u16,
    /// Minimum command; 0 = none.
    pub command: u16,
}

/// What a skill does, kind-tagged. One variant per behavior family the game
/// exhibits; per-variant execution lives in `services`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillShape {
    /// Single hit on the chosen target.
    DirectHit,
    /// Direct hit where the attacker lunges to the target and the victim is
    /// knocked to a nearby tile (DK weapon skills).
    Lunge,
    /// Multi-target attack resolved by a named per-skill routine.
    Area {
        /// Which bespoke area routine in `services` resolves the hits.
        pattern: AreaPattern,
    },
    /// Buff the caster applies to themself.
    BuffSelf {
        /// The applied buff.
        buff: Buff,
    },
    /// Buff cast on any player, including the caster.
    BuffPlayer {
        /// The applied buff.
        buff: Buff,
    },
    /// Buff cast on one party member.
    BuffPartyMember {
        /// The applied buff.
        buff: Buff,
    },
    /// Buff applied to every party member in view range.
    BuffParty {
        /// The applied buff.
        buff: Buff,
    },
    /// Restores a player target's health.
    Heal,
    /// Summons a monster that fights for the caster.
    Summon {
        /// The summoned monster.
        monster: MonsterNumber,
    },
    /// Relocates the caster to a chosen walkable tile.
    Teleport,
    /// Begins charging Nova; the release is the Nova skill record.
    NovaCharge,
    /// Teleports the caster's party members to the caster (Dark Lord Summon).
    RecallParty,
}

/// The closed set of bespoke area-attack routines. `services` resolves each
/// with an exhaustive match; a new area skill adds a variant and breaks the
/// build until its routine exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AreaPattern {
    /// Point-blank fire burst around the target tile.
    Flame,
    /// Directional wind beam.
    Twister,
    /// Random strikes around the caster.
    EvilSpirit,
    /// Eruption around the caster.
    Hellfire,
    /// Directional water beam.
    AquaBeam,
    /// Impact circle at the target tile.
    Cometfall,
    /// Eruption around the caster.
    Inferno,
    /// Three-arrow fan in the aimed direction.
    TripleShot,
    /// Blizzard circle at the target tile.
    IceStorm,
    /// Charged blast around the caster.
    Nova,
    /// Spin hitting everything adjacent to the caster.
    TwistingSlash,
    /// Ground slam around the caster.
    RagefulBlow,
    /// Stab hitting the target plus enemies beside it.
    DeathStab,
    /// Piercing arrow along a line.
    Penetration,
    /// Short flame fan in front of the caster.
    FireSlash,
    /// Wide slash fan in front of the caster.
    PowerSlash,
    /// Explosion hitting the target plus enemies beside it.
    FireBurst,
    /// Quake circle around the caster's mount.
    Earthshake,
}
