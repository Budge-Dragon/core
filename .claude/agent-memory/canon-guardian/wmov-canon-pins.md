---
name: wmov-canon-pins
description: Canon rulings pinned in the W-MOV (movement/flight) plan review — reuse in the code-mode review of the same wave
metadata:
  type: project
---

Plan-mode canon rulings for **W-MOV** (movement & flight), to re-apply when the code-mode review runs. These correct or tighten what the spec/WORKPLAN pinned.

**Why:** Several are non-obvious and one contradicts the WORKPLAN's own DoD; substantiated by a compiled reproduction, not asserted.

**How to apply (verify each in the implementation):**
- **normalize-to-speed tolerance is LINEAR, never squared.** The DoD's "within one sub-unit² of speed²" is arithmetically impossible (measured squared error ~9.6e8 sub-unit² at 10-tile speed). The dominant error is the **isqrt FLOOR** of the magnitude M: `|‖result‖ − speed| ≈ speed·(L−M)/M ≤ ⌈speed/‖v‖⌉ + 1`. Tight (≤~1) only when ‖v‖ ≥ speed. Fusing the div+mul into one `round_div(component·speed, M)` barely helps (731 vs 733) because floor(sqrt) dominates — recommend it anyway (one rounding, no intermediate NonZeroFixed unit).
- **The greedy step MUST clamp to remaining distance (Reynolds arrival), not just world bounds.** Otherwise a mob within one step overshoots — exactly the large-error regime. `WorldPos + WorldVec` clamps to WORLD_EXTENT only; that is not arrival-clamping.
- **round_shift/round_div ties-away-from-zero must use the magnitude/sign form.** Naive `(p+half)>>shift` rounds toward +∞ (gives −1 for −1.5, not −2) — NOT ties-away. Do `sign(p)·((|p|+half)>>shift)` and `sign(n/d)·((|n|+|d|/2)/|d|)`.
- **Leash has no anchor to tether to.** `MonsterInstance` stores only number/placement/health — no spawn-origin anchor. Canonical return-to-anchor leash is a half-implementation without stored anchor. Needs an additive `anchor: WorldPos` on the instance (spawn origin), like the additive `Placement.map`.
- **Cadence gate has no stored next-ready tick.** Rate-limiter/cooldown (Nystrom Update Method) needs persistent `next_action: Tick`; entity has none. `Tick` newtype does not exist yet (grep-confirmed) — W-MOV adds it. `Tick` (time-point) and `Ticks` (duration) are distinct types.
- **Empty landing area is a reachable outcome.** Atlas proves referential integrity but NOT that a `Landing.area` contains a walkable tile. Sampling an empty set is undefined → needs an explicit warp-arrival outcome variant, never a fabricated position.
- **`in_ticks` rounds UP (ceil).** A cooldown must never fire faster than authored.
- **tiles→Radius must be TOTAL** (u8·2^16 ≤ RADIUS_MAX = 2^25 always) — a total constructor, not `Radius::new(..).unwrap()` (unreachable-Err/unwrap banned).
- **isqrt = std `u128::isqrt()`** (stable ≥1.84; toolchain is 1.96). Reject any hand-rolled Newton/bit loop. Home it on `DistanceSq` (the squared type) returning u64 linear sub-units via lossless try_from.

**Code-phase discharge (verdict PASS).** The implementation matched every pin. Two rulings worth keeping so a future review doesn't re-litigate:
- The §2.1 inherent `fn mul/div` shipped as `impl Mul<Fixed>` / `impl Div<NonZeroFixed>` (clippy `should_implement_trait` forbids inherent `mul`/`div`). This is the canonically-PREFERRED form (Rust API Guidelines C-OPS: implement `std::ops` for numeric types); semantics identical to the pinned narrowing contract. Not confusable with the integer `scale(k: i64)` — different arg type, no shift/round. FAITHFUL.
- The `<< TILE_SHIFT` in fixed-point divide is extracted to a free `fixed_div` fn to satisfy clippy `suspicious_arithmetic_impl` (a `Div` body may contain only `/`). Mechanical; the canonical Q-format divide (scale numerator, round_div ties-away) is intact.
- Minor (deferred to debt/rules-guardian, not canon): `draw_cardinal` iterates `CARDINALS` with a trailing dead `Facing::POS_X` return (no-indexing rule forces iterate-and-match over a fixed 8-set); the uniform selection itself is canonical.

See [[review-standard]] and [[mu-core-context]].
