//! The resolved combat snapshot a service reads to strike: a fighter's
//! offensive and defensive magnitudes, its per-element resistances, and its four
//! special-hit chances — plus the player-only derived vital capacities and a
//! resolvable target snapshot. Data only: the profile service derives these from
//! a character or a monster definition; nothing here decides or rolls.

use serde::{Deserialize, Serialize};

use crate::components::active_effect::ActiveEffects;
use crate::components::element::{Element, PerElement};
use crate::components::interval::Interval;
use crate::components::placement::Placement;
use crate::components::pool::Pool;
use crate::components::units::{Level, Percent, Resistance};

/// Whether a combatant is a player or a non-player (monster/NPC). Consumed by
/// combat math (the player-versus-player overrate matchup) and skill targeting
/// (an incidental area hit lands on non-players only); it is a combat category,
/// not an identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    /// A player character.
    Player,
    /// A non-player combatant: a monster or NPC.
    Npc,
}

/// A fighter's resolved combat magnitudes — the view a strike reads. Physical
/// damage is an inclusive span; wizardry is present only for spellcasters. The
/// four special-hit chances are gearless zero until an equipment wave feeds
/// them. No live-health field: current health travels beside the profile (a
/// [`CombatTarget`] for a defender, the caller's own pool for an attacker).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatProfile {
    /// Player or non-player. No serde default — a combatant is always exactly
    /// one kind; fabricating one would corrupt the overrate matchup.
    pub(crate) kind: TargetKind,
    pub(crate) level: Level,
    pub(crate) physical: Interval<u16>,
    pub(crate) wizardry: Option<Interval<u16>>,
    pub(crate) defense: u16,
    pub(crate) attack_rate: u16,
    pub(crate) defense_rate: u16,
    pub(crate) resistances: PerElement<Resistance>,
    pub(crate) critical_chance: Percent,
    pub(crate) excellent_chance: Percent,
    pub(crate) defense_ignore_chance: Percent,
    pub(crate) double_damage_chance: Percent,
    pub(crate) incoming_damage_reduction: Percent,
    pub(crate) flat_damage_add: u32,
    /// Staff rise, doubled (`magic_power + 2·curve`) — the wizardry-span
    /// multiplier `(200 + rise_x2)/200` the skill seam applies; `0` is the ×1
    /// identity. Serde-defaulted so pre-gear persisted profiles still parse.
    #[serde(default)]
    pub(crate) wizardry_rise_x2: u16,
    /// Defender excellent `DamageDecrease`, applied PRE-floor. Gearless zero.
    #[serde(default = "Percent::zero")]
    pub(crate) incoming_dd_pct: Percent,
    /// Attacker wing damage increase, applied POST-floor. Gearless zero.
    #[serde(default = "Percent::zero")]
    pub(crate) wing_damage_pct: Percent,
    /// Defender wing absorb, applied POST-floor (skipped when the damage is
    /// 1 or less). Gearless zero.
    #[serde(default = "Percent::zero")]
    pub(crate) wing_absorb_pct: Percent,
    /// Single or double-wielding — the typed fact the physical strike head's
    /// pre-defense ×2 reads.
    #[serde(default = "WeaponMode::single")]
    pub(crate) weapon_mode: WeaponMode,
}

/// Whether the attacker wields one weapon (or none) or two one-handers — the
/// typed double-wield fact, never a bare bool controlling flow. Read by the
/// physical strike head to apply the authentic pre-defense ×2, and by the
/// skill seam to halve a double-wielded skill's flat `D`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeaponMode {
    /// One weapon or none: the physical span stands as assembled; no
    /// pre-defense doubling. A gearless fighter and every monster are
    /// `Single` — a real domain value, not a fabricated default.
    Single,
    /// Two one-handed swords/axes/maces (the DK/MG dual wield): the physical
    /// span was already ×55/100-assembled in the equipment fold; the strike
    /// doubles the physical quality base before defense (net 110%), and a
    /// double-wielded skill's flat `D` is halved.
    DoubleWield,
}

impl WeaponMode {
    /// The single-weapon value — a real domain value used as the serde default
    /// and by every gearless / monster construction site.
    #[must_use]
    pub const fn single() -> Self {
        Self::Single
    }
}

impl CombatProfile {
    /// Whether the fighter is a player or a non-player — the combat category the
    /// overrate matchup and incidental area-targeting read.
    #[must_use]
    pub fn kind(&self) -> TargetKind {
        self.kind
    }

    /// The fighter's level — sets the min-damage floor.
    #[must_use]
    pub fn level(&self) -> Level {
        self.level
    }

    /// The inclusive physical-damage span.
    #[must_use]
    pub fn physical(&self) -> Interval<u16> {
        self.physical
    }

    /// The wizardry-damage span, present only for spellcasters.
    #[must_use]
    pub fn wizardry(&self) -> Option<Interval<u16>> {
        self.wizardry
    }

    /// Defense subtracted from incoming physical damage.
    #[must_use]
    pub fn defense(&self) -> u16 {
        self.defense
    }

    /// Attack success rate — drives the hit chance and the overrate penalty.
    #[must_use]
    pub fn attack_rate(&self) -> u16 {
        self.attack_rate
    }

    /// Defense success rate — drives the hit chance and the overrate penalty.
    #[must_use]
    pub fn defense_rate(&self) -> u16 {
        self.defense_rate
    }

    /// Resistance to one element — delegates the total per-element lookup.
    #[must_use]
    pub fn resistance(&self, element: Element) -> Resistance {
        *self.resistances.of(element)
    }

    /// Chance a hit rolls critical.
    #[must_use]
    pub fn critical_chance(&self) -> Percent {
        self.critical_chance
    }

    /// Chance a hit rolls excellent.
    #[must_use]
    pub fn excellent_chance(&self) -> Percent {
        self.excellent_chance
    }

    /// Chance a hit ignores the target's defense.
    #[must_use]
    pub fn defense_ignore_chance(&self) -> Percent {
        self.defense_ignore_chance
    }

    /// Chance a hit deals double damage.
    #[must_use]
    pub fn double_damage_chance(&self) -> Percent {
        self.double_damage_chance
    }

    /// Percentage the defender reduces incoming damage by — the transient
    /// contribution timed defensive buffs fold in (gearless zero). Applied as
    /// the final defender-side step of a strike.
    #[must_use]
    pub fn incoming_damage_reduction(&self) -> Percent {
        self.incoming_damage_reduction
    }

    /// Flat damage the attacker adds after defense subtraction and before the
    /// min-damage floor — the transient contribution a Greater Damage buff folds
    /// in (gearless zero). Unlike a physical-span raise, the quality-selected
    /// base (which crit/excellent set) is fixed before this add, so crit and
    /// excellent never amplify it.
    #[must_use]
    pub fn flat_damage_add(&self) -> u32 {
        self.flat_damage_add
    }

    /// Staff rise, doubled — the wizardry-span multiplier numerator over 200
    /// the skill seam applies to the whole `(WizBase + D)` parenthesis. `0` is
    /// the gearless ×1 identity.
    #[must_use]
    pub fn wizardry_rise_x2(&self) -> u16 {
        self.wizardry_rise_x2
    }

    /// Percentage the defender's excellent `DamageDecrease` removes, applied
    /// before the minimum-damage floor (gearless zero).
    #[must_use]
    pub fn incoming_dd_pct(&self) -> Percent {
        self.incoming_dd_pct
    }

    /// Percentage the attacker's wings add, applied after the minimum-damage
    /// floor (gearless zero).
    #[must_use]
    pub fn wing_damage_pct(&self) -> Percent {
        self.wing_damage_pct
    }

    /// Percentage the defender's wings absorb, applied after the
    /// minimum-damage floor and skipped when the damage is 1 or less
    /// (gearless zero).
    #[must_use]
    pub fn wing_absorb_pct(&self) -> Percent {
        self.wing_absorb_pct
    }

    /// Whether the fighter single- or double-wields.
    #[must_use]
    pub fn weapon_mode(&self) -> WeaponMode {
        self.weapon_mode
    }
}

/// A player's derived vital capacities — the maxima the class formula computes
/// from level and stats, returned beside a character's [`CombatProfile`]. Plain
/// data; the character carries no fabricated maxima of its own on the compute
/// path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VitalMaxima {
    /// Maximum health.
    pub max_health: u32,
    /// Maximum mana.
    pub max_mana: u32,
    /// Maximum ability (AG).
    pub max_ability: u32,
}

/// A resolvable defender snapshot: its combat profile, its current health, where
/// it stands, and its live timed effects. The host/orchestrator pre-derives one
/// per candidate so the cast service stays Atlas-free — it strikes what it is
/// handed and folds the defender's own effects (Greater Defense, DK Defense,
/// Defense-reduction) into the profile it strikes against, so the whole two-sided
/// fold is authoritative in core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatTarget {
    profile: CombatProfile,
    health: Pool,
    placement: Placement,
    /// The defender's live timed effects. A candidate a host builds without an
    /// effect array carries none — the real "no active effects" value.
    #[serde(default = "ActiveEffects::empty")]
    active_effects: ActiveEffects,
}

impl CombatTarget {
    /// Bundles a defender snapshot with its live timed effects.
    #[must_use]
    pub fn new(
        profile: CombatProfile,
        health: Pool,
        placement: Placement,
        active_effects: ActiveEffects,
    ) -> Self {
        Self {
            profile,
            health,
            placement,
            active_effects,
        }
    }

    /// The defender's combat profile.
    #[must_use]
    pub fn profile(&self) -> &CombatProfile {
        &self.profile
    }

    /// The defender's current health.
    #[must_use]
    pub fn health(&self) -> Pool {
        self.health
    }

    /// The defender's live timed effects — folded into the profile it is struck
    /// against, and cleared to [`ActiveEffects::EMPTY`] when the strike kills it.
    #[must_use]
    pub fn active_effects(&self) -> ActiveEffects {
        self.active_effects
    }

    /// Where the defender stands.
    #[must_use]
    pub fn placement(&self) -> Placement {
        self.placement
    }

    /// Resistance to one element — delegates to the profile.
    #[must_use]
    pub fn resistance(&self, element: Element) -> Resistance {
        self.profile.resistance(element)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_resistances() -> PerElement<Resistance> {
        PerElement {
            ice: Resistance(0),
            poison: Resistance(0),
            lightning: Resistance(7),
            fire: Resistance(0),
            earth: Resistance(0),
            wind: Resistance(0),
            water: Resistance(0),
        }
    }

    fn profile() -> CombatProfile {
        CombatProfile {
            kind: TargetKind::Npc,
            level: Level::new(30).unwrap(),
            physical: Interval::new(5u16, 10u16).unwrap(),
            wizardry: None,
            defense: 4,
            attack_rate: 100,
            defense_rate: 20,
            resistances: zero_resistances(),
            critical_chance: Percent::ZERO,
            excellent_chance: Percent::ZERO,
            defense_ignore_chance: Percent::ZERO,
            double_damage_chance: Percent::ZERO,
            incoming_damage_reduction: Percent::ZERO,
            flat_damage_add: 0,
            wizardry_rise_x2: 0,
            incoming_dd_pct: Percent::ZERO,
            wing_damage_pct: Percent::ZERO,
            wing_absorb_pct: Percent::ZERO,
            weapon_mode: WeaponMode::Single,
        }
    }

    #[test]
    fn resistance_delegates_the_per_element_lookup() {
        let p = profile();
        assert_eq!(p.resistance(Element::Lightning), Resistance(7));
        assert_eq!(p.resistance(Element::Fire), Resistance(0));
    }

    #[test]
    fn combat_target_bundles_and_exposes_its_parts() {
        use crate::components::movement::Movement;
        use crate::components::spatial::Facing;
        use crate::components::tile::TileCoord;
        use crate::components::units::MapNumber;

        let placement = Placement {
            position: TileCoord::new(2, 3).to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: MapNumber(0),
        };
        let target = CombatTarget::new(profile(), Pool::full(60), placement, ActiveEffects::EMPTY);
        assert_eq!(target.health().current(), 60);
        assert_eq!(target.placement(), placement);
        assert_eq!(target.resistance(Element::Lightning), Resistance(7));
        assert_eq!(target.profile().defense(), 4);
    }

    #[test]
    fn combat_profile_wire_round_trips() {
        let p = profile();
        assert_eq!(p.incoming_damage_reduction(), Percent::ZERO);
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""incoming_damage_reduction":0"#));
        assert!(json.contains(r#""weapon_mode":"single""#));
        assert_eq!(serde_json::from_str::<CombatProfile>(&json).unwrap(), p);
    }

    #[test]
    fn a_pre_gear_wire_form_parses_to_the_gearless_identities() {
        // A profile serialized before the gear fields existed still parses;
        // every new field lands at its gearless zero/identity.
        let mut value = serde_json::to_value(profile()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("wizardry_rise_x2");
        object.remove("incoming_dd_pct");
        object.remove("wing_damage_pct");
        object.remove("wing_absorb_pct");
        object.remove("weapon_mode");
        let parsed: CombatProfile = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, profile());
        assert_eq!(parsed.wizardry_rise_x2(), 0);
        assert_eq!(parsed.incoming_dd_pct(), Percent::ZERO);
        assert_eq!(parsed.wing_damage_pct(), Percent::ZERO);
        assert_eq!(parsed.wing_absorb_pct(), Percent::ZERO);
        assert_eq!(parsed.weapon_mode(), WeaponMode::Single);
    }

    #[test]
    fn target_kind_wire_round_trips_and_is_carried_by_the_profile() {
        for kind in [TargetKind::Player, TargetKind::Npc] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(serde_json::from_str::<TargetKind>(&json).unwrap(), kind);
        }
        assert_eq!(profile().kind(), TargetKind::Npc);
        let json = serde_json::to_string(&profile()).unwrap();
        assert!(json.contains(r#""kind":"npc""#));
    }
}
