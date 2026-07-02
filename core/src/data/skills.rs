//! Record shape of `skills.json` — active, passive, and buff skills.

use serde::{Deserialize, Serialize};

use super::common::{
    ClassId, EffectId, MonsterNumber, ScaledBy, SkillNumber, SourceVersion, StatRequirement,
};

/// One skill definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    /// Skill number as the client knows it.
    pub number: SkillNumber,
    /// The skill's slug.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Flat damage added by the skill.
    pub attack_damage: u16,
    /// Which damage calculation the skill uses.
    pub damage_type: DamageType,
    /// How the skill executes, kind-tagged.
    pub behavior: SkillBehavior,
    /// How targets are selected.
    pub target: SkillTarget,
    /// Who may be targeted.
    pub target_restriction: TargetRestriction,
    /// Maximum cast distance in tiles.
    pub range: u8,
    /// Radius around the target for implicit target selection.
    pub implicit_target_range: u8,
    /// Hits dealt per attack.
    pub hits_per_attack: u8,
    /// Whether the attacker walks into range first.
    pub moves_to_target: bool,
    /// Whether the target is knocked back.
    pub moves_target: bool,
    /// Elemental affinity; absent = non-elemental.
    pub element: Option<Element>,
    /// Whether the elemental damage modifier is skipped.
    pub skip_elemental_modifier: bool,
    /// Magic effect the skill applies; absent = none.
    pub effect: Option<EffectId>,
    /// Minimum stats required to learn/cast.
    pub requirements: Vec<StatRequirement>,
    /// Resources consumed per cast.
    pub consume: Vec<StatRequirement>,
    /// Classes able to learn the skill.
    pub classes: Vec<ClassId>,
    /// Per-skill damage scaling terms; empty = none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub damage_scaling: Vec<ScaledBy>,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}

/// Which damage calculation a skill uses.
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

/// How a skill executes, kind-tagged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillBehavior {
    /// Single direct hit on the target.
    DirectHit,
    /// Area attack resolved automatically around the caster.
    AreaAutomatic {
        /// Area resolution parameters.
        area: SkillArea,
    },
    /// Area attack aimed at an explicit location.
    AreaExplicit {
        /// Area resolution parameters.
        area: SkillArea,
    },
    /// Area attack aimed at an explicit target.
    AreaExplicitTarget {
        /// Area resolution parameters.
        area: SkillArea,
    },
    /// Applies a buff effect.
    Buff,
    /// Restores a resource.
    Regeneration,
    /// Always-on passive.
    Passive,
    /// Summons a monster.
    Summon {
        /// The summoned monster.
        monster: MonsterNumber,
    },
    /// Special-cased behavior (e.g. Teleport).
    Other,
}

/// Area-attack resolution parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillArea {
    /// Shape of the affected area, kind-tagged.
    pub geometry: AreaGeometry,
    /// Whether hits land later instead of instantly.
    pub deferred_hits: bool,
    /// Hit delay per tile of distance.
    pub delay_per_tile_ms: u32,
    /// Delay between consecutive hits.
    pub delay_between_hits_ms: u32,
    /// Inclusive `[min, max]` hits each target receives.
    pub hits_per_target: [u8; 2],
    /// Inclusive `[min, max]` total hits per attack.
    pub hits_per_attack_range: [u8; 2],
    /// Hit probability per tile of distance, `0.0..=1.0`.
    pub hit_chance_per_distance: f64,
    /// Projectiles spawned per attack.
    pub projectile_count: u8,
    /// Radius around each hit location.
    pub effect_range: u8,
}

/// Shape of an area attack, kind-tagged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AreaGeometry {
    /// A widening corridor in the aimed direction.
    Frustum {
        /// Width at the caster, in tiles.
        start_width: f64,
        /// Width at the far end, in tiles.
        end_width: f64,
        /// Length, in tiles.
        distance: f64,
    },
    /// A circle around the target location.
    Circle {
        /// Diameter, in tiles.
        diameter: f64,
    },
    /// No geometric filter.
    None,
}

/// How a skill selects its targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTarget {
    /// The explicitly chosen target.
    Explicit,
    /// All party members.
    ImplicitParty,
    /// All players in range.
    ImplicitPlayersInRange,
    /// All NPCs/monsters in range.
    ImplicitNpcsInRange,
    /// Everyone in range.
    ImplicitAllInRange,
    /// The chosen target plus everyone in range around it.
    ExplicitWithImplicitInRange,
    /// The caster.
    #[serde(rename = "self")]
    SelfTarget,
}

/// Who a skill may target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetRestriction {
    /// Anyone.
    None,
    /// Only the caster.
    #[serde(rename = "self")]
    SelfOnly,
    /// Only party members.
    Party,
    /// Only players.
    Player,
}

/// Elemental affinity of a skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Element {
    /// Ice.
    Ice,
    /// Poison.
    Poison,
    /// Lightning.
    Lightning,
    /// Fire.
    Fire,
    /// Earth.
    Earth,
    /// Wind.
    Wind,
    /// Water.
    Water,
}
