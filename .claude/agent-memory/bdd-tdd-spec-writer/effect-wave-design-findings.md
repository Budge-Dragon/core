---
name: effect-wave-design-findings
description: W-EFFECT (timed buffs/ailments/heal) spec-design findings — the profile-fold integration-seam asymmetry, poison-basis + poison-tick-count port gaps, DefensePct-as-reduction encoding wrinkle, effects draw no RNG, tick-count durations at 50ms
metadata:
  type: project
---

Design findings from writing the W-EFFECT BDD/TDD spec (timed effects: buff application,
ailment lifetimes, heal, folded into combat & movement). Cross-cutting decisions future
waves need; not derivable from code alone. Sibling of [[combat-wave-design-findings]] and
[[movement-sim-design-findings]].

**The integration seam is ASYMMETRIC — the crux the plan-guardians must rule.** GreaterDamage
(+physical), GreaterDefense (+defense), DefenseReduction (×9/10 defense) fold cleanly into
EXISTING `CombatProfile` fields (`physical` Interval, `defense` u16) via `CombatBonus`
contributions — so `effective_profile(base, &ActiveEffects) -> CombatProfile` works for them
and gives `components::bonus::CombatBonus` its first consumer. BUT the DK Defense skill's
"damage-received ×1/2" (and deferred SoulBarrier) is a *defender incoming-damage multiplier*
that `resolve_attack` DOES NOT apply today — `CombatProfile` has no incoming-damage field and
combat.rs never reads `CombatBonus::IncomingDamagePct`. So the Defense buff CANNOT be expressed
as a profile fold without either (a) extending `CombatProfile` + `resolve_attack` with an
incoming-damage-reduction step, or (b) threading a separate incoming modifier into
`resolve_attack`. Recommend (a) reusing the existing `IncomingDamagePct` vocabulary. Spec the
Defense-buff scenarios purely on the observable (buffed target takes half damage), never on the
encoding.

**`CombatBonus::DefensePct` can't express ×9/10.** `Percent` is 0..=100 ADDITIVE ("increased
by"), so a ×0.9 reduction (DefenseReduction) and a ×1/2 damage-received (Defense buff) have no
clean additive-percent representation. Flag: either a reduction-percent reading, or resolve the
factor as an already-scaled integer contribution at apply time. Numbers via `scale_ratio`
(defense 20 → ×9/10 → 18).

**Poison has TWO under-specified port gaps, both flagged, neither to be calcified in a bullet.**
(1) DAMAGE BASIS: PoisonDamageMultiplier 0.03 × WHAT (max health? caster wizardry?) is not in
the reference. Spec poison on cadence + count + monotonic decrease + constant per-tick amount;
leave the exact per-tick magnitude as an acceptance criterion contingent on the ruled basis.
(2) TICK-COUNT SOURCE: `data::effects::Ailment::Poisoned` is a SINGLE variant, but Poison (7
ticks / 20s) vs Decay (3 ticks / 10s) differ. The combat path only carries the bare
`Ailment` (`TargetHit.inflicted: Option<Ailment>`), so the 7-vs-3 count/duration must come from
the inflicting skill at apply time — the bare Ailment is too coarse. Apply must receive a poison
descriptor, not re-derive from the enum. (Decay may itself be S3+ Summoner — confirm it's in
pre-S3 scope at all.)

**Effects draw NO RNG — determinism is tick-driven, not seed-driven.** apply (magnitude from
caster stats + consts), advance (expiry = tick compare; poison damage fixed per tick), and the
buff/heal cast path (no hit roll) are all deterministic pure functions of (state, now[, tick]).
Recommend apply/advance take NO `&mut impl RngCore`. If a signature includes one for uniformity
it must draw ZERO words (assert via the next-word-agrees probe, the roll_apply_elemental-immune
precedent). The W-EFFECT determinism criterion is "same (state, now) → same output," not a seed
replay.

**Durations in ticks at the suite's 50ms/tick base:** 3s=60 (poison cadence), 4s=80 (Defense
buff), 5s=100 (Frozen), 10s=200 (Iced/DefenseReduction/Decay), 20s=400 (Poison), 60s=1200
(GreaterDamage/GreaterDefense). Expiry is INCLUSIVE per `Tick::reached` (now>=expiry ⇒ expired):
apply at t0 dur D ⇒ active at t0+D-1, gone at t0+D. First poison tick fires 3s AFTER apply
(t0+60), not at apply. Magnitudes (integer floor divide): GreaterDamage 3+E/7, GreaterDefense
2+E/8, Heal 5+E/5 (E=70 → 13/10/19; E=0 → 3/2/5, all still positive so no guard needed).

**Movement seam:** Iced ×1/2 folds into the `speed: Fixed` that `resolve_step`/`resolve_drift`
already accept (half-tile step is sub-tile-representable in Fixed). Frozen "cannot move" is a
GATE before the step. Neither monster_ai nor the movement service currently takes active effects,
and neither entity (`MonsterInstance`/`Character`) stores effects yet (greenfield). Recommend the
effects layer expose a movement disposition (Free / Slowed(factor) / Immobilized) the mover
consults, keeping the movement service effect-unaware and RNG-honest (no hidden state). Flag as a
signature question.

**Replace-don't-stack is keyed by effect IDENTITY.** "same SubType doesn't stack" ⇒ ActiveEffects
should be a total structure holding AT MOST ONE effect per identity (duplicate-as-unrepresentable),
so re-apply refreshes expiry/magnitude rather than appending. Secondary open point: elemental
SubType = 255 - elementType means Iced and Freeze (both Ice) could SHARE a SubType and replace
each other — confirm whether that cross-effect replacement is in scope.

**Clear-on-death (StopByDeath=true for every in-scope effect) has a two-owner seam:** a poison
tick that lethals inside `advance_effects` must stop further ticks AND clear remaining effects;
a combat/kill death elsewhere must also clear. Flag who owns each (advance vs kill/combat).
