---
name: project-wcmb-combat-wave-boundaries
description: What the W-CMB combat wave legitimately defers to W-SRC (constant provenance) vs. the two root-fix-now issues its code-review found, so future reviews don't re-litigate the tracked deferral.
metadata:
  type: project
---

The combat wave **W-CMB** (branch `combat`; attack resolution, per-class stat
derivation, skill cast, kill-reward loop) reached code review clean on canon and
deep-module, `CHANGES_REQUIRED` on architecture. See
[[project-v2-rebuild-wave-boundaries]] and [[project-spatial-foundation-wave-boundaries]].

**Formalized deferral (do NOT re-flag as fresh debt):**
- **`CMB-CONST`** (owner W-SRC) — 13 hardcoded/invented combat constants (3% hit
  floor, `·3/10` overrate, `max(1,L/10)` floor, `·5/4` exp factor,
  `BASE_MONEY_DROP=7`, `DROP_LEVEL_WINDOW_GAP=11`, over-level gap 10, high-level
  victim 65, `DASH_SPEED=8`, `SINGLE_TARGET_RADIUS_TILES=1`, `ONE_TILE_SPEED`).
  All carry `// W-SRC:` comments and are correctly-typed consts. Same species as
  `MOB-SPD`; kept out of `openmu-default-values.md` to keep its per-file
  `review`-string count exact. Record `docs/debt/combat-constants-provenance.md`,
  DEBT-INDEX row `CMB-CONST`.

**Two root-fix-now issues found in review (NOT debt — must be fixed before the
wave ships, one-line each; debt-guardian is review-only so the implementer
applies them):**
- `services/skills.rs` `bounding_rect([WorldPos;4])` seeds its fold via
  `.into_iter().next()` with a `None => WorldPos::clamped(0,0)` arm — a fabricated
  zero on a branch the length-4 array proves dead (Iron Law 3). Fix: destructure
  `let [seed, ..] = corners;`. Also flagged by architecture-guardian.
- `components/combat_profile.rs` `CombatProfile::resistances()` (plural) has zero
  call sites — dead `pub` accessor; only singular `resistance(element)` is used.
  Trim it (expose the minimum).

**Judged clean (not debt, not hidden patches):** the 6 reported spec deviations —
resolved `victim_level` param + `resolve_kill` folding `atlas.monster()` once;
the no-combat-block `None`→no-reward `KillResolution`; `ItemLevel::clamped`
compute-path constructor; self-contained `combat_simulation.rs` loader; the three
`try_from(..).unwrap_or(TYPE::MAX)` saturating narrows (boundary conversions, not
lookup-shaped). Also clean: `VitalMaxima` is unconsumed in-core (discarded at
`skills.rs:255 .0` and `combat_simulation.rs:270 _`) but is the complete
class-formula capacity output a host applies — not premature generalization.
