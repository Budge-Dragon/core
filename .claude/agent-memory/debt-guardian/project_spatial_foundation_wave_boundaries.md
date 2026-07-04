---
name: project-spatial-foundation-wave-boundaries
description: What the spatial-foundation work (Waves A + B) legitimately defers to W-MOV / W-ENT vs. what is real debt — so future reviews treat the deferrals as planned, and the tracked follow-ups as tracked, not as fresh violations.
metadata:
  type: project
---

The spatial-foundation work (branch `spatial-foundation`, commits `61f1e37`
Wave A + `5ed218e` Wave B) replaced the classic 8-way tile grid with a
fixed-point `Q40.24` 2.5D ground plane and loaded per-map terrain walk grids
into the `Atlas`. It ships the pure spatial value types only; every *decision*
that consumes them is deferred to a named wave. All deferrals are now formalized
in `docs/debt/` and indexed in `docs/debt/DEBT-INDEX.md`.

**Wave IDs in play:** R1–R4 (v2 data rebuild) done; **W-ENT** = entities wave;
**W-CMB** = combat wave; **W-SRC** = source-verification pass; **W-MOV** =
movement / flight wave (introduced 2026-07-03 for the movement-behaviour
surface). See also [[project-v2-rebuild-wave-boundaries]].

**Why:** the spatial types were built up to but not into movement behaviour,
holding Iron Law 4 (no consumer-less surface) deliberately — the same reason the
`Fixed::mul`/`div` narrowing surface was implemented then removed under Wave A
guardian review.

**How to apply — these are formalized deferrals, do NOT re-flag as fresh debt:**
- **No movement service** and no `apply_flight_change` / `FlightChange` /
  `MovementModeChanged` → **D1**, owner W-MOV, blocked by W-ENT (eligibility gate
  needs character entity state; `core/src/entities/mod.rs` is an empty
  placeholder).
- **`Fixed` has no `mul` / `div` / `NonZeroFixed` / round-saturate** despite
  spec §2.1 → **D2**, owner W-MOV (ships with the normalize-to-speed consumer).
- **`WalkGrid` loaded but only tests call `Atlas::walk_grid` /
  `WalkGrid::walkable`** → **D3**, owner W-MOV (movement service consumes it;
  `Flying` skips the check via `Movement::checks_walkability`).
- **`SpawnPlacement` / `SoccerPitch` still hold `TileCoord`/`TileArea`/
  `TileFacing`** → **D4**, owner W-ENT (spawn/soccer service resolves to world
  space via `to_world` at point of use; `Area` needs walk grid + RNG). Tiles are
  the correct *authoring* layer — keeping them on disk is not a leak.
- **`Landing.facing: Option<Facing>`** is correctly modelled (absence is a
  variant, never a fabricated default). **D5** is a forward-looking policy note:
  a W-MOV warp/movement service must choose arrival facing for a `None` landing
  as explicit service policy, never `unwrap_or`/default in core.
- **Tech debt (works, lint-clean, guardian-flagged):** T1 `narrow_u8` dead
  saturation arm (checked `try_from`, NOT a banned suppressor — dead defensive
  code; reshape so in-range is proven by type); T2 `TileArea::contains`
  test-only; T3 `WorldVec::length_sq` unconsumed; T4 `walk_grid` returns
  `Option` though parse proves every map has a grid (optional future: a
  proven-present `MapId` handle). All in `docs/debt/spatial-foundation-followups.md`.
- **Quality improvements Q1–Q4** (drift-pin tests, expanded proptests, `syn` ban
  scanner + pre-commit hook, CI OS matrix + wasm test-run) are practices-transfer
  audit findings, **not debt from shipped code** — recorded in
  `docs/debt/practices-transfer-quality.md`, unblocked, schedulable anytime.

**Surface these the moment a blocker clears:** when W-ENT lands the character
entity, D1's gate and D4's spawn service become buildable; when W-MOV starts,
D2/D3/D5 and T2/T3 are in scope. Check `docs/debt/DEBT-INDEX.md` at the start of
any movement/entity/tile work and close the rows the wave discharges.

**Closure state (as of 2026-07-03) — most of the above is now RESOLVED; do not
re-open:**
- **W-ENT (committed, branch `entities`)** closed **D4** (spawn/soccer world-space
  resolution — `services/spawn.rs`) and **T4** (Atlas-minted `MapHandle` made the
  resolved walk-grid path total). Records: `worldspace-resolution-services.md`,
  `spatial-foundation-followups.md`.
- **W-MOV (branch `movement`, uncommitted at review time)** closed **D1** (flight
  FSM `apply_flight_change`/`flight_gate`/`change_flight` in `services/movement.rs`
  — eligibility takes host-supplied `Wings`/`CombatLock` facts, so the W-ENT block
  dissolved without reaching into a character entity), **D2** (`Fixed`
  `mul`/`div`/`NonZeroFixed`/`isqrt` consumed by `WorldVec::normalized_to`),
  **D3** (`commit` validates grounded steps against `WalkGrid`), **D5**
  (`resolve_arrival` matches `Landing.facing` `Some`/`None`), and **T3**
  (`length_sq` consumed by `normalized_to` + `seek`). 209 tests pass.
- **Fully CLOSED as of 2026-07-04:** all of **T1–T4** are discharged (T1 by
  W-HARDEN's `tile_index` reshape; T2 by trim; T3/T4 by W-MOV/W-ENT), so
  `spatial-foundation-followups.md` is CLOSED (its header was flipped 2026-07-04 —
  it had been left OPEN despite every sub-item closing). **Q1–Q4** are CLOSED
  (`practices-transfer-quality.md`, W-HARDEN). **MOB-SPD** CLOSED (W-SRC,
  confirm-and-keep). So the DEBT-INDEX no longer carries any spatial/T/Q/MOB row
  — do NOT expect to find them there. **Still OPEN in the index:** only **W-SRC**
  and **CMB-CONST** (both W-SRC provenance), plus the new **I1–I3** (W-INV) — see
  [[project-winv-inventory-wave-boundaries]].
- **New W-MOV finding: `MOB-SPD`** — `MOB_STEP_SPEED` (1 tile/step, invented, not
  sourced) in `services/monster_ai.rs`. Provenance debt of the W-SRC species but
  in-code, so kept as its own record `mob-step-speed-provenance.md` (owner W-SRC),
  NOT folded into `openmu-default-values.md`. The 7-param `decide_monster_action`
  signature is **NOT debt** (each param a distinct non-bundleable domain input;
  the ms→Ticks "future reduction" was a scratchpad-brief note only, never a code
  comment — no dangling forward-reference exists).
