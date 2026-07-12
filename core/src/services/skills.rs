//! Skill casting: the one resolver that turns a caster, a located damaging
//! cast, and a batch of candidate targets into a rejection or a resolved cast.
//! It composes the profile, combat, and movement services — minting the cast's
//! region descriptor from the skill's authored geometry (the aim bound in only
//! where the geometry demands one), deriving the caster's profile and the
//! skill's DamageType-selected strike basis once per cast, striking each
//! covered target on that basis, applying elemental ailments and the per-skill
//! displacement (Earthshake's directional push, the lunge jiggle, the lightning
//! jiggle), and teleporting the caster on a lunge — and spends the skill's cost
//! only once the cast commits. The terrain grid doubles as the safezone
//! firewall: a caster standing on a safe town tile is rejected before anything
//! is spent, a safezone-standing target is dropped from the covered set, and
//! no push or jiggle ever lands a target on a safe tile. Pure and
//! deterministic: the RNG is drawn per target in a fixed order (the strike,
//! then the element application roll, then the displacement draws), and the
//! caster's teleport draws nothing.

use rand_core::RngCore;

use crate::components::active_effect::ActiveEffects;
use crate::components::class::CharacterClass;
use crate::components::combat_profile::{CombatProfile, CombatTarget, WeaponMode};
use crate::components::element::Element;
use crate::components::interval::Interval;
use crate::components::life::LifeState;
use crate::components::placement::Placement;
use crate::components::pool::Pool;
use crate::components::spatial::{
    ConeHalfWidth, Displacement, Fixed, Radius, Region, StepMagnitude, UNITS_PER_TILE, WorldPos,
    WorldRect, WorldVec,
};
use crate::components::stats::Stats;
use crate::components::tile::TerrainGrid;
use crate::components::units::{Tick, TickDuration};
use crate::components::vitals::Vitals;
use crate::data::effects::{Ailment, Buff};
use crate::data::skills::{
    AreaDisplacement, AreaGeometry, CastCost, DamageType, Skill, SkillShape,
};
use crate::entities::character::Character;
use crate::events::combat::AttackOutcome;
use crate::events::effect::BuffCastOutcome;
use crate::events::movement::StepOutcome;
use crate::events::skills::{CastRejection, SkillOutcome, TargetHit};
use crate::services::chance::{draw_heading, roll_apply_elemental};
use crate::services::combat::{ExcellentOrder, StrikeBasis, resolve_attack};
use crate::services::effects::{ApplicableBuff, apply_buff};
use crate::services::movement::{lunge_teleport, resolve_drift, resolve_step};
use crate::services::profile::effective_profile;
use crate::services::ratio::{nonzero, scale_ratio};

/// The pick radius (tiles) around the aim point for a single-target skill — the
/// clicked target's cell.
const SINGLE_TARGET_RADIUS_TILES: u8 = 1;

// W-SRC: the push distance is the authentic Earthshake shove — OpenMU's
// EarthShakeSkillPlugIn throws the victim exactly three tiles away.
/// The straight-line distance Earthshake's push throws a target, in tiles.
const PUSH_TILES: i64 = 3;
/// The push endpoint as a fixed-point distance — [`PUSH_TILES`] tiles along the
/// attacker→target line.
const PUSH_DISTANCE: Fixed = Fixed::from_raw(PUSH_TILES * UNITS_PER_TILE);
/// The number of one-tile CCD sweep sub-steps: `ceil(PUSH_TILES / 1 tile)`.
/// Each increment is one tile, so the count is [`PUSH_TILES`]; that ≤1-tile
/// bound is what keeps each destination-only walkability check sound.
const PUSH_STEPS: i64 = PUSH_TILES;

/// The jiggle's nudge distance — one tile. The ≤1-tile magnitude keeps
/// [`resolve_drift`]'s destination-only walkability check sound (no tunnelling),
/// exactly as an ordinary step's.
const JIGGLE_MAGNITUDE: StepMagnitude = StepMagnitude::ONE_TILE;

// W-SRC: Heal restores 5 + Energy/5 health, applied instantly (no timed effect).
/// Heal flat base, before the energy term.
const HEAL_BASE: u32 = 5;
/// Heal energy divisor: `+ Energy / 5`.
const HEAL_ENERGY_DEN: u32 = 5;

// W-SRC: OpenMU class SkillMultiplier — the ~2x every DK/MG/DL *skill* hit
// carries (never a plain swing), per-mille. DW/SM & FE/ME 1.0
// (ClassDarkWizard.cs:112, ClassFairyElf.cs:120); MG 2.0
// (ClassMagicGladiator.cs:123); DK/BK 2.0 + 0.001*Energy
// (ClassDarkKnight.cs:106,:72); DL 2.0 + 0.0005*Energy
// (ClassDarkLord.cs:130,:83). Energy terms are era-uniform (shared by every
// version config incl. 0.75, Version075/CharacterClassInitialization.cs:13,
// 31-33); the ADOPT + energy-micro-term rulings are pinned producer decisions
// (authentic-YES precedent), not defaults.
/// The ×1.0 skill multiplier base, per-mille.
const SKILL_MULTIPLIER_BASE_UNIT: u32 = 1000;
/// The ×2.0 skill multiplier base, per-mille.
const SKILL_MULTIPLIER_BASE_DOUBLE: u32 = 2000;

/// The `[0, 0]` collapsed span for a None-type skill or a wizardry-absent
/// caster.
fn zero_span() -> Interval<u16> {
    Interval::spanning(0, 0)
}

/// A damaging skill's spatial shape — the closed set the cast resolver handles.
/// Non-damaging skill shapes never reach here; [`route`] sorts them out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamagingSkill {
    /// A single-target strike on the aimed cell.
    DirectHit,
    /// A single-target strike that jiggles the victim and teleports the caster
    /// onto it (the DK weapon skills).
    Lunge,
    /// A multi-target strike over an authored region, with a per-skill
    /// displacement.
    Area {
        /// The authored region the strike covers.
        geometry: AreaGeometry,
        /// The authored displacement each struck target takes.
        displacement: AreaDisplacement,
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

    /// Which damage calculation the skill uses.
    #[must_use]
    pub fn damage_type(self) -> DamageType {
        self.skill.damage_type
    }

    /// The skill's base damage `D` (Skill.txt Damage column).
    #[must_use]
    pub fn attack_damage(self) -> u16 {
        self.skill.attack_damage
    }
}

impl<'a> DamagingSkillRef<'a> {
    /// Pairs this skill with the region descriptor its geometry projects for a
    /// host-supplied aim. An aimed skill (single-target strike or aim-circle
    /// area) binds the aim into `AimedDisc`; a caster-anchored skill discards it
    /// — the returned descriptor holds no aim, so the aim can never reach a
    /// caster-anchored cast. The host always has an aim (the click point); this
    /// is the parse-boundary fold that decides whether the skill consults it.
    #[must_use]
    pub fn locate(self, aim: WorldPos) -> LocatedCast<'a> {
        let geometry = match self.shape() {
            DamagingSkill::DirectHit | DamagingSkill::Lunge => CastGeometry::AimedDisc {
                aim,
                radius: Radius::from_tiles(SINGLE_TARGET_RADIUS_TILES),
            },
            DamagingSkill::Area { geometry, .. } => locate_area(geometry, aim),
        };
        LocatedCast {
            skill: self,
            geometry,
        }
    }
}

/// The region a located cast projects, with the aim already bound in where the
/// geometry demands one. `AimedDisc` is the only variant carrying an aim — the
/// single-target strikes (`DirectHit` / `Lunge`) and the aim-circle areas; the
/// three caster-anchored families carry none, so a caster-anchored cast is
/// aimless by construction. Magnitudes are resolved (`Radius` / `Fixed`), not
/// the ×2 wire grain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastGeometry {
    /// A disc at the aim point — single-target strikes and aim-circle areas.
    AimedDisc {
        /// The bound aim point.
        aim: WorldPos,
        /// The disc radius.
        radius: Radius,
    },
    /// A disc around the caster (caster-circle areas).
    CasterDisc {
        /// The disc radius.
        radius: Radius,
    },
    /// A frontal cone at the caster's facing (cone areas).
    Cone {
        /// The cone length.
        range: Radius,
        /// The cone half-angle as an exact squared cosine.
        half_angle: ConeHalfWidth,
    },
    /// A forward beam rectangle along the caster's facing (beam areas).
    Beam {
        /// The beam length.
        length: Fixed,
        /// The beam half-width, to each side of the axis.
        half_width: Fixed,
    },
}

/// A damaging cast located in the world: the routed skill paired with the
/// resolved region descriptor its geometry demands, from the host's aim. Minted
/// only by [`DamagingSkillRef::locate`], so the pairing is always consistent — a
/// caster-anchored skill's descriptor holds no aim, and a single-target skill's
/// always does. [`cast`] consumes one of these; the aim never reaches it loose.
#[derive(Debug, Clone, Copy)]
pub struct LocatedCast<'a> {
    skill: DamagingSkillRef<'a>,
    geometry: CastGeometry,
}

/// The region descriptor an authored area geometry projects for a host aim: only
/// the aim-circle binds the aim; the other three families are caster-anchored.
/// Exhaustive over [`AreaGeometry`] — a new family breaks the build.
fn locate_area(geometry: AreaGeometry, aim: WorldPos) -> CastGeometry {
    match geometry {
        AreaGeometry::AimCircle { radius_x2 } => CastGeometry::AimedDisc {
            aim,
            radius: Radius::from_half_tiles(radius_x2),
        },
        AreaGeometry::CasterCircle { radius_x2 } => CastGeometry::CasterDisc {
            radius: Radius::from_half_tiles(radius_x2),
        },
        AreaGeometry::Cone {
            length_x2,
            half_angle,
        } => CastGeometry::Cone {
            range: Radius::from_half_tiles(length_x2),
            half_angle,
        },
        AreaGeometry::Beam {
            length_x2,
            half_width_x2,
        } => CastGeometry::Beam {
            length: Fixed::from_half_tiles(length_x2),
            half_width: Fixed::from_half_tiles(half_width_x2),
        },
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
        SkillShape::Area {
            geometry,
            displacement,
        } => SkillRouting::Damaging(DamagingSkillRef {
            skill,
            shape: DamagingSkill::Area {
                geometry,
                displacement,
            },
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

/// The region a located cast covers, given where the caster stands. Total over
/// [`CastGeometry`] — single-target and every area family in one exhaustive
/// match; a new descriptor variant breaks the build.
fn region_of(geometry: CastGeometry, caster: Placement) -> Region {
    match geometry {
        CastGeometry::AimedDisc { aim, radius } => Region::Circle {
            center: aim,
            radius,
        },
        CastGeometry::CasterDisc { radius } => Region::Circle {
            center: caster.position,
            radius,
        },
        CastGeometry::Cone { range, half_angle } => cone_region(caster, range, half_angle),
        CastGeometry::Beam { length, half_width } => beam_rect(caster, length, half_width),
    }
}

fn cone_region(caster: Placement, range: Radius, half_width: ConeHalfWidth) -> Region {
    Region::Cone {
        apex: caster.position,
        facing: caster.facing,
        half_width,
        range,
    }
}

/// A forward beam as the axis-aligned box spanning the caster and the point
/// `length` along its facing, `half_width` wide on each side — both authored
/// magnitudes.
fn beam_rect(caster: Placement, length: Fixed, half_width: Fixed) -> Region {
    let facing = caster.facing.vector();
    let along = scaled_or_zero(facing, length);
    let perpendicular = WorldVec::new(facing.y().scale(-1), facing.x());
    let half = scaled_or_zero(perpendicular, half_width);
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

/// Resolves a skill cast: rejects a safezone-standing caster, an unaffordable
/// cost, or an out-of-range aim before spending anything, strikes every target
/// the located region covers except those standing on a safe tile
/// (single-target skills strike the first covered candidate), applies elemental
/// ailments and the per-skill displacement, teleports the caster on a lunge,
/// then spends the cost. Returns the caster's vitals after the spend (health
/// unchanged) and the [`SkillOutcome`].
///
/// `caster_profile` is the caster's BASE strike view, pre-derived by the host —
/// the equipment fold's output for a geared caster, or the bare
/// `character_profile` for a gearless one; the cast folds the caster's live
/// timed effects onto it internally, mirroring the defender side. Passing the
/// profile keeps `Equipment`/`Atlas` out of the combat path.
#[must_use]
pub fn cast(
    caster: &Character,
    caster_profile: &CombatProfile,
    located: LocatedCast<'_>,
    targets: &[CombatTarget],
    grid: &TerrainGrid,
    rng: &mut impl RngCore,
) -> (Vitals, SkillOutcome) {
    let vitals = caster.vitals();
    if let Some(reason) = cast_rejection(caster, &located, grid) {
        return (vitals, SkillOutcome::Rejected { reason });
    }

    let region = region_of(located.geometry, caster.placement());
    let mut struck: Vec<(usize, &CombatTarget)> = targets
        .iter()
        .enumerate()
        .filter(|(_, target)| {
            let position = target.placement().position;
            region.contains(position) && !grid.safe(position)
        })
        .collect();
    if is_single_target(located.skill.shape()) {
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

    let effective = effective_profile(*caster_profile, &caster.active_effects());
    let basis = skill_strike_basis(caster, &effective, located.skill);
    let attacker = caster.placement();
    let mut hits = Vec::with_capacity(struck.len());
    for &(index, target) in &struck {
        hits.push(resolve_target_hit(
            index,
            &effective,
            target,
            located.skill,
            &basis,
            attacker,
            grid,
            rng,
        ));
    }

    let caster_placement = match located.skill.shape() {
        DamagingSkill::Lunge => match struck.first() {
            Some((_, target)) => lunge_teleport(attacker, target.placement()),
            None => attacker,
        },
        DamagingSkill::DirectHit | DamagingSkill::Area { .. } => attacker,
    };
    let spent = spend_cost(vitals, located.skill.cost());
    (
        spent,
        SkillOutcome::Cast {
            caster_placement,
            hits,
        },
    )
}

/// The first failing precondition, or `None` when the cast may proceed. Order:
/// the caster must be alive (a dead caster casts nothing), then the caster's
/// locus (a safe town tile forbids any offensive cast, regardless of cost or
/// aim), then mana, then ability, then the aim gate — nothing is spent yet.
fn cast_rejection(
    caster: &Character,
    located: &LocatedCast<'_>,
    grid: &TerrainGrid,
) -> Option<CastRejection> {
    match caster.life() {
        LifeState::Alive => {}
        LifeState::Dead { .. } => return Some(CastRejection::CasterNotAlive),
    }
    if grid.safe(caster.placement().position) {
        return Some(CastRejection::CasterInSafezone);
    }
    if let Some(reason) = affordability(&caster.vitals(), located.skill.cost()) {
        return Some(reason);
    }
    if out_of_range(
        caster.placement().position,
        located.skill.range(),
        located.geometry,
    ) {
        return Some(CastRejection::OutOfRange);
    }
    None
}

/// Whether the cast's aim lies beyond the caster's cast range. Only an aimed
/// disc consults the aim (single-target strikes and aim-circle areas); the
/// three caster-anchored families never do. Exhaustive over [`CastGeometry`].
fn out_of_range(caster_pos: WorldPos, cast_range: u8, geometry: CastGeometry) -> bool {
    match geometry {
        CastGeometry::AimedDisc { aim, .. } => {
            !caster_pos.within_range(aim, Radius::from_tiles(cast_range))
        }
        CastGeometry::CasterDisc { .. } | CastGeometry::Cone { .. } | CastGeometry::Beam { .. } => {
            false
        }
    }
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
    match caster.life() {
        LifeState::Alive => {}
        LifeState::Dead { .. } => {
            return (
                vitals,
                BuffCastOutcome::Rejected {
                    reason: CastRejection::CasterNotAlive,
                },
            );
        }
    }
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
    match caster.life() {
        LifeState::Alive => {}
        LifeState::Dead { .. } => {
            return (
                vitals,
                BuffCastOutcome::Rejected {
                    reason: CastRejection::CasterNotAlive,
                },
            );
        }
    }
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

/// The class `SkillMultiplier` as per-mille (÷1000), applied to skill strikes
/// only. Pure arithmetic over the caster's class and total energy.
pub(crate) fn skill_multiplier_per_mille(class: CharacterClass, energy: u16) -> u32 {
    match class {
        CharacterClass::DarkWizard
        | CharacterClass::SoulMaster
        | CharacterClass::FairyElf
        | CharacterClass::MuseElf => SKILL_MULTIPLIER_BASE_UNIT,
        CharacterClass::MagicGladiator => SKILL_MULTIPLIER_BASE_DOUBLE,
        CharacterClass::DarkKnight | CharacterClass::BladeKnight => {
            SKILL_MULTIPLIER_BASE_DOUBLE.saturating_add(u32::from(energy))
        }
        CharacterClass::DarkLord => {
            SKILL_MULTIPLIER_BASE_DOUBLE.saturating_add(u32::from(energy) / 2)
        }
    }
}

/// Selects a skill's augmented strike span and excellent order from the
/// `DamageType`, and pairs it with the caster's class multiplier — the whole span
/// source of a skill strike (`CombatProfile.wizardry` + `Skill.attack_damage` +
/// `Skill.damage_type` read together). A double-wielded skill halves its flat
/// `D` (the head's ×2 restores the skill's authored punch at net ×1); a worn
/// staff's rise multiplies the whole augmented wizardry span. Exhaustive over
/// [`DamageType`]; `None` is a totality arm. Pure, draws no RNG.
pub(crate) fn skill_strike_basis(
    caster: &Character,
    caster_profile: &CombatProfile,
    skill: DamagingSkillRef<'_>,
) -> StrikeBasis {
    let multiplier_per_mille = skill_multiplier_per_mille(caster.class(), caster_energy(caster));
    // W-SRC: a double-wielded skill's flat damage D is halved before the
    // strike head's post-roll ×2 (AttackableExtensions.cs:765-769).
    let d = match caster_profile.weapon_mode() {
        WeaponMode::Single => skill.attack_damage(),
        WeaponMode::DoubleWield => skill.attack_damage() / 2,
    };
    match skill.damage_type() {
        DamageType::Physical => StrikeBasis::Skill {
            span: augmented_span(caster_profile.physical(), d),
            excellent_order: ExcellentOrder::MultiplyThenDefense,
            multiplier_per_mille,
        },
        DamageType::Wizardry => StrikeBasis::Skill {
            // W-SRC: a missing wizardry interval collapses base AND add to zero
            // (missing attributes read 0, AttributeSystem.cs:111-126); D is
            // discarded, never a physical fallback.
            span: match caster_profile.wizardry() {
                Some(wizardry) => rise_applied(
                    augmented_span(wizardry, d),
                    caster_profile.wizardry_rise_x2(),
                ),
                None => zero_span(),
            },
            excellent_order: ExcellentOrder::DefenseThenMultiply,
            multiplier_per_mille,
        },
        DamageType::None => StrikeBasis::Skill {
            // W-SRC: None selects no span (AttackableExtensions.cs:824-826);
            // attack_damage discarded, no physical fallback. The excellent
            // order is moot on a [0,0] span; the physical order is the neutral
            // choice.
            span: zero_span(),
            excellent_order: ExcellentOrder::MultiplyThenDefense,
            multiplier_per_mille,
        },
    }
}

// W-SRC: the staff rise multiplies the WHOLE wizardry parenthesis including
// the skill damage — `(WizBase + D) × (1 + rise/100)` on both ends
// (AttackableExtensions.cs:808-810; ClassDarkWizard.cs:81,113). The ×2 carrier
// keeps odd-magic-power half-points integral; 2 (carrier) × 100 (percent).
/// The rise multiplier's denominator: `× (200 + rise_x2) / 200`.
const RISE_DENOMINATOR: u32 = 200;

/// Multiplies a wizardry span by the staff rise, both ends floored — the
/// single divide of the ×2-carried rise. `rise_x2 = 0` is the ×1 identity
/// (200/200), so a staffless caster's span is untouched.
fn rise_applied(span: Interval<u16>, rise_x2: u16) -> Interval<u16> {
    let num = u32::from(rise_x2).saturating_add(RISE_DENOMINATOR);
    let scale =
        |end: u16| narrow_span_end(scale_ratio(u32::from(end), num, nonzero(RISE_DENOMINATOR)));
    Interval::spanning(scale(span.min()), scale(span.max()))
}

/// Saturating narrow of a scaled span end back into its `u16` home — boundary
/// saturation of a combat magnitude, never a masked lookup.
fn narrow_span_end(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// Augments a base span by a skill's damage `D`: min += D, max += D + D/2
/// (integer floor) — the asymmetric add (AttackableExtensions.cs:735-736).
/// `spanning` is total: `min <= max` holds because `D/2 >= 0`.
fn augmented_span(base: Interval<u16>, d: u16) -> Interval<u16> {
    let min = base.min().saturating_add(d);
    let max = base.max().saturating_add(d).saturating_add(d / 2);
    Interval::spanning(min, max)
}

fn is_single_target(shape: DamagingSkill) -> bool {
    match shape {
        DamagingSkill::DirectHit | DamagingSkill::Lunge => true,
        DamagingSkill::Area { .. } => false,
    }
}

/// How a struck target is displaced this cast — selected per skill, not inferred
/// from the element tag alone. Earthshake's push is authored
/// ([`AreaDisplacement`]); the lunge jiggle is the DK weapon-skill move; the
/// elemental jiggle is the generic lightning modifier, still element-driven
/// (authentic). `Push` also marks the skill's element inert, so its
/// element-application roll is skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetDisplacement {
    /// Earthshake: directional 3-tile push, element-independent (no element roll).
    Push,
    /// Lunge (MovesTarget): a continuous ~1-tile nudge, fires pre-roll on missed
    /// or landed.
    LungeJiggle,
    /// A lightning-element (non-Earthshake) hit's continuous ~1-tile nudge, gated
    /// on the element roll landing (landed hits only).
    ElementalJiggle,
    /// No displacement.
    None,
}

/// The displacement a skill applies to its struck targets. Authored push wins on
/// an area skill; a lunge always jiggles; otherwise the element decides.
/// Exhaustive over the shape and (via [`elemental_displacement`]) over
/// [`Element`].
fn target_displacement(skill: DamagingSkillRef<'_>) -> TargetDisplacement {
    match skill.shape() {
        DamagingSkill::Lunge => TargetDisplacement::LungeJiggle,
        DamagingSkill::DirectHit => elemental_displacement(skill.element()),
        DamagingSkill::Area { displacement, .. } => match displacement {
            AreaDisplacement::DirectionalPush => TargetDisplacement::Push,
            AreaDisplacement::None => elemental_displacement(skill.element()),
        },
    }
}

/// The displacement an element confers: lightning jiggles, everything else (and
/// non-elemental) does not. Exhaustive over [`Element`], so a new element breaks
/// the build — never an `==` comparison.
fn elemental_displacement(element: Option<Element>) -> TargetDisplacement {
    match element {
        Some(Element::Lightning) => TargetDisplacement::ElementalJiggle,
        Some(
            Element::Ice
            | Element::Poison
            | Element::Fire
            | Element::Earth
            | Element::Wind
            | Element::Water,
        )
        | None => TargetDisplacement::None,
    }
}

/// Throws a target three tiles straight away from the attacker along the real
/// attacker→target line — the continuous swept knockback. The endpoint is the
/// target's position plus the away-vector rescaled to [`PUSH_DISTANCE`]; the
/// target is then swept toward it in ≤-one-tile increments (the CCD sub-step —
/// the one-tile bound is what makes each destination-only walkability check
/// sound, so the sweep never tunnels a wall). Each increment stops on the first
/// unwalkable or safe tile and keeps the tiles already gained — a safe town
/// tile refuses the step like a wall, so no target is ever pushed into a
/// safezone. Attacker and target on the exact same point have no direction and
/// no push. Reports the final placement, or `None` when no increment gained
/// ground. Draws no RNG.
fn directional_push(
    attacker: Placement,
    target: Placement,
    grid: &TerrainGrid,
) -> Option<Placement> {
    let dest = match (target.position - attacker.position).normalized_to(PUSH_DISTANCE) {
        Displacement::NoDirection => return None,
        Displacement::Scaled { vector } => target.position + vector,
    };
    let mut current = target;
    let mut moved = false;
    for _ in 0..PUSH_STEPS {
        match resolve_step(current, dest, StepMagnitude::ONE_TILE, grid) {
            StepOutcome::Resolved { placement }
                if placement.position != current.position && !grid.safe(placement.position) =>
            {
                current = placement;
                moved = true;
            }
            StepOutcome::Resolved { .. } | StepOutcome::Blocked => break,
        }
    }
    moved.then_some(current)
}

/// The continuous jiggle: draws a free heading and nudges the target one bounded
/// increment along it. A blocked destination or a safe town destination reports
/// no net move (`None`) — blocked-by-safe is stay, for players and NPCs alike;
/// no re-roll. The heading always picks a direction, so an unobstructed jiggle
/// always moves ~one tile; only a wall or safezone refuses it. Draws a variable
/// but deterministic-per-seed number of words (the heading's disk-rejection
/// sample), matching the continuous wander drift.
fn jiggle(target: Placement, grid: &TerrainGrid, rng: &mut impl RngCore) -> Option<Placement> {
    let heading = draw_heading(rng);
    match resolve_drift(target, heading, JIGGLE_MAGNITUDE, grid) {
        StepOutcome::Resolved { placement }
            if placement.position != target.position && !grid.safe(placement.position) =>
        {
            Some(placement)
        }
        StepOutcome::Resolved { .. } | StepOutcome::Blocked => None,
    }
}

/// Resolves one target's hit: folds the target's own defensive effects into the
/// profile it is struck against (so the two-sided fold is authoritative in
/// core), then the strike on the cast-wide basis, then the elemental ailment
/// (landed hits of non-push skills only) and the per-skill displacement. A
/// lethal strike clears the victim's whole effect store (every effect is
/// `StopByDeath`) and is never displaced. RNG order: strike, element
/// application roll, displacement draws.
///
/// The eight parameters are each a distinct, non-bundleable domain input:
/// the batch index (event identity), the
/// caster's effect-folded profile, the struck target, the skill (element /
/// ailment / displacement class), the cast-wide strike basis, the attacker
/// placement (the push's away-vector origin — a placement, never the whole
/// `Character`), the terrain grid, and the injected RNG.
fn resolve_target_hit(
    index: usize,
    caster_profile: &CombatProfile,
    target: &CombatTarget,
    skill: DamagingSkillRef<'_>,
    basis: &StrikeBasis,
    attacker: Placement,
    grid: &TerrainGrid,
    rng: &mut impl RngCore,
) -> TargetHit {
    let displacement_kind = target_displacement(skill);
    let target_profile = effective_profile(*target.profile(), &target.active_effects());
    let (health, outcome) =
        resolve_attack(caster_profile, &target_profile, target.health(), basis, rng);
    match outcome {
        AttackOutcome::Missed => TargetHit::Missed {
            target_index: index,
            health,
            active_effects: target.active_effects(),
            displacement: missed_displacement(
                displacement_kind,
                attacker,
                target.placement(),
                grid,
                rng,
            ),
        },
        AttackOutcome::Landed { hit } => {
            // A Push skill's element is inert (SkipElementalModifier): no roll,
            // no draw; the other kinds roll the element as shipped.
            let applied = match displacement_kind {
                TargetDisplacement::Push => false,
                TargetDisplacement::LungeJiggle
                | TargetDisplacement::ElementalJiggle
                | TargetDisplacement::None => apply_element(target, skill, rng),
            };
            let inflicted = if applied { skill.inflicts() } else { None };
            TargetHit::Landed {
                target_index: index,
                hit,
                health,
                active_effects: target.active_effects(),
                inflicted,
                displacement: landed_displacement(
                    displacement_kind,
                    applied,
                    attacker,
                    target.placement(),
                    grid,
                    rng,
                ),
            }
        }
        AttackOutcome::Killed { hit } => TargetHit::Killed {
            target_index: index,
            hit,
            health,
            active_effects: ActiveEffects::EMPTY,
        },
    }
}

/// The pre-roll displacement on a MISS: only the push and the lunge jiggle fire
/// (both run before the damage roll); the elemental jiggle and the none-case
/// draw nothing on a miss. Exhaustive.
fn missed_displacement(
    kind: TargetDisplacement,
    attacker: Placement,
    target: Placement,
    grid: &TerrainGrid,
    rng: &mut impl RngCore,
) -> Option<Placement> {
    match kind {
        TargetDisplacement::Push => directional_push(attacker, target, grid),
        TargetDisplacement::LungeJiggle => jiggle(target, grid, rng),
        TargetDisplacement::ElementalJiggle | TargetDisplacement::None => None,
    }
}

/// The displacement on a LANDED hit: push and lunge jiggle always fire; the
/// elemental jiggle fires only if the element applied; none does nothing.
/// Exhaustive.
fn landed_displacement(
    kind: TargetDisplacement,
    applied: bool,
    attacker: Placement,
    target: Placement,
    grid: &TerrainGrid,
    rng: &mut impl RngCore,
) -> Option<Placement> {
    match kind {
        TargetDisplacement::Push => directional_push(attacker, target, grid),
        TargetDisplacement::LungeJiggle => jiggle(target, grid, rng),
        TargetDisplacement::ElementalJiggle => {
            if applied {
                jiggle(target, grid, rng)
            } else {
                None
            }
        }
        TargetDisplacement::None => None,
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
    use crate::services::profile::{character_profile, monster_profile};

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

    fn all_walkable() -> TerrainGrid {
        TerrainGrid::from_words([u64::MAX; 1024])
    }

    /// An all-walkable grid whose listed tiles are safe town tiles.
    fn walkable_with_safe(tiles: &[(u8, u8)]) -> TerrainGrid {
        let mut safe = [0u64; 1024];
        for &(x, y) in tiles {
            let bit = (usize::from(y) << 8) | usize::from(x);
            safe[bit >> 6] |= 1u64 << (bit & 63);
        }
        TerrainGrid::from_bitsets([u64::MAX; 1024], safe)
    }

    /// An all-walkable grid that is safe everywhere EXCEPT the listed tiles —
    /// the sealed-by-safezone counterpart of `walkable_with_safe`.
    fn safe_everywhere_but(tiles: &[(u8, u8)]) -> TerrainGrid {
        let mut safe = [u64::MAX; 1024];
        for &(x, y) in tiles {
            let bit = (usize::from(y) << 8) | usize::from(x);
            safe[bit >> 6] &= !(1u64 << (bit & 63));
        }
        TerrainGrid::from_bitsets([u64::MAX; 1024], safe)
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
            "zen": 0,
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

    /// A gearless caster of any class/stat mix at a tile — built the only way a
    /// character can be, by deserialising its wire form. Command classes get
    /// the `with_command` stat shape.
    fn caster_of(class: &str, strength: u16, energy: u16, tile: (u8, u8)) -> Character {
        let stats = if class == "dark_lord" {
            serde_json::json!({"kind": "with_command", "strength": strength, "agility": 100, "vitality": 100, "energy": energy, "command": 50})
        } else {
            serde_json::json!({"kind": "standard", "strength": strength, "agility": 100, "vitality": 100, "energy": energy})
        };
        let json = serde_json::json!({
            "class": class,
            "level": 50,
            "experience": 0,
            "stats": stats,
            "unspent_points": 0,
            "zen": 0,
            "placement": {
                "position": serde_json::to_value(TileCoord::new(tile.0, tile.1).to_world()).unwrap(),
                "facing": {"x": 1, "y": 0},
                "movement": "grounded",
                "map": 0
            },
            "vitals": {
                "health": {"current": 500, "max": 500},
                "mana": {"current": 400, "max": 400},
                "ability": {"current": 400, "max": 400}
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

    /// The caster's gearless base profile — what a host without an equipment
    /// fold pre-derives and passes into [`cast`].
    fn base_profile_of(caster: &Character) -> CombatProfile {
        character_profile(caster).0
    }

    /// The same caster in the death→respawn window — set through the wire so the
    /// private `life` field lands `Dead`.
    fn dead(caster: &Character) -> Character {
        let mut value = serde_json::to_value(caster).unwrap();
        value["life"] = serde_json::json!({"kind": "dead", "respawn_at": 1000});
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
                    geometry: AreaGeometry::CasterCircle { radius_x2: 12 },
                    displacement: AreaDisplacement::None,
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

    fn placed_at(tile: (u8, u8)) -> Placement {
        Placement {
            position: TileCoord::new(tile.0, tile.1).to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: MapNumber(0),
        }
    }

    /// The region a hand-built area skill covers for a caster and aim — the
    /// authored-geometry read exercised end-to-end through `locate` + `region_of`.
    fn area_region_of(
        geometry: AreaGeometry,
        range: u8,
        caster: Placement,
        aim: WorldPos,
    ) -> Region {
        let definition = skill(
            SkillShape::Area {
                geometry,
                displacement: AreaDisplacement::None,
            },
            None,
            None,
            range,
            0,
            0,
        );
        let located = damaging_ref(&definition).locate(aim);
        region_of(located.geometry, caster)
    }

    #[test]
    fn region_of_is_total_over_every_descriptor() {
        let caster = placed_at((10, 10));
        let aim = TileCoord::new(14, 10).to_world();
        let descriptors = [
            CastGeometry::AimedDisc {
                aim,
                radius: Radius::from_half_tiles(2),
            },
            CastGeometry::CasterDisc {
                radius: Radius::from_half_tiles(4),
            },
            CastGeometry::Cone {
                range: Radius::from_half_tiles(12),
                half_angle: ConeHalfWidth::DEG_45,
            },
            CastGeometry::Beam {
                length: Fixed::from_half_tiles(8),
                half_width: Fixed::from_half_tiles(3),
            },
        ];
        for descriptor in descriptors {
            // Every descriptor yields a region containing its own centre.
            let region = region_of(descriptor, caster);
            assert!(
                region.contains(caster.position) || region.contains(aim),
                "{descriptor:?}"
            );
        }
    }

    #[test]
    fn an_aim_circle_covers_its_authored_radius_not_the_cast_range() {
        // Flame's shrink: AimCircle{radius_x2: 2} covers one tile from the aim
        // and excludes two, regardless of the cast range 6.
        let caster = placed_at((10, 10));
        let aim = TileCoord::new(10, 10).to_world();
        let region = area_region_of(AreaGeometry::AimCircle { radius_x2: 2 }, 6, caster, aim);
        assert!(region.contains(TileCoord::new(11, 10).to_world()));
        assert!(!region.contains(TileCoord::new(12, 10).to_world()));
    }

    #[test]
    fn a_half_tile_aim_circle_covers_the_diagonal_neighbour() {
        // IceStorm's 1.5: √2 ≈ 1.414 < 1.5 <= 2, expressible only at the
        // half-tile grain.
        let caster = placed_at((10, 10));
        let aim = TileCoord::new(10, 10).to_world();
        let region = area_region_of(AreaGeometry::AimCircle { radius_x2: 3 }, 6, caster, aim);
        assert!(region.contains(TileCoord::new(11, 11).to_world()));
        assert!(!region.contains(TileCoord::new(12, 10).to_world()));
    }

    #[test]
    fn a_caster_circle_centres_on_the_caster_and_ignores_range_zero() {
        // Hellfire revived: a data-range-0 skill still projects its authored
        // r=2 disc around the CASTER, not the aim.
        let caster = placed_at((10, 10));
        let aim = TileCoord::new(50, 50).to_world();
        let region = area_region_of(AreaGeometry::CasterCircle { radius_x2: 4 }, 0, caster, aim);
        assert!(region.contains(TileCoord::new(12, 10).to_world()));
        assert!(!region.contains(TileCoord::new(13, 10).to_world()));
        assert!(!region.contains(aim));
    }

    #[test]
    fn the_triple_shot_cone_uses_the_exact_ratio_and_its_authored_length() {
        let caster = placed_at((10, 10));
        let aim = caster.position;
        let half_angle = ConeHalfWidth::new(196, core::num::NonZeroU64::new(277).unwrap()).unwrap();
        let region = area_region_of(
            AreaGeometry::Cone {
                length_x2: 14,
                half_angle,
            },
            6,
            caster,
            aim,
        );
        // Six ahead, three off-axis: cos² = 0.8, distance √45 ≈ 6.7 ≤ 7 —
        // inside 196/277 ≈ 0.708 and in range. (The BDD spec's (17,13) worked
        // vector sits at Euclidean √58 ≈ 7.62 > 7 from the apex, outside the
        // design doc's Euclidean cone range — flagged, not silently adopted.)
        assert!(region.contains(TileCoord::new(16, 13).to_world()));
        // Five ahead, four off-axis: cos² ≈ 0.61 — outside the exact ratio,
        // though inside the old DEG_45 (0.5), and within range √41 ≈ 6.4.
        assert!(!region.contains(TileCoord::new(15, 14).to_world()));
        // The authored length 7 reaches past the cast range 6 (decoupled).
        assert!(region.contains(TileCoord::new(17, 10).to_world()));
        assert!(!region.contains(TileCoord::new(18, 10).to_world()));
    }

    #[test]
    fn the_power_slash_cone_excludes_a_ninety_degree_off_facing_target() {
        // The re-pin from the shipped 90-degree semicircle to the authentic DEG_45.
        let caster = placed_at((10, 10));
        let region = area_region_of(
            AreaGeometry::Cone {
                length_x2: 12,
                half_angle: ConeHalfWidth::DEG_45,
            },
            5,
            caster,
            caster.position,
        );
        assert!(region.contains(TileCoord::new(13, 10).to_world()));
        assert!(!region.contains(TileCoord::new(10, 15).to_world()));
        assert!(!region.contains(TileCoord::new(7, 10).to_world()));
    }

    #[test]
    fn a_beam_carries_its_authored_half_width() {
        // Twister: length 4, half-width 1.5 covers a 1-tile-off target the
        // shipped half-tile beam missed.
        let caster = placed_at((10, 10));
        let region = area_region_of(
            AreaGeometry::Beam {
                length_x2: 8,
                half_width_x2: 3,
            },
            6,
            caster,
            caster.position,
        );
        assert!(region.contains(TileCoord::new(13, 11).to_world()));
        assert!(!region.contains(TileCoord::new(13, 12).to_world()));
    }

    #[test]
    fn a_short_wide_beam_is_a_rect_not_a_cone() {
        // Fire Slash: length 2, half-width 2 — one ahead, two off-axis is in
        // (a cone of any half-angle ≤ 45° would exclude it); three ahead is out.
        let caster = placed_at((10, 10));
        let region = area_region_of(
            AreaGeometry::Beam {
                length_x2: 4,
                half_width_x2: 4,
            },
            2,
            caster,
            caster.position,
        );
        assert!(region.contains(TileCoord::new(11, 12).to_world()));
        assert!(!region.contains(TileCoord::new(13, 10).to_world()));
    }

    #[test]
    fn a_caster_anchored_region_is_invariant_to_the_aim() {
        let caster = placed_at((10, 10));
        for geometry in [
            AreaGeometry::CasterCircle { radius_x2: 8 },
            AreaGeometry::Cone {
                length_x2: 12,
                half_angle: ConeHalfWidth::DEG_45,
            },
            AreaGeometry::Beam {
                length_x2: 8,
                half_width_x2: 3,
            },
        ] {
            let near = area_region_of(geometry, 6, caster, TileCoord::new(11, 10).to_world());
            let absurd = area_region_of(geometry, 6, caster, TileCoord::new(250, 3).to_world());
            assert_eq!(near, absurd, "{geometry:?}");
        }
    }

    #[test]
    fn insufficient_mana_rejects_and_spends_nothing() {
        let caster = caster_at((10, 10), 5, 100);
        let definition = skill(SkillShape::DirectHit, None, None, 3, 50, 0);
        let damaging = damaging_ref(&definition);
        let targets = [target_at((11, 10), 0)];
        let aim = TileCoord::new(11, 10).to_world();
        let mut rng = TestRng::new(1);
        let (vitals, outcome) = cast(
            &caster,
            &base_profile_of(&caster),
            damaging.locate(aim),
            &targets,
            &all_walkable(),
            &mut rng,
        );
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
        let (_, outcome) = cast(
            &caster,
            &base_profile_of(&caster),
            damaging.locate(aim),
            &targets,
            &all_walkable(),
            &mut rng,
        );
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
        let (vitals, outcome) = cast(
            &caster,
            &base_profile_of(&caster),
            damaging.locate(aim),
            &targets,
            &all_walkable(),
            &mut rng,
        );
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
                geometry: AreaGeometry::CasterCircle { radius_x2: 12 },
                displacement: AreaDisplacement::None,
            },
            None,
            None,
            3,
            10,
            0,
        );
        let damaging = damaging_ref(&definition);
        // Target far outside the caster-centred r=6 disc.
        let targets = [target_at((40, 40), 0)];
        let aim = TileCoord::new(10, 10).to_world();
        let mut rng = TestRng::new(1);
        let (vitals, outcome) = cast(
            &caster,
            &base_profile_of(&caster),
            damaging.locate(aim),
            &targets,
            &all_walkable(),
            &mut rng,
        );
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
        let (vitals, outcome) = cast(
            &caster,
            &base_profile_of(&caster),
            damaging.locate(aim),
            &targets,
            &all_walkable(),
            &mut rng,
        );
        assert_eq!(vitals.mana.current(), 70);
        assert_eq!(vitals.ability.current(), 30);
        assert_eq!(vitals.health, caster.vitals().health);
        assert!(matches!(outcome, SkillOutcome::Cast { .. }));
    }

    #[test]
    fn a_dead_caster_cannot_cast_a_damaging_skill_and_spends_nothing() {
        // The alive caster would land this cast (affordable, in range, target
        // covered); death is the only thing that changes.
        let alive = caster_at((10, 10), 100, 40);
        let corpse = dead(&alive);
        let definition = skill(SkillShape::DirectHit, None, None, 3, 30, 10);
        let damaging = damaging_ref(&definition);
        let targets = [target_at((11, 10), 0)];
        let aim = TileCoord::new(11, 10).to_world();
        let mut rng = TestRng::new(2);
        let (vitals, outcome) = cast(
            &corpse,
            &base_profile_of(&corpse),
            damaging.locate(aim),
            &targets,
            &all_walkable(),
            &mut rng,
        );
        assert_eq!(vitals, corpse.vitals(), "a corpse spends nothing");
        assert_eq!(
            outcome,
            SkillOutcome::Rejected {
                reason: CastRejection::CasterNotAlive
            }
        );
        // The identical alive caster is unaffected — it casts.
        let mut rng = TestRng::new(2);
        let (_, alive_outcome) = cast(
            &alive,
            &base_profile_of(&alive),
            damaging.locate(aim),
            &targets,
            &all_walkable(),
            &mut rng,
        );
        assert!(matches!(alive_outcome, SkillOutcome::Cast { .. }));
    }

    #[test]
    fn a_dead_caster_cannot_cast_a_buff_and_spends_nothing() {
        use crate::components::active_effect::ActiveEffects;
        let alive = caster_at((10, 10), 100, 100);
        let corpse = dead(&alive);
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
            &corpse,
            buff,
            corpse.placement().position,
            ActiveEffects::EMPTY,
            Tick(0),
            tick50(),
        );
        assert_eq!(vitals, corpse.vitals(), "a corpse spends nothing");
        assert_eq!(
            outcome,
            BuffCastOutcome::Rejected {
                reason: CastRejection::CasterNotAlive
            }
        );
    }

    #[test]
    fn a_dead_caster_cannot_cast_a_heal_and_spends_nothing() {
        let alive = caster_at((10, 10), 100, 100);
        let corpse = dead(&alive);
        let def = skill(SkillShape::Heal, None, None, 6, 20, 0);
        let heal_ref = heal(&def);
        let receiver = Pool::new(10, 100).unwrap();
        let (vitals, outcome) = cast_heal(&corpse, heal_ref, receiver);
        assert_eq!(vitals, corpse.vitals(), "a corpse spends nothing");
        assert_eq!(
            outcome,
            BuffCastOutcome::Rejected {
                reason: CastRejection::CasterNotAlive
            }
        );
        // The identical alive caster is unaffected — it heals.
        let (_, alive_outcome) = cast_heal(&alive, heal(&def), receiver);
        assert!(matches!(alive_outcome, BuffCastOutcome::Healed { .. }));
    }

    /// The damage a single-target cast dealt to its one struck target (landed or
    /// lethal), or `None` for a miss or rejection.
    fn landed_damage(outcome: SkillOutcome) -> Option<u32> {
        match outcome {
            SkillOutcome::Cast { hits, .. } => match hits.first() {
                Some(TargetHit::Landed { hit, .. } | TargetHit::Killed { hit, .. }) => {
                    Some(hit.damage.0)
                }
                Some(TargetHit::Missed { .. }) | None => None,
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
                    &base_profile_of(&plain),
                    damaging.locate(aim),
                    &targets,
                    &all_walkable(),
                    &mut TestRng::new(seed),
                )
                .1,
            );
            let buffed_dmg = landed_damage(
                cast(
                    &buffed,
                    &base_profile_of(&buffed),
                    damaging.locate(aim),
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
                    &base_profile_of(&caster),
                    damaging.locate(aim),
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
                &base_profile_of(&caster),
                damaging.locate(aim),
                &targets,
                &all_walkable(),
                &mut TestRng::new(seed),
            )
            .1
            else {
                continue;
            };
            let Some(hit) = hits.first() else { continue };
            match hit {
                TargetHit::Killed { active_effects, .. } => {
                    assert_eq!(
                        *active_effects,
                        ActiveEffects::EMPTY,
                        "a lethal strike clears every effect"
                    );
                    saw_kill = true;
                    break;
                }
                TargetHit::Landed { .. } | TargetHit::Missed { .. } => {}
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
        if let SkillOutcome::Cast { hits, .. } = cast(
            &caster,
            &base_profile_of(&caster),
            plain.locate(aim),
            &targets,
            &all_walkable(),
            &mut rng,
        )
        .1
        {
            if let TargetHit::Landed { inflicted, .. } = hits[0] {
                assert_eq!(inflicted, Some(Ailment::Frozen));
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
        if let SkillOutcome::Cast { hits, .. } = cast(
            &caster,
            &base_profile_of(&caster),
            icy.locate(aim),
            &immune,
            &all_walkable(),
            &mut rng,
        )
        .1
        {
            if let TargetHit::Landed { inflicted, .. } = hits[0] {
                assert_eq!(inflicted, None);
            }
        }
    }

    /// One tile in sub-units — the jiggle's per-axis bound.
    const TILE: i64 = crate::components::spatial::UNITS_PER_TILE;

    #[test]
    fn a_landed_lightning_hit_jiggles_within_one_tile_per_axis() {
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
        let start = targets[0].placement().position;
        let aim = TileCoord::new(11, 10).to_world();
        let mut jiggled = false;
        for seed in 0u64..32 {
            let mut rng = TestRng::new(seed);
            let SkillOutcome::Cast { hits, .. } = cast(
                &caster,
                &base_profile_of(&caster),
                bolt.locate(aim),
                &targets,
                &all_walkable(),
                &mut rng,
            )
            .1
            else {
                panic!("cast should resolve");
            };
            match hits[0] {
                TargetHit::Landed {
                    displacement: Some(moved),
                    ..
                } => {
                    // The jiggle is at most one tile on each axis.
                    assert!((moved.position.x().raw() - start.x().raw()).abs() <= TILE);
                    assert!((moved.position.y().raw() - start.y().raw()).abs() <= TILE);
                    assert_ne!(moved.position, start, "a reported move is a net move");
                    jiggled = true;
                }
                // A missed lightning hit never jiggles (the element rolls on
                // landed hits only).
                TargetHit::Missed { displacement, .. } => assert_eq!(displacement, None),
                TargetHit::Landed {
                    displacement: None, ..
                }
                | TargetHit::Killed { .. } => {}
            }
        }
        assert!(
            jiggled,
            "a seed in 0..32 lands an applied jiggle that moves"
        );
    }

    #[test]
    fn a_missed_lightning_hit_draws_no_element_roll_and_no_jiggle() {
        // White-box: the elemental jiggle never fires on a miss, and the
        // dispatch draws nothing.
        let target = placed_at((11, 10));
        let attacker = placed_at((10, 10));
        let mut rng = TestRng::new(7);
        let displaced = missed_displacement(
            TargetDisplacement::ElementalJiggle,
            attacker,
            target,
            &all_walkable(),
            &mut rng,
        );
        assert_eq!(displaced, None);
        let mut probe = TestRng::new(7);
        assert_eq!(rng.next_u64(), probe.next_u64(), "no word drawn");
    }

    #[test]
    fn an_unapplied_elemental_jiggle_neither_moves_nor_draws() {
        // White-box: a landed lightning hit whose element roll failed skips
        // the jiggle entirely — no move, no words.
        let target = placed_at((11, 10));
        let attacker = placed_at((10, 10));
        let mut rng = TestRng::new(9);
        let displaced = landed_displacement(
            TargetDisplacement::ElementalJiggle,
            false,
            attacker,
            target,
            &all_walkable(),
            &mut rng,
        );
        assert_eq!(displaced, None);
        let mut probe = TestRng::new(9);
        assert_eq!(rng.next_u64(), probe.next_u64(), "no word drawn");
    }

    #[test]
    fn a_lunge_teleports_the_caster_onto_the_target_and_jiggles_the_victim() {
        let caster = caster_at((10, 10), 100, 100);
        let lunge_def = skill(SkillShape::Lunge, None, None, 4, 0, 0);
        let lunge = damaging_ref(&lunge_def);
        let targets = [target_at((13, 10), 0)];
        let start = targets[0].placement().position;
        let aim = TileCoord::new(13, 10).to_world();
        for seed in 0u64..16 {
            let mut rng = TestRng::new(seed);
            let SkillOutcome::Cast {
                caster_placement,
                hits,
            } = cast(
                &caster,
                &base_profile_of(&caster),
                lunge.locate(aim),
                &targets,
                &all_walkable(),
                &mut rng,
            )
            .1
            else {
                panic!("lunge should resolve");
            };
            // The caster teleports onto the target's exact cell, every outcome.
            assert_eq!(caster_placement.position, start, "seed {seed}");
            match hits[0] {
                // The victim's jiggle fires on missed AND landed hits alike.
                TargetHit::Landed {
                    displacement: Some(moved),
                    ..
                }
                | TargetHit::Missed {
                    displacement: Some(moved),
                    ..
                } => {
                    assert!((moved.position.x().raw() - start.x().raw()).abs() <= TILE);
                    assert!((moved.position.y().raw() - start.y().raw()).abs() <= TILE);
                }
                TargetHit::Landed {
                    displacement: None, ..
                }
                | TargetHit::Missed {
                    displacement: None, ..
                }
                | TargetHit::Killed { .. } => {}
            }
        }
    }

    #[test]
    fn a_lunge_that_kills_does_not_jiggle_but_still_teleports() {
        let caster = caster_at((10, 10), 100, 100);
        let lunge_def = skill(SkillShape::Lunge, None, None, 4, 0, 0);
        let lunge = damaging_ref(&lunge_def);
        // A frail 1-HP victim: any landing strike kills it.
        let targets = [target_with((13, 10), 0, 0, 1, ActiveEffects::EMPTY)];
        let aim = TileCoord::new(13, 10).to_world();
        let mut saw_kill = false;
        for seed in 0u64..16 {
            let mut rng = TestRng::new(seed);
            let SkillOutcome::Cast {
                caster_placement,
                hits,
            } = cast(
                &caster,
                &base_profile_of(&caster),
                lunge.locate(aim),
                &targets,
                &all_walkable(),
                &mut rng,
            )
            .1
            else {
                continue;
            };
            assert_eq!(caster_placement.position, aim, "the teleport still fires");
            if let TargetHit::Killed { .. } = hits[0] {
                // Killed carries no displacement field at all — the victim
                // stays; only the RNG-draw contract is left to check: the
                // strike sequence is the whole consumption (no jiggle words).
                saw_kill = true;
            }
        }
        assert!(saw_kill, "a landing strike kills the 1-HP victim");
    }

    #[test]
    fn a_blocked_jiggle_leaves_the_target_in_place() {
        // A grid where only the caster/target row is walkable: a jiggle off it
        // is blocked, so a landed shove reports no displacement (no re-roll).
        let mut words = [0u64; 1024];
        for x in 0u16..256 {
            let bit = (10usize << 8) | usize::from(x);
            words[bit >> 6] |= 1u64 << (bit & 63);
        }
        let grid = TerrainGrid::from_words(words);
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
            if let SkillOutcome::Cast { hits, .. } = cast(
                &caster,
                &base_profile_of(&caster),
                bolt.locate(aim),
                &targets,
                &grid,
                &mut rng,
            )
            .1
            {
                if let TargetHit::Landed {
                    displacement: Some(moved),
                    ..
                } = hits[0]
                {
                    // Any reported move stayed on the walkable row.
                    assert!(grid.walkable(moved.position), "seed {seed}");
                }
            }
        }
    }

    /// A hand-built area skill of the given geometry, displacement, element,
    /// and range — the dispatch/aim-gate tests' fixture.
    fn area_skill(
        geometry: AreaGeometry,
        displacement: AreaDisplacement,
        element: Option<Element>,
        range: u8,
    ) -> Skill {
        skill(
            SkillShape::Area {
                geometry,
                displacement,
            },
            element,
            None,
            range,
            0,
            0,
        )
    }

    /// An Earthshake-shaped skill: caster circle r=5, directional push, an
    /// inert lightning tag.
    fn earthshake_skill() -> Skill {
        area_skill(
            AreaGeometry::CasterCircle { radius_x2: 10 },
            AreaDisplacement::DirectionalPush,
            Some(Element::Lightning),
            10,
        )
    }

    #[test]
    fn an_aim_centered_area_cast_gates_its_aim_before_spend() {
        let caster = caster_at((10, 10), 100, 100);
        let flame = area_skill(
            AreaGeometry::AimCircle { radius_x2: 2 },
            AreaDisplacement::None,
            Some(Element::Fire),
            6,
        );
        let damaging = damaging_ref(&flame);
        // Ten tiles out: beyond the cast range 6 — rejected, nothing spent.
        let far = TileCoord::new(20, 10).to_world();
        assert_eq!(
            cast_rejection(&caster, &damaging.locate(far), &all_walkable()),
            Some(CastRejection::OutOfRange)
        );
        // Four tiles out: within range — no OutOfRange.
        let near = TileCoord::new(14, 10).to_world();
        assert_eq!(
            cast_rejection(&caster, &damaging.locate(near), &all_walkable()),
            None
        );
        // The gate is pure: same inputs, same answer, no RngCore in reach.
        assert_eq!(
            cast_rejection(&caster, &damaging.locate(far), &all_walkable()),
            cast_rejection(&caster, &damaging.locate(far), &all_walkable())
        );
    }

    #[test]
    fn a_caster_anchored_cast_never_rejects_out_of_range() {
        let caster = caster_at((10, 10), 100, 100);
        for definition in [
            area_skill(
                AreaGeometry::CasterCircle { radius_x2: 4 },
                AreaDisplacement::None,
                None,
                0,
            ),
            area_skill(
                AreaGeometry::Cone {
                    length_x2: 12,
                    half_angle: ConeHalfWidth::DEG_45,
                },
                AreaDisplacement::None,
                None,
                5,
            ),
            area_skill(
                AreaGeometry::Beam {
                    length_x2: 8,
                    half_width_x2: 3,
                },
                AreaDisplacement::None,
                None,
                6,
            ),
        ] {
            let damaging = damaging_ref(&definition);
            let absurd = TileCoord::new(250, 250).to_world();
            assert_eq!(
                cast_rejection(&caster, &damaging.locate(absurd), &all_walkable()),
                None
            );
        }
    }

    #[test]
    fn a_caster_anchored_cast_is_invariant_to_the_aim() {
        // The observable proof of the aim split: two absurdly different aims,
        // byte-identical outcomes under the same seed.
        let caster = caster_at((10, 10), 100, 100);
        let hellfire = area_skill(
            AreaGeometry::CasterCircle { radius_x2: 4 },
            AreaDisplacement::None,
            Some(Element::Fire),
            0,
        );
        let damaging = damaging_ref(&hellfire);
        let targets = [target_at((12, 10), 0)];
        let run = |aim: WorldPos| {
            let mut rng = TestRng::new(11);
            cast(
                &caster,
                &base_profile_of(&caster),
                damaging.locate(aim),
                &targets,
                &all_walkable(),
                &mut rng,
            )
        };
        assert_eq!(
            run(TileCoord::new(10, 10).to_world()),
            run(TileCoord::new(250, 3).to_world())
        );
    }

    #[test]
    fn the_displacement_dispatch_selects_per_skill_never_per_element() {
        // Earthshake (lightning) pushes; a lunge jiggles; a plain lightning
        // strike jiggles on the element; everything else is None.
        let quake = earthshake_skill();
        assert_eq!(
            target_displacement(damaging_ref(&quake)),
            TargetDisplacement::Push
        );
        let lunge_def = skill(SkillShape::Lunge, None, None, 2, 0, 0);
        assert_eq!(
            target_displacement(damaging_ref(&lunge_def)),
            TargetDisplacement::LungeJiggle
        );
        let bolt = skill(
            SkillShape::DirectHit,
            Some(Element::Lightning),
            None,
            6,
            0,
            0,
        );
        assert_eq!(
            target_displacement(damaging_ref(&bolt)),
            TargetDisplacement::ElementalJiggle
        );
        let lightning_area = area_skill(
            AreaGeometry::AimCircle { radius_x2: 2 },
            AreaDisplacement::None,
            Some(Element::Lightning),
            3,
        );
        assert_eq!(
            target_displacement(damaging_ref(&lightning_area)),
            TargetDisplacement::ElementalJiggle
        );
        let plain = skill(SkillShape::DirectHit, Some(Element::Fire), None, 6, 0, 0);
        assert_eq!(
            target_displacement(damaging_ref(&plain)),
            TargetDisplacement::None
        );
        let non_elemental = skill(SkillShape::DirectHit, None, None, 6, 0, 0);
        assert_eq!(
            target_displacement(damaging_ref(&non_elemental)),
            TargetDisplacement::None
        );
    }

    #[test]
    fn the_jiggle_nudges_within_one_tile_over_a_spread_of_directions() {
        // The continuous jiggle always nudges ~1 tile on open ground (the old
        // stay(0,0) outcome is gone — a heading is always drawn), each axis
        // bounded to one tile, and it reaches a spread of directions across all
        // four quadrants — not a snapped 8-way hop.
        let target = placed_at((11, 10));
        let grid = all_walkable();
        let mut quadrants = [false; 4];
        let mut distinct = std::collections::BTreeSet::new();
        for seed in 0u64..512 {
            let mut rng = TestRng::new(seed);
            let moved = jiggle(target, &grid, &mut rng).expect("open ground always nudges");
            let dx = moved.position.x().raw() - target.position.x().raw();
            let dy = moved.position.y().raw() - target.position.y().raw();
            assert!(
                dx.abs() <= TILE && dy.abs() <= TILE,
                "seed {seed}: within one tile per axis"
            );
            assert_ne!(
                moved.position, target.position,
                "seed {seed}: a nudge moves"
            );
            if dx > 0 && dy > 0 {
                quadrants[0] = true;
            }
            if dx < 0 && dy > 0 {
                quadrants[1] = true;
            }
            if dx < 0 && dy < 0 {
                quadrants[2] = true;
            }
            if dx > 0 && dy < 0 {
                quadrants[3] = true;
            }
            distinct.insert((dx, dy));
        }
        assert!(
            quadrants.iter().all(|&hit| hit),
            "every quadrant reached: {quadrants:?}"
        );
        assert!(
            distinct.len() > 8,
            "a continuous spread, not an 8-way hop: {}",
            distinct.len()
        );
    }

    #[test]
    fn the_jiggle_is_deterministic_per_seed_whether_blocked_or_open() {
        // The continuous jiggle draws a variable (heading-dependent) number of
        // words, but the same seed reproduces the same outcome and consumes the
        // same words bit-for-bit — on open ground and on a sealed grid alike (a
        // blocked destination is a stay, never a re-roll).
        let mut words = [0u64; 1024];
        let bit = (10usize << 8) | usize::from(11u8);
        words[bit >> 6] |= 1u64 << (bit & 63);
        let sealed = TerrainGrid::from_words(words);
        let target = placed_at((11, 10));
        for grid in [&all_walkable(), &sealed] {
            for seed in 0u64..16 {
                let mut a = TestRng::new(seed);
                let mut b = TestRng::new(seed);
                assert_eq!(
                    jiggle(target, grid, &mut a),
                    jiggle(target, grid, &mut b),
                    "seed {seed}"
                );
                // Equal words consumed: the next word still agrees.
                assert_eq!(a.next_u64(), b.next_u64(), "seed {seed}");
            }
        }
    }

    #[test]
    fn the_push_throws_three_tiles_directly_away() {
        // Two tiles east of the attacker, thrown three tiles further along the
        // attacker→target line to (15,10). The push is rng-free by signature.
        let attacker = placed_at((10, 10));
        let target = placed_at((12, 10));
        let moved = directional_push(attacker, target, &all_walkable())
            .expect("open ground: the push moves");
        assert_eq!(moved.position, TileCoord::new(15, 10).to_world());
    }

    #[test]
    fn the_push_follows_the_real_diagonal_angle_three_tiles() {
        // A diagonal away-vector is swept at its real angle (no whole-tile
        // snap): the target lands three tiles straight-line away — ~2.12 tiles
        // on each axis — not a ~4.24-tile diagonal throw to a whole-tile corner.
        let attacker = placed_at((10, 10));
        let target = placed_at((12, 12));
        let start = target.position;
        let moved = directional_push(attacker, target, &all_walkable())
            .expect("open ground: the push moves");
        assert_eq!(moved.position, WorldPos::clamped(958_223, 958_223));
        let dx = moved.position.x().raw() - start.x().raw();
        let dy = moved.position.y().raw() - start.y().raw();
        assert_eq!(dx, dy, "equal offset per axis — the true diagonal");
        assert!(dx > 0);
        // Straight-line displacement is exactly three tiles (not the diagonal's
        // 3·√2 ≈ 4.24), and its per-axis Chebyshev reach is only two tiles.
        assert_eq!(start.distance_sq(moved.position).isqrt(), 3 * 65_536);
        assert_eq!(dx / TILE, 2);
    }

    #[test]
    fn the_push_stops_at_the_first_unwalkable_tile_keeping_gained_ground() {
        // (13,10) and (14,10) walkable, (15,10) blocked: two steps kept.
        let mut words = [0u64; 1024];
        for x in [12u8, 13, 14] {
            let bit = (10usize << 8) | usize::from(x);
            words[bit >> 6] |= 1u64 << (bit & 63);
        }
        let grid = TerrainGrid::from_words(words);
        let attacker = placed_at((10, 10));
        let target = placed_at((12, 10));
        let moved = directional_push(attacker, target, &grid).expect("two walkable steps are kept");
        assert_eq!(moved.position, TileCoord::new(14, 10).to_world());
    }

    #[test]
    fn the_push_reports_none_when_the_first_tile_is_blocked() {
        // Only the target's own tile walkable: no ground gained.
        let mut words = [0u64; 1024];
        let bit = (10usize << 8) | usize::from(12u8);
        words[bit >> 6] |= 1u64 << (bit & 63);
        let grid = TerrainGrid::from_words(words);
        let attacker = placed_at((10, 10));
        let target = placed_at((12, 10));
        assert_eq!(directional_push(attacker, target, &grid), None);
    }

    #[test]
    fn a_same_point_target_is_not_pushed() {
        // Attacker and target on the exact same point: no direction, no push —
        // a clean `None`, never a random fallback heading.
        let attacker = placed_at((10, 10));
        let target = placed_at((10, 10));
        assert_eq!(directional_push(attacker, target, &all_walkable()), None);
    }

    /// The index a hit reports, on every variant.
    fn hit_index(hit: &TargetHit) -> usize {
        match hit {
            TargetHit::Missed { target_index, .. }
            | TargetHit::Landed { target_index, .. }
            | TargetHit::Killed { target_index, .. } => *target_index,
        }
    }

    #[test]
    fn a_safezone_caster_is_rejected_before_any_spend() {
        // FIRE-1: a funded caster on a safe tile, valid in-range target — the
        // cast is refused CasterInSafezone, nothing spent, no word drawn.
        let caster = caster_at((10, 10), 100, 100);
        let definition = skill(SkillShape::DirectHit, None, None, 3, 30, 10);
        let damaging = damaging_ref(&definition);
        let targets = [target_at((11, 10), 0)];
        let aim = TileCoord::new(11, 10).to_world();
        let grid = walkable_with_safe(&[(10, 10)]);
        let mut rng = TestRng::new(5);
        let (vitals, outcome) = cast(
            &caster,
            &base_profile_of(&caster),
            damaging.locate(aim),
            &targets,
            &grid,
            &mut rng,
        );
        assert_eq!(vitals, caster.vitals(), "nothing spent");
        assert_eq!(
            outcome,
            SkillOutcome::Rejected {
                reason: CastRejection::CasterInSafezone
            }
        );
        let mut probe = TestRng::new(5);
        assert_eq!(rng.next_u64(), probe.next_u64(), "no word drawn");
    }

    #[test]
    fn the_safezone_term_precedes_affordability_and_the_aim_gate() {
        // White-box order: a broke caster aiming out of range from a safe tile
        // surfaces CasterInSafezone — the locus gate dominates.
        let broke = caster_at((10, 10), 0, 0);
        let definition = skill(SkillShape::DirectHit, None, None, 2, 50, 50);
        let damaging = damaging_ref(&definition);
        let far = TileCoord::new(30, 10).to_world();
        assert_eq!(
            cast_rejection(
                &broke,
                &damaging.locate(far),
                &walkable_with_safe(&[(10, 10)])
            ),
            Some(CastRejection::CasterInSafezone)
        );
    }

    #[test]
    fn a_safezone_standing_target_is_dropped_from_the_covered_set() {
        // FIRE-2: both targets are geometrically covered; the safe-tile one
        // produces no TargetHit while the open-ground one is struck.
        let caster = caster_at((10, 10), 100, 100);
        let quake = area_skill(
            AreaGeometry::CasterCircle { radius_x2: 10 },
            AreaDisplacement::None,
            None,
            10,
        );
        let damaging = damaging_ref(&quake);
        let targets = [target_at((11, 10), 0), target_at((12, 10), 0)];
        let aim = TileCoord::new(10, 10).to_world();
        let grid = walkable_with_safe(&[(11, 10)]);
        for seed in 0u64..8 {
            let mut rng = TestRng::new(seed);
            let SkillOutcome::Cast { hits, .. } = cast(
                &caster,
                &base_profile_of(&caster),
                damaging.locate(aim),
                &targets,
                &grid,
                &mut rng,
            )
            .1
            else {
                panic!("the open-ground target keeps the cast resolving");
            };
            assert_eq!(hits.len(), 1, "seed {seed}");
            assert_eq!(hit_index(&hits[0]), 1, "seed {seed}");
        }
    }

    #[test]
    fn a_direct_hit_is_refused_on_both_sides_of_the_safezone_line() {
        // FIRE-3: the attacker-in-safezone cast is rejected CasterInSafezone;
        // the target-in-safezone cast strikes no one. Nothing spent either way.
        let definition = skill(SkillShape::DirectHit, None, None, 3, 20, 5);
        let damaging = damaging_ref(&definition);
        let aim = TileCoord::new(11, 10).to_world();
        let targets = [target_at((11, 10), 0)];

        let attacker_safe = caster_at((10, 10), 100, 100);
        let mut rng = TestRng::new(4);
        let (vitals, outcome) = cast(
            &attacker_safe,
            &base_profile_of(&attacker_safe),
            damaging.locate(aim),
            &targets,
            &walkable_with_safe(&[(10, 10)]),
            &mut rng,
        );
        assert_eq!(vitals, attacker_safe.vitals());
        assert_eq!(
            outcome,
            SkillOutcome::Rejected {
                reason: CastRejection::CasterInSafezone
            }
        );

        let attacker_open = caster_at((10, 10), 100, 100);
        let mut rng = TestRng::new(4);
        let (vitals, outcome) = cast(
            &attacker_open,
            &base_profile_of(&attacker_open),
            damaging.locate(aim),
            &targets,
            &walkable_with_safe(&[(11, 10)]),
            &mut rng,
        );
        assert_eq!(vitals, attacker_open.vitals());
        assert_eq!(
            outcome,
            SkillOutcome::Rejected {
                reason: CastRejection::NoTargetsInRegion
            }
        );
    }

    #[test]
    fn the_push_stops_at_a_safe_tile_keeping_gained_ground() {
        // FIRE-4: swept +x from (12,10); (13,10) is open, (14,10) is safe — the
        // safe tile refuses the increment like a wall, prior ground kept.
        let attacker = placed_at((10, 10));
        let target = placed_at((12, 10));
        let moved = directional_push(attacker, target, &walkable_with_safe(&[(14, 10)]))
            .expect("one open step is kept");
        assert_eq!(moved.position, TileCoord::new(13, 10).to_world());
    }

    #[test]
    fn the_push_reports_none_when_the_first_tile_is_safe() {
        // A safe opening increment gains no ground — `None`, exactly like the wall.
        let attacker = placed_at((10, 10));
        let target = placed_at((12, 10));
        assert_eq!(
            directional_push(attacker, target, &walkable_with_safe(&[(13, 10)])),
            None
        );
    }

    #[test]
    fn a_jiggle_onto_a_safe_destination_stays() {
        // FIRE-6: every neighbouring tile is safe, so every drawn heading nudges
        // onto a safe tile and is refused — no move, deterministic per seed, no
        // re-roll.
        let target = placed_at((11, 10));
        let grid = safe_everywhere_but(&[(11, 10)]);
        for seed in 0u64..16 {
            let mut a = TestRng::new(seed);
            let mut b = TestRng::new(seed);
            assert_eq!(jiggle(target, &grid, &mut a), None, "seed {seed}");
            assert_eq!(jiggle(target, &grid, &mut b), None, "seed {seed}");
            // Deterministic draw: both runs consumed the same words.
            assert_eq!(a.next_u64(), b.next_u64(), "seed {seed}");
        }
    }

    #[test]
    fn the_firewall_is_inert_where_no_safe_tile_is_touched() {
        // FIRE-7: with the only safe tile far away, cast, push, and jiggle are
        // byte-identical to the walk-only grid under every seed.
        let caster = caster_at((10, 10), 100, 100);
        let remote = walkable_with_safe(&[(200, 200)]);
        let quake = earthshake_skill();
        let bolt = skill(
            SkillShape::DirectHit,
            Some(Element::Lightning),
            None,
            3,
            0,
            0,
        );
        let lunge_def = skill(SkillShape::Lunge, None, None, 4, 0, 0);
        for definition in [&quake, &bolt, &lunge_def] {
            let damaging = damaging_ref(definition);
            let targets = [target_at((12, 10), 0)];
            let aim = TileCoord::new(12, 10).to_world();
            for seed in 0u64..8 {
                let open = cast(
                    &caster,
                    &base_profile_of(&caster),
                    damaging.locate(aim),
                    &targets,
                    &all_walkable(),
                    &mut TestRng::new(seed),
                );
                let remote_safe = cast(
                    &caster,
                    &base_profile_of(&caster),
                    damaging.locate(aim),
                    &targets,
                    &remote,
                    &mut TestRng::new(seed),
                );
                assert_eq!(open, remote_safe, "seed {seed}");
            }
        }
    }

    #[test]
    fn earthshake_pushes_missed_and_landed_targets_but_never_killed_ones() {
        let caster = caster_at((10, 10), 100, 100);
        let quake = earthshake_skill();
        let damaging = damaging_ref(&quake);
        let aim = TileCoord::new(10, 10).to_world();
        let start = TileCoord::new(12, 10).to_world();

        // Sturdy target: missed and landed hits both push over open ground.
        let sturdy = [target_at((12, 10), 0)];
        let mut saw_missed = false;
        let mut saw_landed = false;
        for seed in 0u64..64 {
            let mut rng = TestRng::new(seed);
            let SkillOutcome::Cast { hits, .. } = cast(
                &caster,
                &base_profile_of(&caster),
                damaging.locate(aim),
                &sturdy,
                &all_walkable(),
                &mut rng,
            )
            .1
            else {
                panic!("earthshake should resolve");
            };
            match hits[0] {
                TargetHit::Missed { displacement, .. } => {
                    let moved = displacement.expect("a missed quake still scatters");
                    assert_ne!(moved.position, start);
                    saw_missed = true;
                }
                TargetHit::Landed { displacement, .. } => {
                    let moved = displacement.expect("a landed quake scatters");
                    assert_ne!(moved.position, start);
                    saw_landed = true;
                }
                TargetHit::Killed { .. } => {}
            }
        }
        assert!(saw_missed && saw_landed, "both outcomes reached in 0..64");

        // Frail target: a kill is never pushed (Killed carries no field) and
        // the element roll is skipped, so the strike is the only consumption.
        let frail = [target_with((12, 10), 0, 0, 1, ActiveEffects::EMPTY)];
        let mut saw_kill = false;
        for seed in 0u64..32 {
            let mut rng = TestRng::new(seed);
            if let SkillOutcome::Cast { hits, .. } = cast(
                &caster,
                &base_profile_of(&caster),
                damaging.locate(aim),
                &frail,
                &all_walkable(),
                &mut rng,
            )
            .1
            {
                if matches!(hits[0], TargetHit::Killed { .. }) {
                    saw_kill = true;
                }
            }
        }
        assert!(saw_kill, "a landing strike kills the 1-HP victim");
    }

    #[test]
    fn earthshake_draws_no_element_roll_on_a_landed_hit() {
        // The inert-lightning pin: against a fully inert twin (same strike,
        // no element, no displacement) the quake's word stream is identical —
        // Earthshake skips the element roll and its off-tile push draws
        // nothing, so the strike sequence is the whole consumption for both.
        let caster = caster_at((10, 10), 100, 100);
        let quake = earthshake_skill();
        let inert_twin = area_skill(
            AreaGeometry::CasterCircle { radius_x2: 10 },
            AreaDisplacement::None,
            None,
            10,
        );
        let targets = [target_at((12, 10), 0)];
        let aim = TileCoord::new(10, 10).to_world();
        let mut compared = false;
        for seed in 0u64..32 {
            let mut quake_rng = TestRng::new(seed);
            let SkillOutcome::Cast { hits, .. } = cast(
                &caster,
                &base_profile_of(&caster),
                damaging_ref(&quake).locate(aim),
                &targets,
                &all_walkable(),
                &mut quake_rng,
            )
            .1
            else {
                panic!("earthshake should resolve");
            };
            if !matches!(hits[0], TargetHit::Landed { .. }) {
                continue;
            }
            let mut twin_rng = TestRng::new(seed);
            let _ = cast(
                &caster,
                &base_profile_of(&caster),
                damaging_ref(&inert_twin).locate(aim),
                &targets,
                &all_walkable(),
                &mut twin_rng,
            );
            assert_eq!(
                quake_rng.next_u64(),
                twin_rng.next_u64(),
                "seed {seed}: the quake drew a word beyond the strike sequence"
            );
            compared = true;
        }
        assert!(compared, "a seed in 0..32 lands a non-lethal quake hit");
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
            cast(
                &caster,
                &base_profile_of(&caster),
                lunge.locate(aim),
                &targets,
                &all_walkable(),
                &mut rng,
            )
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

    /// A direct-hit skill of the given damage type and authored damage `D`.
    fn typed_skill(damage_type: DamageType, attack_damage: u16) -> Skill {
        let mut definition = skill(SkillShape::DirectHit, None, None, 6, 0, 0);
        definition.damage_type = damage_type;
        definition.attack_damage = attack_damage;
        definition
    }

    /// The strike basis a caster/skill pair selects, derived exactly as
    /// `cast()` derives it — through the caster's own effect-folded profile.
    fn basis_of(caster: &Character, definition: &Skill) -> StrikeBasis {
        let profile = effective_profile(character_profile(caster).0, &caster.active_effects());
        skill_strike_basis(caster, &profile, damaging_ref(definition))
    }

    #[test]
    fn a_physical_skill_augments_the_weapon_span_asymmetrically() {
        // DK str 200 → weapon span [33, 50]; D = 60 adds +60/+90.
        let caster = caster_of("dark_knight", 200, 30, (10, 10));
        assert_eq!(
            basis_of(&caster, &typed_skill(DamageType::Physical, 60)),
            StrikeBasis::Skill {
                span: Interval::new(93, 140).unwrap(),
                excellent_order: ExcellentOrder::MultiplyThenDefense,
                multiplier_per_mille: 2030,
            }
        );
        // A zero-damage weapon skill rolls the bare weapon span.
        assert_eq!(
            basis_of(&caster, &typed_skill(DamageType::Physical, 0)),
            StrikeBasis::Skill {
                span: Interval::new(33, 50).unwrap(),
                excellent_order: ExcellentOrder::MultiplyThenDefense,
                multiplier_per_mille: 2030,
            }
        );
    }

    #[test]
    fn a_wizardry_skill_selects_the_energy_scaled_wizardry_span() {
        // DW energy 100 → wizardry [11, 25]; + D = 45 → [56, 92]. The physical
        // span (str 40 → [5, 10]) never enters.
        let low = caster_of("dark_wizard", 40, 100, (10, 10));
        assert_eq!(
            basis_of(&low, &typed_skill(DamageType::Wizardry, 45)),
            StrikeBasis::Skill {
                span: Interval::new(56, 92).unwrap(),
                excellent_order: ExcellentOrder::DefenseThenMultiply,
                multiplier_per_mille: 1000,
            }
        );
        // Energy 400 → wizardry [44, 100]; both span ends scale with Energy.
        let high = caster_of("dark_wizard", 40, 400, (10, 10));
        assert_eq!(
            basis_of(&high, &typed_skill(DamageType::Wizardry, 45)),
            StrikeBasis::Skill {
                span: Interval::new(89, 167).unwrap(),
                excellent_order: ExcellentOrder::DefenseThenMultiply,
                multiplier_per_mille: 1000,
            }
        );
    }

    #[test]
    fn a_wizardry_cast_without_wizardry_collapses_the_whole_span() {
        // A DK carries no wizardry interval: base AND add collapse to [0, 0];
        // the D = 45 is discarded, never a physical fallback.
        let caster = caster_of("dark_knight", 200, 30, (10, 10));
        assert_eq!(
            basis_of(&caster, &typed_skill(DamageType::Wizardry, 45)),
            StrikeBasis::Skill {
                span: Interval::new(0, 0).unwrap(),
                excellent_order: ExcellentOrder::DefenseThenMultiply,
                multiplier_per_mille: 2030,
            }
        );
    }

    #[test]
    fn a_none_type_skill_selects_no_span_and_discards_its_damage() {
        let caster = caster_of("dark_knight", 200, 30, (10, 10));
        assert_eq!(
            basis_of(&caster, &typed_skill(DamageType::None, 120)),
            StrikeBasis::Skill {
                span: Interval::new(0, 0).unwrap(),
                excellent_order: ExcellentOrder::MultiplyThenDefense,
                multiplier_per_mille: 2030,
            }
        );
    }

    #[test]
    fn the_skill_multiplier_is_per_mille_and_integer_per_class() {
        use crate::components::class::CharacterClass as Class;
        assert_eq!(skill_multiplier_per_mille(Class::DarkWizard, 300), 1000);
        assert_eq!(skill_multiplier_per_mille(Class::SoulMaster, 300), 1000);
        assert_eq!(skill_multiplier_per_mille(Class::FairyElf, 300), 1000);
        assert_eq!(skill_multiplier_per_mille(Class::MuseElf, 300), 1000);
        assert_eq!(skill_multiplier_per_mille(Class::MagicGladiator, 300), 2000);
        assert_eq!(skill_multiplier_per_mille(Class::DarkKnight, 30), 2030);
        assert_eq!(skill_multiplier_per_mille(Class::BladeKnight, 500), 2500);
        // DL is + Energy/2, integer floor: 101/2 = 50.
        assert_eq!(skill_multiplier_per_mille(Class::DarkLord, 101), 2050);
    }

    #[test]
    fn the_multiplier_reads_total_energy_on_both_stat_shapes() {
        // Standard shape (DK) and WithCommand shape (DL) both feed the energy
        // micro-term through the same caster_energy read.
        let knight = caster_of("dark_knight", 200, 500, (10, 10));
        assert!(matches!(
            basis_of(&knight, &typed_skill(DamageType::Physical, 0)),
            StrikeBasis::Skill {
                multiplier_per_mille: 2500,
                ..
            }
        ));
        let lord = caster_of("dark_lord", 200, 100, (10, 10));
        assert!(matches!(
            basis_of(&lord, &typed_skill(DamageType::Physical, 0)),
            StrikeBasis::Skill {
                multiplier_per_mille: 2050,
                ..
            }
        ));
    }

    #[test]
    fn the_staff_rise_multiplies_the_whole_augmented_span() {
        // EQ-STAFF-1/2: DW wizardry [11, 25]; Legendary Staff rise_x2 = 67 →
        // ×267/200. A plain (D = 0) span scales to [14, 33]; the D = 45
        // augmented span [56, 92] scales to [74, 122] — the rise wraps the
        // WHOLE (WizBase + D) parenthesis, after augmented_span.
        let caster = caster_of("dark_wizard", 40, 100, (10, 10));
        let geared = CombatProfile {
            wizardry_rise_x2: 67,
            ..character_profile(&caster).0
        };
        assert_eq!(
            skill_strike_basis(
                &caster,
                &geared,
                damaging_ref(&typed_skill(DamageType::Wizardry, 0))
            ),
            StrikeBasis::Skill {
                span: Interval::new(14, 33).unwrap(),
                excellent_order: ExcellentOrder::DefenseThenMultiply,
                multiplier_per_mille: 1000,
            }
        );
        assert_eq!(
            skill_strike_basis(
                &caster,
                &geared,
                damaging_ref(&typed_skill(DamageType::Wizardry, 45))
            ),
            StrikeBasis::Skill {
                span: Interval::new(74, 122).unwrap(),
                excellent_order: ExcellentOrder::DefenseThenMultiply,
                multiplier_per_mille: 1000,
            }
        );
        // The gearless rise (0) is the ×1 identity.
        assert_eq!(
            basis_of(&caster, &typed_skill(DamageType::Wizardry, 45)),
            StrikeBasis::Skill {
                span: Interval::new(56, 92).unwrap(),
                excellent_order: ExcellentOrder::DefenseThenMultiply,
                multiplier_per_mille: 1000,
            }
        );
    }

    #[test]
    fn the_rise_never_touches_a_physical_skill_span() {
        // A rise-carrying profile (an MG with a staff and a physical skill)
        // leaves the physical basis exactly as augmented.
        let caster = caster_of("dark_knight", 200, 30, (10, 10));
        let geared = CombatProfile {
            wizardry_rise_x2: 67,
            ..character_profile(&caster).0
        };
        assert_eq!(
            skill_strike_basis(
                &caster,
                &geared,
                damaging_ref(&typed_skill(DamageType::Physical, 60))
            ),
            StrikeBasis::Skill {
                span: Interval::new(93, 140).unwrap(),
                excellent_order: ExcellentOrder::MultiplyThenDefense,
                multiplier_per_mille: 2030,
            }
        );
    }

    #[test]
    fn a_double_wielded_skill_halves_its_flat_d() {
        // EQ-DW-3: Rageful-Blow-shaped D = 60 uses D = 30 when double-wielding
        // — over the ×0.55-folded span [55, 110]: [85, 155] (min +30, max
        // +30+15), so the head's ×2 keeps the skill's punch at net ×1.
        let caster = caster_of("dark_knight", 200, 30, (10, 10));
        let base = character_profile(&caster).0;
        let dual = CombatProfile {
            physical: Interval::new(55, 110).unwrap(),
            weapon_mode: WeaponMode::DoubleWield,
            ..base
        };
        assert_eq!(
            skill_strike_basis(
                &caster,
                &dual,
                damaging_ref(&typed_skill(DamageType::Physical, 60))
            ),
            StrikeBasis::Skill {
                span: Interval::new(85, 155).unwrap(),
                excellent_order: ExcellentOrder::MultiplyThenDefense,
                multiplier_per_mille: 2030,
            }
        );
        // Single-wielding the same span keeps the full D: [115, 200].
        let single = CombatProfile {
            physical: Interval::new(55, 110).unwrap(),
            ..base
        };
        assert_eq!(
            skill_strike_basis(
                &caster,
                &single,
                damaging_ref(&typed_skill(DamageType::Physical, 60))
            ),
            StrikeBasis::Skill {
                span: Interval::new(115, 200).unwrap(),
                excellent_order: ExcellentOrder::MultiplyThenDefense,
                multiplier_per_mille: 2030,
            }
        );
    }

    #[test]
    fn the_basis_selection_is_pure() {
        // No RngCore in any signature on the seam; two derivations from the
        // same inputs are identical values.
        let caster = caster_of("dark_wizard", 40, 100, (10, 10));
        let definition = typed_skill(DamageType::Wizardry, 45);
        assert_eq!(
            basis_of(&caster, &definition),
            basis_of(&caster, &definition)
        );
    }

    #[test]
    fn a_wizardry_cast_strikes_the_selected_span_through_cast() {
        // DW energy 100, physical span [5, 10]: a landed wizardry D=45 hit lies
        // in the augmented wizardry span [56, 92] (multiplier 1000, defense 0)
        // — far above the weapon span it never reads.
        let caster = caster_of("dark_wizard", 40, 100, (10, 10));
        let definition = typed_skill(DamageType::Wizardry, 45);
        let damaging = damaging_ref(&definition);
        let targets = [target_at((11, 10), 0)];
        let aim = TileCoord::new(11, 10).to_world();
        let mut landed = 0u32;
        for seed in 0u64..32 {
            let outcome = cast(
                &caster,
                &base_profile_of(&caster),
                damaging.locate(aim),
                &targets,
                &all_walkable(),
                &mut TestRng::new(seed),
            )
            .1;
            let Some(damage) = landed_damage(outcome) else {
                continue;
            };
            assert!(
                (56..=92).contains(&damage),
                "seed {seed}: {damage} outside the augmented wizardry span"
            );
            landed += 1;
        }
        assert!(landed > 0, "a seed in 0..32 lands a wizardry hit");
    }

    #[test]
    fn a_physical_cast_exceeds_the_same_casters_plain_swing() {
        // Rageful-Blow-shaped D=60 on a DK: the skill's span add and the class
        // multiplier both raise a landed hit above the plain swing of the same
        // weapon under the identical seed and target.
        let caster = caster_of("dark_knight", 200, 30, (10, 10));
        let definition = typed_skill(DamageType::Physical, 60);
        let damaging = damaging_ref(&definition);
        let targets = [target_at((11, 10), 0)];
        let aim = TileCoord::new(11, 10).to_world();
        let caster_profile = character_profile(&caster).0;
        let mut compared = 0u32;
        for seed in 0u64..32 {
            let skill_dmg = landed_damage(
                cast(
                    &caster,
                    &base_profile_of(&caster),
                    damaging.locate(aim),
                    &targets,
                    &all_walkable(),
                    &mut TestRng::new(seed),
                )
                .1,
            );
            let mut rng = TestRng::new(seed);
            let (_, plain_outcome) = resolve_attack(
                &caster_profile,
                targets[0].profile(),
                targets[0].health(),
                &StrikeBasis::PlainSwing,
                &mut rng,
            );
            let plain_dmg = match plain_outcome {
                AttackOutcome::Landed { hit } | AttackOutcome::Killed { hit } => Some(hit.damage.0),
                AttackOutcome::Missed => None,
            };
            let (Some(skill_dmg), Some(plain_dmg)) = (skill_dmg, plain_dmg) else {
                continue;
            };
            assert!(
                skill_dmg > plain_dmg,
                "seed {seed}: skill {skill_dmg} must exceed plain {plain_dmg}"
            );
            compared += 1;
        }
        assert!(compared > 0, "a seed in 0..32 lands both strikes");
    }
}
