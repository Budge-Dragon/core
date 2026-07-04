//! Skill casting: the one resolver that turns a caster, a damaging skill, an aim
//! point, and a batch of candidate targets into a rejection or a resolved cast.
//! It composes the profile, combat, and movement services — deriving the
//! caster's profile, striking each target the skill's region covers, applying
//! elemental ailments and knockback, and dashing the caster on a lunge — and
//! spends the skill's cost only once the cast commits. Pure and deterministic:
//! the RNG is drawn per target in a fixed order (the strike, then the element
//! application roll, then the knockback heading), and the caster's dash is
//! deterministic.

use rand_core::RngCore;

use crate::components::active_effect::ActiveEffects;
use crate::components::combat_profile::{CombatProfile, CombatTarget};
use crate::components::element::Element;
use crate::components::placement::Placement;
use crate::components::pool::Pool;
use crate::components::spatial::{
    ConeHalfWidth, Displacement, Fixed, HALF_TILE, Radius, Region, UNITS_PER_TILE, WorldPos,
    WorldRect, WorldVec,
};
use crate::components::stats::Stats;
use crate::components::tile::WalkGrid;
use crate::components::units::{Tick, TickDuration};
use crate::components::vitals::Vitals;
use crate::data::effects::{Ailment, Buff};
use crate::data::skills::{AreaPattern, CastCost, Skill, SkillShape};
use crate::entities::character::Character;
use crate::events::combat::AttackOutcome;
use crate::events::effect::BuffCastOutcome;
use crate::events::movement::StepOutcome;
use crate::events::skills::{CastRejection, SkillOutcome, TargetHit};
use crate::services::chance::{draw_cardinal, roll_apply_elemental};
use crate::services::combat::resolve_attack;
use crate::services::effects::{ApplicableBuff, apply_buff};
use crate::services::movement::{resolve_drift, resolve_step};
use crate::services::profile::{character_profile, effective_profile};
use crate::services::ratio::{nonzero, scale_ratio};

// W-SRC: invented movement grains — no data file carries a skill knockback or
// dash distance. One tile is the knockback grain (mirrors monster_ai's step);
// a lunge dash covers the whole melee gap so the caster ends on the target.
/// The one-tile knockback/shove step distance.
const ONE_TILE_SPEED: Fixed = Fixed::from_raw(UNITS_PER_TILE);
/// A lunge dash distance, large enough to close any melee-range gap in one step.
const DASH_SPEED: Fixed = Fixed::from_raw(8 * UNITS_PER_TILE);
/// The pick radius (tiles) around the aim point for a single-target skill — the
/// clicked target's cell.
const SINGLE_TARGET_RADIUS_TILES: u8 = 1;

// W-SRC: Heal restores 5 + Energy/5 health, applied instantly (no timed effect).
/// Heal flat base, before the energy term.
const HEAL_BASE: u32 = 5;
/// Heal energy divisor: `+ Energy / 5`.
const HEAL_ENERGY_DEN: u32 = 5;

/// A damaging skill's spatial shape — the closed set the cast resolver handles.
/// Non-damaging skill shapes never reach here; [`route`] sorts them out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamagingSkill {
    /// A single-target strike on the aimed cell.
    DirectHit,
    /// A single-target strike that knocks the victim back and dashes the caster
    /// in (the DK weapon skills).
    Lunge,
    /// A multi-target strike resolved by a bespoke area pattern.
    Area {
        /// Which area pattern shapes the region.
        pattern: AreaPattern,
    },
}

/// A skill proven damaging: its definition plus its resolved damaging shape.
/// Minted only by [`route`], so a held value is always a damaging skill.
#[derive(Debug, Clone, Copy)]
pub struct DamagingSkillRef<'a> {
    skill: &'a Skill,
    shape: DamagingSkill,
}

impl DamagingSkillRef<'_> {
    /// The damaging shape.
    #[must_use]
    pub fn shape(self) -> DamagingSkill {
        self.shape
    }

    /// The skill's maximum cast range in tiles.
    #[must_use]
    pub fn range(self) -> u8 {
        self.skill.range
    }

    /// The skill's per-cast resource cost.
    #[must_use]
    pub fn cost(self) -> CastCost {
        self.skill.cost
    }

    /// The skill's elemental affinity, if any.
    #[must_use]
    pub fn element(self) -> Option<Element> {
        self.skill.element
    }

    /// The ailment a successful application inflicts, if any.
    #[must_use]
    pub fn inflicts(self) -> Option<Ailment> {
        self.skill.inflicts
    }
}

/// A skill proven an applicable buff: its definition plus which of the three
/// applicable buffs it grants. Minted only by [`route`].
#[derive(Debug, Clone, Copy)]
pub struct ApplicableBuffRef<'a> {
    skill: &'a Skill,
    buff: ApplicableBuff,
}

impl ApplicableBuffRef<'_> {
    /// Which applicable buff this skill grants.
    #[must_use]
    pub fn buff(self) -> ApplicableBuff {
        self.buff
    }

    /// The skill's maximum cast range in tiles.
    #[must_use]
    pub fn range(self) -> u8 {
        self.skill.range
    }

    /// The skill's per-cast resource cost.
    #[must_use]
    pub fn cost(self) -> CastCost {
        self.skill.cost
    }
}

/// A skill proven a heal: its definition. Minted only by [`route`].
#[derive(Debug, Clone, Copy)]
pub struct HealRef<'a> {
    skill: &'a Skill,
}

impl HealRef<'_> {
    /// The skill's per-cast resource cost.
    #[must_use]
    pub fn cost(self) -> CastCost {
        self.skill.cost
    }
}

/// How a skill is resolved this wave: a damaging strike, an applicable buff, a
/// heal, or deferred to a future wave. The single total router over
/// [`SkillShape`] — a new shape (or a newly applicable buff) breaks the build
/// until its routing is decided.
#[derive(Debug, Clone, Copy)]
pub enum SkillRouting<'a> {
    /// A damaging skill — resolve with [`cast`].
    Damaging(DamagingSkillRef<'a>),
    /// An applicable buff — resolve with [`cast_buff`].
    Buff(ApplicableBuffRef<'a>),
    /// A heal — resolve with [`cast_heal`].
    Heal(HealRef<'a>),
    /// A shape (or buff) this wave does not yet resolve.
    Deferred,
}

/// Routes a skill to how it is resolved this wave. Exhaustive over every skill
/// shape, and over every buff for the two buff shapes it applies — the
/// applicable buffs become [`SkillRouting::Buff`], the rest defer. Every
/// non-damaging, non-buff, non-heal shape is an explicit or-pattern, never a
/// wildcard, so a new shape breaks the build until its routing is decided.
#[must_use]
pub fn route(skill: &Skill) -> SkillRouting<'_> {
    match skill.shape {
        SkillShape::DirectHit => SkillRouting::Damaging(DamagingSkillRef {
            skill,
            shape: DamagingSkill::DirectHit,
        }),
        SkillShape::Lunge => SkillRouting::Damaging(DamagingSkillRef {
            skill,
            shape: DamagingSkill::Lunge,
        }),
        SkillShape::Area { pattern } => SkillRouting::Damaging(DamagingSkillRef {
            skill,
            shape: DamagingSkill::Area { pattern },
        }),
        SkillShape::BuffSelf { buff } | SkillShape::BuffPlayer { buff } => route_buff(skill, buff),
        SkillShape::Heal => SkillRouting::Heal(HealRef { skill }),
        SkillShape::BuffPartyMember { .. }
        | SkillShape::BuffParty { .. }
        | SkillShape::Summon { .. }
        | SkillShape::Teleport
        | SkillShape::NovaCharge
        | SkillShape::RecallParty => SkillRouting::Deferred,
    }
}

/// Routes a single-target buff skill by its buff: the three applicable buffs
/// resolve to [`SkillRouting::Buff`]; the rest defer. Exhaustive over [`Buff`].
fn route_buff(skill: &Skill, buff: Buff) -> SkillRouting<'_> {
    match buff {
        Buff::Defense => SkillRouting::Buff(ApplicableBuffRef {
            skill,
            buff: ApplicableBuff::Defense,
        }),
        Buff::GreaterDamage => SkillRouting::Buff(ApplicableBuffRef {
            skill,
            buff: ApplicableBuff::GreaterDamage,
        }),
        Buff::GreaterDefense => SkillRouting::Buff(ApplicableBuffRef {
            skill,
            buff: ApplicableBuff::GreaterDefense,
        }),
        Buff::SoulBarrier
        | Buff::SwellLife
        | Buff::CriticalDamageIncrease
        | Buff::InfiniteArrow
        | Buff::Alcohol => SkillRouting::Deferred,
    }
}

/// The region an area pattern covers: caster-centered discs, target-centered
/// discs, frontal cones, and forward line beams. Exhaustive over all eighteen
/// patterns; a new pattern breaks the build until its region is defined.
#[must_use]
fn area_region(pattern: AreaPattern, caster: Placement, aim: WorldPos, range: u8) -> Region {
    match pattern {
        AreaPattern::EvilSpirit
        | AreaPattern::Hellfire
        | AreaPattern::Inferno
        | AreaPattern::Nova
        | AreaPattern::TwistingSlash
        | AreaPattern::RagefulBlow
        | AreaPattern::Earthshake => Region::Circle {
            center: caster.position,
            radius: Radius::from_tiles(range),
        },
        AreaPattern::Flame
        | AreaPattern::Cometfall
        | AreaPattern::IceStorm
        | AreaPattern::FireBurst
        | AreaPattern::DeathStab => Region::Circle {
            center: aim,
            radius: Radius::from_tiles(range),
        },
        AreaPattern::TripleShot | AreaPattern::FireSlash => {
            cone_region(caster, range, ConeHalfWidth::DEG_45)
        }
        AreaPattern::PowerSlash => cone_region(caster, range, ConeHalfWidth::DEG_90),
        AreaPattern::Twister | AreaPattern::AquaBeam | AreaPattern::Penetration => {
            line_region(caster, range)
        }
    }
}

fn cone_region(caster: Placement, range: u8, half_width: ConeHalfWidth) -> Region {
    Region::Cone {
        apex: caster.position,
        facing: caster.facing,
        half_width,
        range: Radius::from_tiles(range),
    }
}

/// A forward line beam as the axis-aligned box spanning the caster and the point
/// `range` tiles along its facing, half a tile wide on each side.
fn line_region(caster: Placement, range: u8) -> Region {
    let facing = caster.facing.vector();
    let along = scaled_or_zero(facing, tiles(range));
    let perpendicular = WorldVec::new(facing.y().scale(-1), facing.x());
    let half = scaled_or_zero(perpendicular, Fixed::from_raw(HALF_TILE));
    let endpoint = caster.position + along;
    let corners = [
        caster.position + half,
        caster.position + half.scale(-1),
        endpoint + half,
        endpoint + half.scale(-1),
    ];
    Region::Rect {
        rect: bounding_rect(corners),
    }
}

fn scaled_or_zero(direction: WorldVec, magnitude: Fixed) -> WorldVec {
    match direction.normalized_to(magnitude) {
        Displacement::Scaled { vector } => vector,
        Displacement::NoDirection => WorldVec::ZERO,
    }
}

fn tiles(count: u8) -> Fixed {
    Fixed::from_raw(i64::from(count).saturating_mul(UNITS_PER_TILE))
}

/// The axis-aligned bounding box of four corners. Destructuring the array binds
/// the seed corner and the remaining three directly, so the length-4 type proves
/// the seed is present with no fallback and no indexing.
fn bounding_rect(corners: [WorldPos; 4]) -> WorldRect {
    let [seed, ..] = corners;
    let (mut min_x, mut min_y) = (seed.x().raw(), seed.y().raw());
    let (mut max_x, mut max_y) = (seed.x().raw(), seed.y().raw());
    for corner in corners {
        min_x = min_x.min(corner.x().raw());
        min_y = min_y.min(corner.y().raw());
        max_x = max_x.max(corner.x().raw());
        max_y = max_y.max(corner.y().raw());
    }
    WorldRect::spanning(
        WorldPos::clamped(min_x, min_y),
        WorldPos::clamped(max_x, max_y),
    )
}

/// Resolves a skill cast: rejects for cost or range before spending anything,
/// strikes every target the region covers (single-target skills strike the first
/// covered candidate), applies elemental ailments and knockback, dashes the
/// caster on a lunge, then spends the cost. Returns the caster's vitals after the
/// spend (health unchanged) and the [`SkillOutcome`].
#[must_use]
pub fn cast(
    caster: &Character,
    skill: DamagingSkillRef<'_>,
    aim: WorldPos,
    targets: &[CombatTarget],
    grid: &WalkGrid,
    rng: &mut impl RngCore,
) -> (Vitals, SkillOutcome) {
    let vitals = caster.vitals();
    if let Some(reason) = cast_rejection(caster, skill, aim) {
        return (vitals, SkillOutcome::Rejected { reason });
    }

    let region = skill_region(skill, caster.placement(), aim);
    let mut struck: Vec<(usize, &CombatTarget)> = targets
        .iter()
        .enumerate()
        .filter(|(_, target)| region.contains(target.placement().position))
        .collect();
    if is_single_target(skill.shape()) {
        struck.truncate(1);
    }
    if struck.is_empty() {
        return (
            vitals,
            SkillOutcome::Rejected {
                reason: CastRejection::NoTargetsInRegion,
            },
        );
    }

    let caster_profile = effective_profile(character_profile(caster).0, &caster.active_effects());
    let mut hits = Vec::with_capacity(struck.len());
    for &(index, target) in &struck {
        hits.push(resolve_target_hit(
            index,
            &caster_profile,
            target,
            skill,
            grid,
            rng,
        ));
    }

    let caster_placement = match skill.shape() {
        DamagingSkill::Lunge => lunge_dash(caster.placement(), &struck, grid),
        DamagingSkill::DirectHit | DamagingSkill::Area { .. } => caster.placement(),
    };
    let spent = spend_cost(vitals, skill.cost());
    (
        spent,
        SkillOutcome::Cast {
            caster_placement,
            hits,
        },
    )
}

/// The first failing precondition, or `None` when the cast may proceed. Order:
/// mana, then ability, then (single-target only) range — nothing is spent yet.
fn cast_rejection(
    caster: &Character,
    skill: DamagingSkillRef<'_>,
    aim: WorldPos,
) -> Option<CastRejection> {
    if let Some(reason) = affordability(&caster.vitals(), skill.cost()) {
        return Some(reason);
    }
    let out_of_range = !caster
        .placement()
        .position
        .within_range(aim, Radius::from_tiles(skill.range()));
    if is_single_target(skill.shape()) && out_of_range {
        return Some(CastRejection::OutOfRange);
    }
    None
}

/// The affordability precondition shared by every cast: insufficient mana, then
/// insufficient ability, or `None` when the cost is affordable. Nothing is spent.
fn affordability(vitals: &Vitals, cost: CastCost) -> Option<CastRejection> {
    if vitals.mana.current() < u32::from(cost.mana) {
        return Some(CastRejection::InsufficientMana);
    }
    if vitals.ability.current() < u32::from(cost.ability) {
        return Some(CastRejection::InsufficientAbility);
    }
    None
}

/// Casts an applicable buff onto a supplied receiver's effect store: rejects on
/// unaffordable cost or an out-of-range receiver (spending nothing), else spends
/// the cost, resolves the buff's magnitude from the caster's energy, and applies
/// it. Returns the caster's spent vitals and the resolved effect the host stores
/// on the receiver (self or ally). The merged store is the caller's to persist
/// via `receiver_effects.with(effect)`; this reports the resolved effect as the
/// authoritative decision. `receiver_pos` is where the receiver stands — for a
/// self-cast the caller passes the caster's own position, which is always in
/// range. Which entity receives the buff stays a host targeting decision; the
/// range rule is core's to compute and enforce.
#[must_use]
pub fn cast_buff(
    caster: &Character,
    buff: ApplicableBuffRef<'_>,
    receiver_pos: WorldPos,
    receiver_effects: ActiveEffects,
    now: Tick,
    tick: TickDuration,
) -> (Vitals, BuffCastOutcome) {
    let vitals = caster.vitals();
    if let Some(reason) = affordability(&vitals, buff.cost()) {
        return (vitals, BuffCastOutcome::Rejected { reason });
    }
    if !caster
        .placement()
        .position
        .within_range(receiver_pos, Radius::from_tiles(buff.range()))
    {
        return (
            vitals,
            BuffCastOutcome::Rejected {
                reason: CastRejection::OutOfRange,
            },
        );
    }
    let spent = spend_cost(vitals, buff.cost());
    let (_merged, effect) = apply_buff(
        buff.buff(),
        caster_energy(caster),
        receiver_effects,
        now,
        tick,
    );
    (spent, BuffCastOutcome::Applied { effect })
}

/// Casts a heal onto a supplied receiver's health: rejects on unaffordable cost
/// (spending nothing), else spends the cost, restores `5 + Energy/5` to the
/// receiver's pool (clamped at its maximum), and reports the health actually
/// restored. Instant — stores no timed effect, so it reads no clock.
#[must_use]
pub fn cast_heal(
    caster: &Character,
    heal: HealRef<'_>,
    receiver_health: Pool,
) -> (Vitals, BuffCastOutcome) {
    let vitals = caster.vitals();
    if let Some(reason) = affordability(&vitals, heal.cost()) {
        return (vitals, BuffCastOutcome::Rejected { reason });
    }
    let spent = spend_cost(vitals, heal.cost());
    let healed = HEAL_BASE.saturating_add(scale_ratio(
        u32::from(caster_energy(caster)),
        1,
        nonzero(HEAL_ENERGY_DEN),
    ));
    let restored = receiver_health.restored(healed);
    let amount = restored.current().saturating_sub(receiver_health.current());
    (spent, BuffCastOutcome::Healed { amount })
}

/// The caster's energy — the wizardry stat the buff and heal magnitudes scale
/// off, on either stat shape.
fn caster_energy(caster: &Character) -> u16 {
    match caster.stats() {
        Stats::Standard { energy, .. } | Stats::WithCommand { energy, .. } => energy,
    }
}

fn is_single_target(shape: DamagingSkill) -> bool {
    match shape {
        DamagingSkill::DirectHit | DamagingSkill::Lunge => true,
        DamagingSkill::Area { .. } => false,
    }
}

fn skill_region(skill: DamagingSkillRef<'_>, caster: Placement, aim: WorldPos) -> Region {
    match skill.shape() {
        DamagingSkill::DirectHit | DamagingSkill::Lunge => Region::Circle {
            center: aim,
            radius: Radius::from_tiles(SINGLE_TARGET_RADIUS_TILES),
        },
        DamagingSkill::Area { pattern } => area_region(pattern, caster, aim, skill.range()),
    }
}

/// Resolves one target's hit: folds the target's own defensive effects into the
/// profile it is struck against (so the two-sided fold is authoritative in core),
/// then the strike, then — only on a landed (non-lethal) hit — the elemental
/// ailment and the single knockback. A lethal strike clears the victim's whole
/// effect store (every effect is `StopByDeath`). RNG order: strike, element
/// application roll, knockback heading.
fn resolve_target_hit(
    index: usize,
    caster_profile: &CombatProfile,
    target: &CombatTarget,
    skill: DamagingSkillRef<'_>,
    grid: &WalkGrid,
    rng: &mut impl RngCore,
) -> TargetHit {
    let target_profile = effective_profile(*target.profile(), &target.active_effects());
    let (health, outcome) = resolve_attack(caster_profile, &target_profile, target.health(), rng);
    let mut inflicted = None;
    let mut displacement = None;
    if let AttackOutcome::Landed { .. } = outcome {
        let applied = apply_element(target, skill, rng);
        if applied {
            inflicted = skill.inflicts();
        }
        if knockback_triggered(skill, applied) {
            displacement = knockback(target.placement(), grid, rng);
        }
    }
    // A lethal strike clears the victim's whole effect store in-core; a non-lethal
    // strike carries it forward unchanged (a newly inflicted ailment is reported
    // separately in `inflicted` for the host to apply).
    let active_effects = match outcome {
        AttackOutcome::Killed { .. } => ActiveEffects::EMPTY,
        AttackOutcome::Landed { .. } | AttackOutcome::Missed => target.active_effects(),
    };
    TargetHit {
        target_index: index,
        outcome,
        health,
        active_effects,
        inflicted,
        displacement,
    }
}

/// Whether the skill's elemental effect applies to a target. An elemental skill
/// rolls against the target's resistance for that element; a non-elemental skill
/// always applies (no roll, no RNG drawn).
fn apply_element(
    target: &CombatTarget,
    skill: DamagingSkillRef<'_>,
    rng: &mut impl RngCore,
) -> bool {
    match skill.element() {
        Some(element) => roll_apply_elemental(target.resistance(element), rng),
        None => true,
    }
}

/// Whether a landed hit knocks the target back: a lunge always does, and a
/// lightning-element hit does when its application landed. When both would fire
/// the target is still knocked once — this predicate is queried once per hit.
fn knockback_triggered(skill: DamagingSkillRef<'_>, element_applied: bool) -> bool {
    let lunge = matches!(skill.shape(), DamagingSkill::Lunge);
    let lightning = element_applied && skill.element() == Some(Element::Lightning);
    lunge || lightning
}

/// Knocks the target one tile along a drawn heading; a blocked drift leaves it in
/// place (no displacement reported).
fn knockback(target: Placement, grid: &WalkGrid, rng: &mut impl RngCore) -> Option<Placement> {
    let heading = draw_cardinal(rng);
    match resolve_drift(target, heading, ONE_TILE_SPEED, grid) {
        StepOutcome::Resolved { placement } => Some(placement),
        StepOutcome::Blocked => None,
    }
}

/// The caster's placement after a lunge dash toward its single struck target; a
/// blocked dash leaves the caster in place.
fn lunge_dash(caster: Placement, struck: &[(usize, &CombatTarget)], grid: &WalkGrid) -> Placement {
    match struck.first() {
        Some((_, target)) => {
            match resolve_step(caster, target.placement().position, DASH_SPEED, grid) {
                StepOutcome::Resolved { placement } => placement,
                StepOutcome::Blocked => caster,
            }
        }
        None => caster,
    }
}

fn spend_cost(vitals: Vitals, cost: CastCost) -> Vitals {
    Vitals {
        health: vitals.health,
        mana: vitals.mana.reduced(u32::from(cost.mana)),
        ability: vitals.ability.reduced(u32::from(cost.ability)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::active_effect::ActiveEffect;
    use crate::components::element::PerElement;
    use crate::components::movement::Movement;
    use crate::components::spatial::Facing;
    use crate::components::tile::TileCoord;
    use crate::components::units::{MapNumber, Resistance};
    use crate::data::common::{Provenance, SkillNumber, SourceVersion};
    use crate::data::skills::{DamageType, LearnRequirement};
    use crate::services::profile::monster_profile;

    /// Deterministic `SplitMix64` for replayable tests.
    struct TestRng {
        state: u64,
    }

    impl TestRng {
        fn new(seed: u64) -> Self {
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

    fn all_walkable() -> WalkGrid {
        WalkGrid::from_words([u64::MAX; 1024])
    }

    fn skill(
        shape: SkillShape,
        element: Option<Element>,
        inflicts: Option<Ailment>,
        range: u8,
        mana: u16,
        ability: u16,
    ) -> Skill {
        Skill {
            number: SkillNumber(1),
            provenance: Provenance {
                source_version: SourceVersion::V075,
                review: None,
            },
            attack_damage: 0,
            damage_type: DamageType::Physical,
            element,
            inflicts,
            range,
            shape,
            cost: CastCost { mana, ability },
            learn: LearnRequirement {
                level: 0,
                energy: 0,
                command: 0,
            },
            classes: crate::components::class::ClassSet::NONE,
        }
    }

    fn caster_at(tile: (u8, u8), mana: u32, ability: u32) -> Character {
        let json = serde_json::json!({
            "class": "dark_knight",
            "level": 50,
            "experience": 0,
            "stats": {"kind": "standard", "strength": 200, "agility": 100, "vitality": 100, "energy": 30},
            "unspent_points": 0,
            "placement": {
                "position": serde_json::to_value(TileCoord::new(tile.0, tile.1).to_world()).unwrap(),
                "facing": {"x": 1, "y": 0},
                "movement": "grounded",
                "map": 0
            },
            "vitals": {
                "health": {"current": 500, "max": 500},
                "mana": {"current": mana, "max": mana.max(1)},
                "ability": {"current": ability, "max": ability.max(1)}
            }
        });
        serde_json::from_value(json).unwrap()
    }

    /// The same caster carrying one active timed effect — round-tripped through
    /// the wire so the effect lands in the character's private store.
    fn with_effect(caster: &Character, effect: ActiveEffect) -> Character {
        let mut value = serde_json::to_value(caster).unwrap();
        value["active_effects"] = serde_json::to_value(ActiveEffects::EMPTY.with(effect)).unwrap();
        serde_json::from_value(value).unwrap()
    }

    fn resistances(lightning: u8) -> PerElement<Resistance> {
        PerElement {
            ice: Resistance(0),
            poison: Resistance(0),
            lightning: Resistance(lightning),
            fire: Resistance(0),
            earth: Resistance(0),
            wind: Resistance(0),
            water: Resistance(0),
        }
    }

    fn target_at(tile: (u8, u8), lightning_resist: u8) -> CombatTarget {
        target_with(tile, lightning_resist, 0, 300, ActiveEffects::EMPTY)
    }

    /// A monster target with a tunable defense, health, and carried effects — the
    /// defensive-fold and clear-on-kill tests read these knobs.
    fn target_with(
        tile: (u8, u8),
        lightning_resist: u8,
        defense: u16,
        hp: u32,
        active_effects: ActiveEffects,
    ) -> CombatTarget {
        let combat = crate::data::monster_definitions::MonsterCombat {
            level: crate::components::units::Level::new(20).unwrap(),
            hp: 300,
            min_phys_damage: 5,
            max_phys_damage: 10,
            defense,
            attack_rate: 10,
            defense_rate: 10,
        };
        let profile = monster_profile(&combat, &resistances(lightning_resist), combat.level);
        let placement = Placement {
            position: TileCoord::new(tile.0, tile.1).to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: MapNumber(0),
        };
        CombatTarget::new(profile, Pool::full(hp), placement, active_effects)
    }

    /// Extracts the damaging reference the router yields, or fails the test — the
    /// test-side unwrap now that [`route`] is the single classifier.
    fn damaging_ref(skill: &Skill) -> DamagingSkillRef<'_> {
        match route(skill) {
            SkillRouting::Damaging(reference) => reference,
            SkillRouting::Buff(_) | SkillRouting::Heal(_) | SkillRouting::Deferred => {
                panic!("expected a damaging skill")
            }
        }
    }

    #[test]
    fn route_sorts_every_shape_into_its_disposition() {
        assert!(matches!(
            route(&skill(SkillShape::DirectHit, None, None, 3, 0, 0)),
            SkillRouting::Damaging(_)
        ));
        assert!(matches!(
            route(&skill(SkillShape::Lunge, None, None, 2, 0, 0)),
            SkillRouting::Damaging(_)
        ));
        assert!(matches!(
            route(&skill(
                SkillShape::Area {
                    pattern: AreaPattern::Nova
                },
                None,
                None,
                4,
                0,
                0
            )),
            SkillRouting::Damaging(_)
        ));
        assert!(matches!(
            route(&skill(
                SkillShape::BuffSelf {
                    buff: Buff::Defense
                },
                None,
                None,
                0,
                0,
                0
            )),
            SkillRouting::Buff(_)
        ));
        assert!(matches!(
            route(&skill(
                SkillShape::BuffPlayer {
                    buff: Buff::GreaterDamage
                },
                None,
                None,
                6,
                0,
                0
            )),
            SkillRouting::Buff(_)
        ));
        assert!(matches!(
            route(&skill(SkillShape::Heal, None, None, 0, 0, 0)),
            SkillRouting::Heal(_)
        ));
        // A buff this wave does not resolve, and a non-buff deferred shape.
        assert!(matches!(
            route(&skill(
                SkillShape::BuffSelf {
                    buff: Buff::SoulBarrier
                },
                None,
                None,
                0,
                0,
                0
            )),
            SkillRouting::Deferred
        ));
        assert!(matches!(
            route(&skill(SkillShape::Teleport, None, None, 0, 0, 0)),
            SkillRouting::Deferred
        ));
    }

    #[test]
    fn area_region_is_total_over_every_pattern() {
        let caster = Placement {
            position: TileCoord::new(10, 10).to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: MapNumber(0),
        };
        let aim = TileCoord::new(14, 10).to_world();
        for pattern in [
            AreaPattern::Flame,
            AreaPattern::Twister,
            AreaPattern::EvilSpirit,
            AreaPattern::Hellfire,
            AreaPattern::AquaBeam,
            AreaPattern::Cometfall,
            AreaPattern::Inferno,
            AreaPattern::TripleShot,
            AreaPattern::IceStorm,
            AreaPattern::Nova,
            AreaPattern::TwistingSlash,
            AreaPattern::RagefulBlow,
            AreaPattern::DeathStab,
            AreaPattern::Penetration,
            AreaPattern::FireSlash,
            AreaPattern::PowerSlash,
            AreaPattern::FireBurst,
            AreaPattern::Earthshake,
        ] {
            // Every pattern yields a region containing at least its own centre.
            let region = area_region(pattern, caster, aim, 5);
            assert!(
                region.contains(caster.position) || region.contains(aim),
                "{pattern:?}"
            );
        }
    }

    #[test]
    fn cone_region_excludes_behind_the_caster() {
        let caster = Placement {
            position: TileCoord::new(20, 20).to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: MapNumber(0),
        };
        let region = area_region(AreaPattern::PowerSlash, caster, caster.position, 6);
        assert!(region.contains(TileCoord::new(23, 20).to_world()));
        assert!(!region.contains(TileCoord::new(17, 20).to_world()));
    }

    #[test]
    fn circle_region_excludes_outside_the_radius() {
        let caster = Placement {
            position: TileCoord::new(20, 20).to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: MapNumber(0),
        };
        let region = area_region(AreaPattern::Nova, caster, caster.position, 3);
        assert!(region.contains(TileCoord::new(22, 20).to_world()));
        assert!(!region.contains(TileCoord::new(30, 20).to_world()));
    }

    #[test]
    fn insufficient_mana_rejects_and_spends_nothing() {
        let caster = caster_at((10, 10), 5, 100);
        let definition = skill(SkillShape::DirectHit, None, None, 3, 50, 0);
        let damaging = damaging_ref(&definition);
        let targets = [target_at((11, 10), 0)];
        let aim = TileCoord::new(11, 10).to_world();
        let mut rng = TestRng::new(1);
        let (vitals, outcome) = cast(&caster, damaging, aim, &targets, &all_walkable(), &mut rng);
        assert_eq!(vitals, caster.vitals());
        assert_eq!(
            outcome,
            SkillOutcome::Rejected {
                reason: CastRejection::InsufficientMana
            }
        );
    }

    #[test]
    fn insufficient_ability_rejects_before_range() {
        let caster = caster_at((10, 10), 100, 5);
        let definition = skill(SkillShape::DirectHit, None, None, 3, 0, 50);
        let damaging = damaging_ref(&definition);
        let targets = [target_at((11, 10), 0)];
        let aim = TileCoord::new(11, 10).to_world();
        let mut rng = TestRng::new(1);
        let (_, outcome) = cast(&caster, damaging, aim, &targets, &all_walkable(), &mut rng);
        assert_eq!(
            outcome,
            SkillOutcome::Rejected {
                reason: CastRejection::InsufficientAbility
            }
        );
    }

    #[test]
    fn aim_beyond_range_rejects_out_of_range() {
        let caster = caster_at((10, 10), 100, 100);
        let definition = skill(SkillShape::DirectHit, None, None, 2, 0, 0);
        let damaging = damaging_ref(&definition);
        let targets = [target_at((30, 10), 0)];
        let aim = TileCoord::new(30, 10).to_world();
        let mut rng = TestRng::new(1);
        let (vitals, outcome) = cast(&caster, damaging, aim, &targets, &all_walkable(), &mut rng);
        assert_eq!(vitals, caster.vitals());
        assert_eq!(
            outcome,
            SkillOutcome::Rejected {
                reason: CastRejection::OutOfRange
            }
        );
    }

    #[test]
    fn no_targets_in_region_rejects_and_spends_nothing() {
        let caster = caster_at((10, 10), 100, 100);
        let definition = skill(
            SkillShape::Area {
                pattern: AreaPattern::Nova,
            },
            None,
            None,
            3,
            10,
            0,
        );
        let damaging = damaging_ref(&definition);
        // Target far outside the caster-centred nova radius.
        let targets = [target_at((40, 40), 0)];
        let aim = TileCoord::new(10, 10).to_world();
        let mut rng = TestRng::new(1);
        let (vitals, outcome) = cast(&caster, damaging, aim, &targets, &all_walkable(), &mut rng);
        assert_eq!(vitals, caster.vitals());
        assert_eq!(
            outcome,
            SkillOutcome::Rejected {
                reason: CastRejection::NoTargetsInRegion
            }
        );
    }

    #[test]
    fn a_successful_cast_spends_the_cost() {
        let caster = caster_at((10, 10), 100, 40);
        let definition = skill(SkillShape::DirectHit, None, None, 3, 30, 10);
        let damaging = damaging_ref(&definition);
        let targets = [target_at((11, 10), 0)];
        let aim = TileCoord::new(11, 10).to_world();
        let mut rng = TestRng::new(2);
        let (vitals, outcome) = cast(&caster, damaging, aim, &targets, &all_walkable(), &mut rng);
        assert_eq!(vitals.mana.current(), 70);
        assert_eq!(vitals.ability.current(), 30);
        assert_eq!(vitals.health, caster.vitals().health);
        assert!(matches!(outcome, SkillOutcome::Cast { .. }));
    }

    /// The damage a single-target cast dealt to its one struck target (landed or
    /// lethal), or `None` for a miss or rejection.
    fn landed_damage(outcome: SkillOutcome) -> Option<u32> {
        match outcome {
            SkillOutcome::Cast { hits, .. } => match hits.first() {
                Some(target_hit) => match target_hit.outcome {
                    AttackOutcome::Landed { hit } | AttackOutcome::Killed { hit } => {
                        Some(hit.damage.0)
                    }
                    AttackOutcome::Missed => None,
                },
                None => None,
            },
            SkillOutcome::Rejected { .. } => None,
        }
    }

    #[test]
    fn an_active_greater_damage_buff_raises_the_casters_cast_damage() {
        // The empty-effects fold is the identity: an unbuffed caster strikes with
        // its base profile, so this wave leaves the effect-free cast byte-identical.
        let plain = caster_at((10, 10), 100, 100);
        let base = character_profile(&plain).0;
        assert_eq!(effective_profile(base, &plain.active_effects()), base);

        // The same caster carrying an active Greater Damage buff folds the
        // offensive bonus into its strike profile, so its cast lands strictly more
        // damage under an identical seed and target.
        let buffed = with_effect(
            &plain,
            ActiveEffect::GreaterDamage {
                amount: 40,
                expiry: Tick(1000),
            },
        );
        let definition = skill(SkillShape::DirectHit, None, None, 3, 0, 0);
        let damaging = damaging_ref(&definition);
        let targets = [target_at((11, 10), 0)];
        let aim = TileCoord::new(11, 10).to_world();
        let mut compared = false;
        for seed in 0u64..8 {
            let plain_dmg = landed_damage(
                cast(
                    &plain,
                    damaging,
                    aim,
                    &targets,
                    &all_walkable(),
                    &mut TestRng::new(seed),
                )
                .1,
            );
            let buffed_dmg = landed_damage(
                cast(
                    &buffed,
                    damaging,
                    aim,
                    &targets,
                    &all_walkable(),
                    &mut TestRng::new(seed),
                )
                .1,
            );
            // The hit roll is identical (the buff never touches attack rate), so a
            // landing plain strike implies a landing buffed strike.
            if let (Some(plain_dmg), Some(buffed_dmg)) = (plain_dmg, buffed_dmg) {
                assert!(
                    buffed_dmg > plain_dmg,
                    "seed {seed}: buffed {buffed_dmg} must exceed plain {plain_dmg}"
                );
                compared = true;
            }
        }
        assert!(compared, "at least one seed lands a comparable hit");
    }

    #[test]
    fn a_targets_defensive_effects_change_the_damage_it_takes_through_cast() {
        // An unbuffed caster (flat add 0) striking a monster with base defense 20,
        // so a defensive effect folded onto the TARGET visibly moves the damage —
        // proving the two-sided fold runs through the real cast() path, not just
        // effective_profile in isolation.
        let caster = caster_at((10, 10), 100, 100);
        let definition = skill(SkillShape::DirectHit, None, None, 3, 0, 0);
        let damaging = damaging_ref(&definition);
        let aim = TileCoord::new(11, 10).to_world();

        let plain = [target_with((11, 10), 0, 20, 300, ActiveEffects::EMPTY)];
        let greater_defense = [target_with(
            (11, 10),
            0,
            20,
            300,
            ActiveEffects::EMPTY.with(ActiveEffect::GreaterDefense {
                amount: 30,
                expiry: Tick(1000),
            }),
        )];
        let defense_reduction = [target_with(
            (11, 10),
            0,
            20,
            300,
            ActiveEffects::EMPTY.with(ActiveEffect::DefenseReduction { expiry: Tick(1000) }),
        )];
        let dk_defense = [target_with(
            (11, 10),
            0,
            20,
            300,
            ActiveEffects::EMPTY.with(ActiveEffect::Defense { expiry: Tick(1000) }),
        )];

        let cast_dmg = |targets: &[CombatTarget], seed: u64| {
            landed_damage(
                cast(
                    &caster,
                    damaging,
                    aim,
                    targets,
                    &all_walkable(),
                    &mut TestRng::new(seed),
                )
                .1,
            )
        };

        let mut compared = false;
        for seed in 0u64..16 {
            let (Some(base), Some(gd), Some(dr), Some(dk)) = (
                cast_dmg(&plain, seed),
                cast_dmg(&greater_defense, seed),
                cast_dmg(&defense_reduction, seed),
                cast_dmg(&dk_defense, seed),
            ) else {
                continue;
            };
            // Greater Defense raises defense -> less damage; Defense-reduction
            // lowers it -> more damage; DK Defense halves incoming -> less damage.
            assert!(
                gd < base,
                "seed {seed}: greater defense {gd} vs base {base}"
            );
            assert!(
                dr > base,
                "seed {seed}: defense-reduction {dr} vs base {base}"
            );
            assert!(dk < base, "seed {seed}: dk defense {dk} vs base {base}");
            compared = true;
        }
        assert!(compared, "at least one seed lands on every variant");
    }

    #[test]
    fn a_lethal_strike_clears_the_victims_active_effects() {
        // A frail (1 HP) monster carrying poison + ice is one-shot; the kill clears
        // its whole effect store in-core (every timed effect is StopByDeath).
        let caster = caster_at((10, 10), 100, 100);
        let definition = skill(SkillShape::DirectHit, None, None, 3, 0, 0);
        let damaging = damaging_ref(&definition);
        let aim = TileCoord::new(11, 10).to_world();
        let afflicted = ActiveEffects::EMPTY
            .with(ActiveEffect::Poisoned {
                per_tick_damage: 5,
                remaining: crate::components::active_effect::PoisonTicks::INITIAL,
                next_tick: Tick(60),
                cadence: crate::components::units::Ticks(60),
            })
            .with(ActiveEffect::Iced { expiry: Tick(600) });
        let targets = [target_with((11, 10), 0, 0, 1, afflicted)];

        let mut saw_kill = false;
        for seed in 0u64..16 {
            let SkillOutcome::Cast { hits, .. } = cast(
                &caster,
                damaging,
                aim,
                &targets,
                &all_walkable(),
                &mut TestRng::new(seed),
            )
            .1
            else {
                continue;
            };
            let Some(hit) = hits.first() else { continue };
            match hit.outcome {
                AttackOutcome::Killed { .. } => {
                    assert_eq!(
                        hit.active_effects,
                        ActiveEffects::EMPTY,
                        "a lethal strike clears every effect"
                    );
                    saw_kill = true;
                    break;
                }
                AttackOutcome::Landed { .. } | AttackOutcome::Missed => {}
            }
        }
        assert!(saw_kill, "a landing strike kills the 1-HP victim");
    }

    #[test]
    fn a_non_elemental_hit_inflicts_its_ailment_and_an_elemental_hit_gates_on_resistance() {
        let caster = caster_at((10, 10), 100, 100);
        // Non-elemental: always inflicts.
        let plain_def = skill(SkillShape::DirectHit, None, Some(Ailment::Frozen), 3, 0, 0);
        let plain = damaging_ref(&plain_def);
        let targets = [target_at((11, 10), 0)];
        let aim = TileCoord::new(11, 10).to_world();
        let mut rng = TestRng::new(3);
        if let SkillOutcome::Cast { hits, .. } =
            cast(&caster, plain, aim, &targets, &all_walkable(), &mut rng).1
        {
            if let AttackOutcome::Landed { .. } = hits[0].outcome {
                assert_eq!(hits[0].inflicted, Some(Ailment::Frozen));
            }
        }
        // Fully immune elemental target (resist 255): never inflicts.
        let icy_def = skill(
            SkillShape::DirectHit,
            Some(Element::Lightning),
            Some(Ailment::Iced),
            3,
            0,
            0,
        );
        let icy = damaging_ref(&icy_def);
        let immune = [target_at((11, 10), 255)];
        let mut rng = TestRng::new(3);
        if let SkillOutcome::Cast { hits, .. } =
            cast(&caster, icy, aim, &immune, &all_walkable(), &mut rng).1
        {
            assert_eq!(hits[0].inflicted, None);
        }
    }

    #[test]
    fn a_lightning_hit_shoves_the_target_one_tile() {
        let caster = caster_at((10, 10), 100, 100);
        let bolt_def = skill(
            SkillShape::DirectHit,
            Some(Element::Lightning),
            None,
            3,
            0,
            0,
        );
        let bolt = damaging_ref(&bolt_def);
        let targets = [target_at((11, 10), 0)];
        let aim = TileCoord::new(11, 10).to_world();
        let mut rng = TestRng::new(4);
        let (_, outcome) = cast(&caster, bolt, aim, &targets, &all_walkable(), &mut rng);
        match outcome {
            SkillOutcome::Cast { hits, .. } => match hits[0].outcome {
                AttackOutcome::Landed { .. } => {
                    let moved = hits[0].displacement.expect("a landed lightning hit shoves");
                    assert_ne!(moved.position, targets[0].placement().position);
                }
                AttackOutcome::Killed { .. } | AttackOutcome::Missed => {}
            },
            SkillOutcome::Rejected { .. } => panic!("cast should resolve"),
        }
    }

    #[test]
    fn a_lunge_knocks_the_target_back_and_dashes_the_caster_in() {
        let caster = caster_at((10, 10), 100, 100);
        let lunge_def = skill(SkillShape::Lunge, None, None, 4, 0, 0);
        let lunge = damaging_ref(&lunge_def);
        let targets = [target_at((13, 10), 0)];
        let aim = TileCoord::new(13, 10).to_world();
        let mut rng = TestRng::new(5);
        let (_, outcome) = cast(&caster, lunge, aim, &targets, &all_walkable(), &mut rng);
        match outcome {
            SkillOutcome::Cast {
                caster_placement,
                hits,
            } => {
                // The caster dashed toward the target (moved east from x=10).
                assert!(
                    caster_placement.position.x().raw() > caster.placement().position.x().raw()
                );
                if let AttackOutcome::Landed { .. } = hits[0].outcome {
                    assert!(
                        hits[0].displacement.is_some(),
                        "a landed lunge knocks the target"
                    );
                }
            }
            SkillOutcome::Rejected { .. } => panic!("lunge should resolve"),
        }
    }

    #[test]
    fn a_blocked_knockback_leaves_the_target_in_place() {
        // A grid where only the caster/target row is walkable: a drift off it is
        // blocked, so a landed shove reports no displacement.
        let mut words = [0u64; 1024];
        for x in 0u16..256 {
            let bit = (10usize << 8) | usize::from(x);
            words[bit >> 6] |= 1u64 << (bit & 63);
        }
        let grid = WalkGrid::from_words(words);
        let caster = caster_at((10, 10), 100, 100);
        let bolt_def = skill(
            SkillShape::DirectHit,
            Some(Element::Lightning),
            None,
            3,
            0,
            0,
        );
        let bolt = damaging_ref(&bolt_def);
        let targets = [target_at((11, 10), 0)];
        let aim = TileCoord::new(11, 10).to_world();
        // Seeds whose heading draws off-row must report no displacement.
        for seed in 0u64..16 {
            let mut rng = TestRng::new(seed);
            if let SkillOutcome::Cast { hits, .. } =
                cast(&caster, bolt, aim, &targets, &grid, &mut rng).1
            {
                if let AttackOutcome::Landed { .. } = hits[0].outcome {
                    if let Some(moved) = hits[0].displacement {
                        // Any reported move stayed on the walkable row.
                        assert!(grid.walkable(moved.position), "seed {seed}");
                    }
                }
            }
        }
    }

    #[test]
    fn same_seed_replays_bit_for_bit() {
        let caster = caster_at((10, 10), 100, 100);
        let lunge_def = skill(
            SkillShape::Lunge,
            Some(Element::Lightning),
            Some(Ailment::Iced),
            4,
            10,
            5,
        );
        let lunge = damaging_ref(&lunge_def);
        let targets = [target_at((12, 10), 30)];
        let aim = TileCoord::new(12, 10).to_world();
        let run = |seed: u64| {
            let mut rng = TestRng::new(seed);
            cast(&caster, lunge, aim, &targets, &all_walkable(), &mut rng)
        };
        assert_eq!(run(9), run(9));
    }

    fn tick50() -> TickDuration {
        TickDuration::new(50).unwrap()
    }

    fn applicable_buff(skill: &Skill) -> ApplicableBuffRef<'_> {
        match route(skill) {
            SkillRouting::Buff(reference) => reference,
            SkillRouting::Damaging(_) | SkillRouting::Heal(_) | SkillRouting::Deferred => {
                panic!("expected an applicable buff")
            }
        }
    }

    fn heal(skill: &Skill) -> HealRef<'_> {
        match route(skill) {
            SkillRouting::Heal(reference) => reference,
            SkillRouting::Damaging(_) | SkillRouting::Buff(_) | SkillRouting::Deferred => {
                panic!("expected a heal")
            }
        }
    }

    #[test]
    fn cast_buff_spends_and_reports_the_energy_scaled_effect() {
        use crate::components::active_effect::{ActiveEffect, ActiveEffects};
        // caster_at seeds energy 30.
        let caster = caster_at((10, 10), 100, 100);
        let def = skill(
            SkillShape::BuffSelf {
                buff: Buff::GreaterDamage,
            },
            None,
            None,
            0,
            20,
            5,
        );
        let buff = applicable_buff(&def);
        let (vitals, outcome) = cast_buff(
            &caster,
            buff,
            caster.placement().position,
            ActiveEffects::EMPTY,
            Tick(0),
            tick50(),
        );
        assert_eq!(vitals.mana.current(), 80);
        assert_eq!(vitals.ability.current(), 95);
        match outcome {
            // 3 + 30/7 = 7; 60_000ms / 50ms = 1200 ticks.
            BuffCastOutcome::Applied { effect } => assert_eq!(
                effect,
                ActiveEffect::GreaterDamage {
                    amount: 7,
                    expiry: Tick(1200),
                }
            ),
            BuffCastOutcome::Rejected { .. } | BuffCastOutcome::Healed { .. } => {
                panic!("expected an applied buff")
            }
        }
    }

    #[test]
    fn cast_buff_rejects_when_mana_is_short_and_spends_nothing() {
        use crate::components::active_effect::ActiveEffects;
        let caster = caster_at((10, 10), 5, 100);
        let def = skill(
            SkillShape::BuffSelf {
                buff: Buff::Defense,
            },
            None,
            None,
            0,
            50,
            0,
        );
        let buff = applicable_buff(&def);
        let (vitals, outcome) = cast_buff(
            &caster,
            buff,
            caster.placement().position,
            ActiveEffects::EMPTY,
            Tick(0),
            tick50(),
        );
        assert_eq!(vitals, caster.vitals());
        assert_eq!(
            outcome,
            BuffCastOutcome::Rejected {
                reason: CastRejection::InsufficientMana
            }
        );
    }

    #[test]
    fn cast_buff_rejects_an_out_of_range_receiver_and_applies_an_in_range_one() {
        let caster = caster_at((10, 10), 100, 100);
        let def = skill(
            SkillShape::BuffPlayer {
                buff: Buff::GreaterDefense,
            },
            None,
            None,
            6,
            20,
            0,
        );
        let buff = applicable_buff(&def);
        // A receiver 30 tiles away is beyond the skill's 6-tile range.
        let far = TileCoord::new(40, 10).to_world();
        let (vitals, outcome) =
            cast_buff(&caster, buff, far, ActiveEffects::EMPTY, Tick(0), tick50());
        assert_eq!(
            vitals,
            caster.vitals(),
            "an out-of-range cast spends nothing"
        );
        assert_eq!(
            outcome,
            BuffCastOutcome::Rejected {
                reason: CastRejection::OutOfRange
            }
        );
        // A receiver within the 6-tile range applies.
        let near = TileCoord::new(13, 10).to_world();
        let (_, applied) = cast_buff(&caster, buff, near, ActiveEffects::EMPTY, Tick(0), tick50());
        assert!(matches!(applied, BuffCastOutcome::Applied { .. }));
    }

    #[test]
    fn cast_heal_restores_energy_scaled_health_bounded_by_max() {
        let caster = caster_at((10, 10), 100, 100);
        let def = skill(SkillShape::Heal, None, None, 0, 10, 0);
        let heal = heal(&def);
        // 5 + 30/5 = 11 restored into the wound.
        let (vitals, outcome) = cast_heal(&caster, heal, Pool::new(50, 100).unwrap());
        assert_eq!(vitals.mana.current(), 90);
        assert_eq!(outcome, BuffCastOutcome::Healed { amount: 11 });
        // Near full, only the restorable amount is reported (2 of 11 fit).
        let (_, capped) = cast_heal(&caster, heal, Pool::new(98, 100).unwrap());
        assert_eq!(capped, BuffCastOutcome::Healed { amount: 2 });
    }

    #[test]
    fn cast_heal_rejects_when_unaffordable() {
        let caster = caster_at((10, 10), 3, 100);
        let def = skill(SkillShape::Heal, None, None, 0, 50, 0);
        let heal = heal(&def);
        let (vitals, outcome) = cast_heal(&caster, heal, Pool::new(50, 100).unwrap());
        assert_eq!(vitals, caster.vitals());
        assert_eq!(
            outcome,
            BuffCastOutcome::Rejected {
                reason: CastRejection::InsufficientMana
            }
        );
    }
}
