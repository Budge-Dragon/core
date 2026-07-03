---
name: combat-wave-design-findings
description: W-CMB (combat) spec-design findings — corrected scope (player fight-math + lightning shove IN), zen=exp+7 coupling, per-class integer-rounding scheme, AttackOutcome axis, drop-window totality, atlas::progression gap
metadata:
  type: project
---

Design findings from writing the W-CMB (attack resolution + skill damage + kill-reward
loop) BDD/TDD spec. Cross-cutting decisions future waves need; not derivable from code alone.

**SCOPE CORRECTION (2026-07-03): player fight-math and lightning shove are IN — my earlier
recommendation to defer both was overruled by the user.** W-CMB now builds the base per-class
stat->combat derivation for all 8 classes (5 formula families) from the gearless Character,
AND composes the lightning 1-tile displacement via movement::resolve_step/resolve_drift.
Equipment/CombatBonus aggregation and level-up vitals recompute stay deferred (need
Inventory/Equipment). Both monster-side (from MonsterCombat) and player-side (from Character)
CombatProfiles are derived in-wave.

**The five formula families and their sourced values (docs/reference/openmu-facts/0…md).**
8 CharacterClass variants fold into 5 families sharing a line's formula: {DarkWizard,SoulMaster},
{DarkKnight,BladeKnight}, {FairyElf,MuseElf}, {MagicGladiator}, {DarkLord}. Derivation is an
EXHAUSTIVE per-class match. Shared: DefenseFinal = DefenseBase/2 (combat DEFENSE); pre-S3
AttackRatePvm/DefenseRatePvm are authoritative (classic PvP). Worked DK example that pins the
spec (Level 50, Str 60, Agi 40, Vit 50, Ene 30): attack_rate 325, min_phys 10, max_phys 15,
defense 6, defense_rate 13, max_health 285, max_mana 65, max_ability 62, all chance stats 0.
Gearless crit/excellent/defense-ignore/double chances are AUTHENTIC ZERO (they come only from
excellent item options) — a total derivation produces zero, never a fabricated `Default`.

**Per-class rounding scheme is load-bearing and undecided — canon-guardian must rule it.** The
worked example numbers assume: multiply-before-divide (1.5*Agi = (3*Agi)/2, NOT 3*(Agi/2)),
and per-term truncation (each relationship contribution truncates independently, matching
OpenMU's separate AttributeRelationship contributions). Nested pure divisions collapse safely
(floor(floor(a/3)/2) == floor(a/6)), but coefficient terms diverge under a different order. DL
MaxHealth is the one documented pooling case: 48.5+1.5*Level+2*Vit = (97 + 3*Level + 4*Vit)/2
to hold the half-point. Same widen->mul->div discipline everywhere else.

**Zen drop amount couples loot to experience: money = awarded_experience + 7 (BaseMoneyDrop=7),
MoneyAmountRate 1.0 pre-S3 (docs/reference/openmu-facts/5…md L39).** The proposed
resolve_kill_drops(killer,victim,map,&Atlas,&mut rng) signature CANNOT compute the authentic
zen amount because it never receives the exp. Upstream reframe: the KillResolution orchestrator
computes award_kill_experience FIRST, then threads the awarded exp into drop resolution so the
money Drop carries amount = exp+7 — or the money amount is filled at the bundle level. Flag this
as a port-shape question, not a workaround.

**Item drop window: eligible pool = drop_level in (monster_level-12, monster_level], i.e.
inclusive [monster_level-11, monster_level]; DropLevelMaxGap=12; floor saturates at 0.** The
strict `>` vs `>=` off-by-one (min = mlvl-11 not mlvl-12) must be pinned. Dropped item plus-level
= min((monster_level - item.drop_level)/3, item.max_item_level). Empty eligible window ->
Drop::Nothing (WeightedTable::new rejects empty, so the empty case is guarded BEFORE the table,
never a panic — the loot totality edge). Excellent needs monster_level >= 25 (ExcellentItemDropLevelDelta),
pool at monster_level-25.

**AttackOutcome axis collision unchanged — state-machine-agent owns it.** Naive {Missed|Hit|
Critical|Excellent|Killed} mixes lethality (killed?) with quality (normal/crit/excellent) with
modifiers (defense-ignored/doubled). Write scenarios on OBSERVABLES (damage magnitude, kill-vs-
not, reported quality, reported modifiers) never on the variant encoding. resolve_attack reports
lethality but NOT drops — drops are a separate service (loot), so Killed does not carry a drops
Vec here (differs from the CLAUDE.md illustrative example).

**Exp killerLevel dampening MUST be exact integer ratio (widen->multiply->divide).** base *=
(targetLevel+10)/killerLevel — divide-first collapses to 0 (e.g. killer 50 vs target 30: 40/50=0).
Correct: base*(target+10)/killer. Worked: target 30 killer 50 -> base 550 -> 550*40/50 = 440 ->
*5/4 = 550 -> jitter. Same discipline for *1.25 (=*5/4) and [80,120]% jitter (*pts/100). A key
mutation teeth-check: divide-first bug zeroes the award.

**Elemental apply-vs-resist curve mismatch — canon-guardian to rule.** chance::roll_resistance
is uniform_below(255) < byte -> "resisted" with prob byte/255. Authentic facts say elemental
effect APPLIES with prob 1/(resistance+1); resistance>=255 immune. Different mid-range curves.
Boundaries hold under BOTH (R=0 always applies, R=255 never), so spec those two deterministically
and write mid-range abstractly ("applies iff not resisted"). Flag: reuse roll_resistance as a
documented approximation OR add an apply-with-1/(r+1) primitive.

**Lightning shove seam: skills service composes movement, combat stays pure.** On a landed
lightning-element application (target !resist), draw a cardinal + resolve_drift the target one
tile against its WalkGrid; report the new placement in the TargetHit. The "can't attack while
shoved" behavior is EMERGENT (target left attack range -> monster_ai re-chases before attacking)
— NO stun/attack-lock timer (unsourced). resolve_attack (combat) never touches WalkGrid/movement.

**Atlas discards game_config after extracting .drops — progression() accessor must be added.**
atlas/mod.rs L151 does `let drop_config = game_config.drops;` and drops the rest. W-CMB must
retain game_config.progression (max_party_size, exp_jitter_percent RatePercentRange=[80,120]) and
expose Atlas::progression(). New chance primitive uniform_in_inclusive(Interval<u16>,rng)->u16
for the damage span and the exp jitter.

**Pool damage-application path returns in W-CMB.** components::pool has NO mutation method (a
compute-path `clamped` was removed in W-ENT, noted to return with its consumer here). W-CMB adds
a total reduce (Pool::damaged(u32)->Pool saturating current at 0, max unchanged). Live health
stays on the ENTITY (MonsterInstance.health / Vitals.health), passed/returned alongside the
resolved-stats CombatProfile — never bundled into CombatProfile (that mixes derived-static with
live state).

**Hardcoded consts (3% hit floor, 3/10 overrate, level/10 floor, 5/4 exp factor, BaseMoneyDrop=7,
DropLevelMaxGap=12, ExcellentItemDropLevelDelta=25) are OpenMU hardcoded logic, NOT in
game_config.json.** Module-level consts with provenance comments; flag debt-guardian for a W-SRC
provenance row mirroring docs/debt/mob-step-speed-provenance.md.

**e2e precedent to mirror: core/tests/movement_simulation.rs + core/tests/common/mod.rs.** Shared
harness exposes real_atlas() (whole checked-in dataset via Atlas::parse), the SplitMix64 TestRng,
and or_abort (lint-clean load-failure path — no unwrap/panic outside #[test] bodies in shared
harness code). W-CMB's e2e drives a real character_profile-vs-real-monster kill loop over the
Atlas plus a mutation teeth-check (inject a plausible bug, confirm a specific invariant reddens,
revert).
