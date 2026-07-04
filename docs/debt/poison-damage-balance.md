# Debt record: poison DoT is authentic but OP by modern standards (3% of target max HP)

- **ID:** EFF-POISON (design / balance — keep-now, modernize-later)
- **Status:** OPEN
- **Owner wave:** W-BALANCE (a future balance/modernization pass — not yet scheduled)
- **Created:** 2026-07-04, during the W-EFFECT plan phase.
- **Category:** balance (works as designed; flagged for a future modern re-tune)
- **Severity:** medium (an exploit, not a crash)
- **Location (once W-EFFECT lands):** `core/src/services/effects.rs` — the poison
  per-tick damage constant and the poison apply/refresh rules. The apply path
  bakes `per_tick_damage = max(1, scale_ratio(target_max_health, 3, 100))`.

## Symptom

Poison damage-over-time is resolved as **3% of the target's MAXIMUM health per
tick**, 7 ticks per application ≈ **21% of the target's max HP per application**.
Because the magnitude is a fraction of the *victim's own* max HP, poison:

1. **ignores the attacker entirely** — a level-10 character and a level-400
   character inflict the identical 21%-of-max-HP; there is no attacker-power
   scaling; and
2. **ignores the target's defense** — defense reduces normal hits but does
   nothing against a %-of-max-HP tick.

Combined with **refresh-on-reapply** (re-casting resets the 7-tick stream) and
**no cap on how many times poison may be re-applied over time**, a weak attacker
can whittle down an arbitrarily strong monster purely by kiting and
re-poisoning indefinitely. Concretely: a low-level wizard can solo a very strong
monster it could never out-damage normally, just by keeping poison on it long
enough. That bypasses the power-gating that is supposed to stop weak players from
killing strong monsters.

## Why it is kept as-is for now

This IS the authentic classic MU poison model (poison damage = a percentage of
the victim's max HP), and W-EFFECT ships it faithfully. The user decided
(2026-07-04): **keep it authentic for this wave; do not adjust now — revisit and
possibly modernize later.** So the shipped behavior is intentional and correct
for the pre-S3 target; this record exists only so the balance concern is not
lost.

## Root cause (the modern-balance gap)

The authentic percent-of-victim-max-HP model has neither attacker scaling nor a
total-damage ceiling, so its power is unbounded relative to the attacker. That
is the exploit surface a modernization pass would close.

## Resolution plan (future W-BALANCE / modernization pass)

Revisit the poison model and pick a modern balance shape, e.g. one or more of:
- **Attacker-scaled** damage (scale by the caster's wizardry / level, not just
  the victim's max HP) so a weak attacker's poison is weak;
- a **total-damage / duration ceiling** per poison "session" (cap cumulative
  poison damage, or cap re-application stacking / add diminishing returns);
- a **level-gate** (poison effectiveness falls off when the caster is far below
  the target's level);
- a **flat + small-percent hybrid** (a modest floor plus a capped percent).
Then adjust the poison magnitude/refresh rules in `services/effects.rs` and
re-run `cargo test -p mu-core`.

## Not to be confused with

The separate **provenance** flag on the same number: the poison basis
(`PoisonDamageMultiplier` 0.03 and *what* it multiplies) is under-specified in
the reference and is ruled "3% of target max HP" pending an authentic-source
byte-check — that is tracked as a W-SRC concern in
[combat-constants-provenance.md](combat-constants-provenance.md) (is 3% the
*authentic* value?). **This** record is the orthogonal *balance* concern (even if
3% is authentic, it is OP by modern standards).

## Discharge

Discharged when the future balance pass either adopts a modernized poison model
(and updates `services/effects.rs`) or explicitly decides the authentic model is
acceptable and records that decision here. Remove EFF-POISON from `DEBT-INDEX.md`
at that point.
