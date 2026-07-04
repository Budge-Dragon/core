---
name: weffect-state-machine-design
description: W-EFFECT (timed buffs/ailments/heal) state-machine type + transition design decisions — reuse in code-mode and by W-INV/W-PARTY
metadata:
  type: project
---

Load-bearing state-machine design decisions for **W-EFFECT** (ActiveEffect store, apply/advance/effective_profile/mobility). Grounded in the codebase's own idioms; verified against the architecture + canon plan verdicts and [[weffect-canon-pins]] (canon-guardian).

**Why:** These shapes resolve the plan's open questions concretely; they are non-obvious (the slot/enum duality, the Ice mutual-exclusion, the CombatBonus-consumer tension) and must survive into code unchanged.

**How to apply (each is a firm decision, not a suggestion):**

- **ActiveEffects = total structure with named OPTIONAL slots per SubType-group, NOT a Vec.** This is the ClassSet/DamageModifiers/PerElement idiom generalized to heterogeneous optional payloads. Six slots: `defense: Option<Tick>`, `defense_reduction: Option<Tick>`, `greater_damage: Option<TimedBonus>`, `greater_defense: Option<TimedBonus>`, `ice: Option<IceStatus>`, `poison: Option<PoisonDot>`. Duplicate-per-identity is unrepresentable BY CONSTRUCTION (one slot each). `Option` here is GENUINE present/absent optionality (an effect is genuinely on the entity or not), NOT a flow flag — effects are INDEPENDENT and CO-OCCUR (poison+iced+defReduction together), unlike the banned mutually-exclusive optional-soup. Wire form: `try_from="Vec<ActiveEffect>"` / `into`, serializes as a JSON array, duplicate identity (or Iced+Frozen both present) → parse error. `ActiveEffects::EMPTY` const = no effects (the seed value AND the clear-on-death value).

- **Iced↔Frozen share the `ice` slot (SubType 254) → mutual exclusion for free.** `IceStatus { Iced { expires_at } | Frozen { expires_at } }`. Applying one overwrites the other (canon SubType-replace). This is why slots group by SubType, not raw identity.

- **`ActiveEffect` (component enum, kind-tagged, 7 flat variants) is the WIRE element + iteration element + `EffectApplied` payload.** Each variant carries its RESOLVED integer magnitude + absolute expiry Tick (poison: per_tick_damage + remaining + next_tick). It is COMPONENT-LEVEL — it does NOT wrap `data::effects::Buff/Ailment` (that would be the first component→data upward leak). The `data::Buff/Ailment → ActiveEffect` map lives in `services/effects::apply_*`. The container reconstructs `Vec<ActiveEffect>` from slots (ClassSet↔Vec<CharacterClass> precedent).

- **apply/advance are RNG-free, no TickDuration on advance.** `apply_buff`/`apply_ailment` resolve ms→absolute Ticks (via `DurationMs::in_ticks` + `now`) and per-tick damage AT APPLY; `advance_effects(effects, health: Pool, now: Tick) -> (ActiveEffects, Pool, Vec<EffectEvent>)` only compares `now` to stored Ticks. Poison catch-up bounded by the remaining COUNTER (7), never the now-gap. Poison self-terminates on the 7th tick at Tick(420) — governed by the counter, NOT a 400 expiry gate.

- **Poison per-tick = 3% of TARGET max health (`scale_ratio(max_hp,3,nonzero(100))`), baked at apply, `.max(1)` floor.** apply_ailment reads target max-HP, not caster stats. 7×3% = 21% total → poison is NON-LETHAL from full; the e2e must pre-damage below 21%.

- **StopByDeath: on a lethal poison tick, advance returns `ActiveEffects::EMPTY` + zeroed Pool + a final `PoisonKilled { damage }` event (a distinct variant, NOT a bool flag on PoisonTick).** Clearing = assign `ActiveEffects::EMPTY` — the shared clear used by advance (poison death) AND the combat/kill orchestrator on `AttackOutcome::Killed` (no logic duplication; resolve_attack stays effect-free).

- **Unified `route(&Skill) -> SkillRouting { Damaging | Buff | Heal | Deferred }` SUBSUMES the existing `classify`** (no second overlapping Option-classifier). `ApplicableBuff { Defense | GreaterDamage | GreaterDefense }` is the proven subset minted at the boundary (DamagingSkill precedent), from `BuffSelf`/`BuffPlayer` shapes when the buff is in-scope; `InfiniteArrow`/`Alcohol`/party/`Summon`/`Teleport`/`NovaCharge`/`RecallParty` → Deferred via explicit or-patterns. Buff sub-match over `Buff` is exhaustive.

- **BuffPlayer IS in-scope (self OR supplied ally) — GreaterDamage/GreaterDefense are BuffPlayer shapes in our data.** `cast_buff` separates CASTER (Energy from Stats + pays cost from Vitals) from RECEIVER (`receiver_effects: ActiveEffects` supplied by host — self or ally). BuffPartyMember/BuffParty stay Deferred (need W-PARTY enumeration). Energy = base stat Energy this wave (gear TotalEnergy folds in W-INV).

- **Buff/Heal cast outcome = `BuffCastOutcome { Rejected { reason: CastRejection } | Applied { effect: ActiveEffect } | Healed { restored: u32 } }` — REUSES CastRejection** (one rejection vocab, no near-duplicate, no fabricated empty-hits Cast). Only InsufficientMana/InsufficientAbility are producible on this path.

- **effective_profile(base, &effects) is the fold seam in `services/profile`; resolve_attack keeps its `&CombatProfile` signature.** It folds each effect's `effect_bonus(&ActiveEffect) -> Option<CombatBonus>` (the currency; CombatBonus's FIRST consumer) via `fold_profile_bonus`, THEN applies the DefenseReduction ×9/10 as a derivation step (NOT via additive DefensePct). `fold_profile_bonus` handles the 3 effect-emitted variants (GreaterDamage→PhysicalDamage raising BOTH span ends per DoD; GreaterDefense→Defense; DK Defense→IncomingDamagePct{50}); the remainder is ONE explicit or-pattern no-op (documented W-INV extension point — resolved profile has no field / not emitted by W-EFFECT). effective_profile is TRANSIENT per strike, NEVER persisted.

- **CombatProfile grows ONE `incoming_damage_reduction: Percent` field** (base = ZERO in every constructor). effective_profile folds IncomingDamagePct into it multiplicatively; resolve_attack applies it as the FINAL defender-side step (after the min-damage floor). No new resolve_attack param.

- **Movement stays effect-unaware: `mobility(&effects, base_speed: Fixed) -> Mobility { Free | Slowed { speed } | Immobilized }` supplied as a plain input.** Iced → Slowed (base ×1/2). Frozen → Immobilized (short-circuits `decide_monster_action`'s move branches — leash/chase/wander → Idle; attack unaffected = movement-only gate). `decide_monster_action` gains an 8th param `mobility: Mobility` (flag deep-module-guardian; justified distinct non-bundleable input). resolve_step/resolve_drift UNCHANGED (already take `speed: Fixed`).

- **`Pool::restored(self, amount) -> Pool` = `current.saturating_add(amount).min(max)`, max unchanged, const** — additive sibling of `reduced`. Heal's reported amount = `new.current() - old.current()`.

- **Both entities gain `active_effects: ActiveEffects`** (Character: widen RawCharacter + TryFrom/From + accessor; MonsterInstance: plain pub field), seeded `ActiveEffects::EMPTY`.

See [[weffect-canon-pins]] for the authentic durations/magnitudes and the poison/stacking canon rulings.
