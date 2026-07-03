# Debt record: movement / flight wave (W-MOV) deferrals

- **ID:** W-MOV (covers D1, D2, D3, D5)
- **Status:** CLOSED (2026-07-03, W-MOV) — all four deferrals discharged; see
  the per-item closure notes and the [Discharge](#discharge) section. D1, D2,
  D3, D5 removed from `DEBT-INDEX.md`.
- **Owner wave:** W-MOV (movement / flight wave)
- **Created:** 2026-07-03, during the spatial-foundation work (Waves A + B,
  commits `61f1e37` and `5ed218e` on branch `spatial-foundation`).
- **Scope:** the movement-behaviour surface that the spatial foundation was
  deliberately built *up to but not into*. Waves A + B ship the pure spatial
  value types (`Fixed`, `WorldPos`/`WorldVec`, `Radius`/`DistanceSq`, `Facing`,
  `ConeHalfWidth`, `WorldRect`, `Region`, `Movement`, `WalkGrid`) and load the
  per-map walk grids into the `Atlas`. Every **decision** that consumes those
  types — the flight state machine, the narrowing arithmetic normalize needs,
  the grounded-step walkability check, and the arrival-facing policy — is
  W-MOV's, and none of it appears in Wave A/B code. This is not missing work;
  it is Iron Law 4 (no consumer-less surface) held deliberately.

## Why this is deferred, not a violation

A `services/movement.rs` written now would have no caller: `core/src/entities/`
is an empty placeholder (`core/src/entities/mod.rs` carries doc comments only),
so nothing holds the character state a flight-eligibility gate reads, and no
combat or movement loop calls a step function. Shipping the service, the
`Fixed` narrowing ops, or a walk-grid check ahead of that consumer would be
exactly the consumer-less surface Iron Law 4 forbids — the same reason the
`Fixed::mul`/`div` surface was implemented and then removed during Wave A
guardian review. The spatial types are the floor these decisions stand on; the
decisions land with the entity state that gives them a caller.

## Deferred items

### D1 — Flight state machine + movement service (`services/movement.rs`)

- **CLOSED (2026-07-03, W-MOV).** `services/movement.rs` ships the flight FSM as
  the pure 2×2 transition `apply_flight_change` (lines 38-58) returning
  `Vec<FlightOutcome>` (`events/movement.rs`); redundant changes are idempotent
  no-ops emitting nothing. The eligibility gate `flight_gate` (lines 81-99) +
  `enable_off_sky` (lines 63-75) decides over `MapEnvironment`
  (`data/map_definitions.rs` — Sky forces flight) and the host-supplied
  `Wings` / `CombatLock` facts (`components/movement.rs`), capability before
  transient lock. `change_flight` (lines 105-116) is the gate-then-transition
  public service. **Blocked-by resolved:** the W-ENT dependency was discharged
  by modeling eligibility inputs as *injected* two-state domain facts (`Wings`,
  `CombatLock`) rather than reaching into a character entity — so the gate has a
  real consumer today and never leaks entity internals. Proof:
  `apply_flight_change_covers_all_four_tuples`, `flight_gate_truth_table`,
  `change_flight_denied_leaves_mode_and_reports_reason`.
- **Symptom:** there is no movement service. `Movement`
  (`core/src/components/movement.rs`) is a two-variant `Grounded | Flying`
  classifier whose only behaviour is `checks_walkability()`; no transition
  function, no eligibility gate, no step validation exists.
- **Root cause:** the transition consumes runtime character state (wings
  equipped, combat-lock) that lives on an entity, and `entities/` is an empty
  placeholder until W-ENT. A transition function with no state to transition
  and no caller is a consumer-less surface.
- **Resolution plan (W-MOV):** add `services/movement.rs` with
  `apply_flight_change((Movement, FlightChange) -> (Movement, Vec<MovementModeChanged>))`
  as a pure per-variant transition returning outcome events (never an effect).
  The eligibility gate (wings equipped / map allows flight via
  `MapEnvironment::Sky` on `map_definitions.rs` / not combat-locked) reads the
  W-ENT character entity. Grounded steps validate against the map's `WalkGrid`
  (see D3); `Flying` skips the check, keyed off `Movement::checks_walkability`.
  Consumes the already-built `WalkGrid`, `Movement`, `WorldPos`, `Facing` —
  no new spatial types.
- **Blocked-by:** **W-ENT** — the eligibility gate needs character entity state.

### D2 — Fixed-point narrowing surface

- **CLOSED (2026-07-03, W-MOV).** The §2.1 narrowing surface landed in
  `components/spatial.rs` **with its consumer**, so Iron Law 4 (no consumer-less
  surface) is discharged: `NonZeroFixed::new` rejects zero into
  `SpatialError::ZeroFixed` (lines 149-155, variant at line 77), making
  div-by-zero unrepresentable; `Fixed` `Mul` (lines 164-174) and
  `Div<NonZeroFixed>` (lines 176-185) round nearest with ties away from zero via
  the magnitude/sign helpers `round_shift` (line 861) / `round_div` (line 869)
  and saturate-narrow through `saturate_i64` (line 881) — no `as`, no `unwrap`,
  no `panic`; `DistanceSq::isqrt` (line 523) takes the integer floor via the
  cast-free `u64_from_u128_low` (line 905). **Consumer:** `WorldVec::normalized_to`
  (lines 330-338) folds the zero vector to `Displacement::NoDirection` and scales
  any other via `isqrt` + `NonZeroFixed` + `/` + `*`; the live chain is
  `normalized_to` ← `seek` (`services/movement.rs:147`) ←
  `resolve_step`/`resolve_drift` ← `decide_monster_action`. Proof:
  `mul_by_one_tile_is_identity`, `mul`/`div_rounds_ties_away_from_zero_both_signs`,
  `mul_saturates_on_overflow`, `non_zero_fixed_rejects_zero`,
  `isqrt_floors_perfect_and_non_square_and_zero`,
  `normalized_to_reaches_speed_and_is_scale_invariant` + the isqrt/normalize
  proptests.
- **Symptom:** `Fixed` (`core/src/components/spatial.rs`) exposes only
  `from_raw`, `raw`, `from_tile_parts`, `scale` (integer factor), and
  saturating `Add`/`Sub`. There is no `Fixed::mul`, no `Fixed::div`, no
  `NonZeroFixed`, and no round-nearest / saturate-narrow helper — although
  `docs/specs/2026-07-03-spatial-foundation.md` §2.1 specifies all of them.
- **Root cause:** no Wave A/B value needs fixed×fixed or fixed÷fixed; the
  distance/cone predicates are exact integer dot products that never narrow a
  product back to `Fixed`. The narrowing surface is needed only by
  normalize-to-fixed-speed, which is W-MOV's. It was implemented and then
  removed in Wave A under guardian review for exactly this reason.
- **Resolution plan (W-MOV):** land the §2.1 surface with its first consumer —
  `Fixed::mul` / `Fixed::div(NonZeroFixed)` (widen to `i128`, round-nearest
  ties-away, saturate-narrow with a checked/saturating conversion, no `as`,
  no `unwrap`), `NonZeroFixed::new` rejecting zero into `SpatialError` so
  div-by-zero is unrepresentable, and integer `isqrt` only if normalize-to-speed
  needs a magnitude. Never a bare `f64::sqrt`.
- **Blocked-by:** **W-MOV** — ships with the normalize-to-speed consumer; no
  earlier wave has a caller.

### D3 — Walk-grid consumer (grounded-step validation)

- **CLOSED (2026-07-03, W-MOV).** The walk grid now has a live *service*
  consumer: `commit` (`services/movement.rs:124-141`) blocks a grounded step onto
  a non-walkable destination —
  `if placement.movement.checks_walkability() && !grid.walkable(destination) {
  return StepOutcome::Blocked }` — while a `Flying` entity skips the check,
  keyed off `Movement::checks_walkability`. The public services `resolve_step`
  (line 162) and `resolve_drift` (line 178) route through `commit`, and
  `decide_monster_action` (`services/monster_ai.rs:98,115,127`) feeds them the
  grid. Cross-file proof against the **real Atlas walk grids**:
  `grounded_steps_respect_real_terrain_walls` (`core/tests/data_files.rs`), plus
  unit tests `grounded_step_blocks_on_non_walkable_destination`,
  `grounded_step_resolves_onto_walkable_destination`,
  `flying_step_crosses_a_non_walkable_cell`.
- **Symptom:** `WalkGrid` loads per map at `Atlas::parse`
  (`core/src/data/atlas.rs` `index_terrain`, proven bijective with the map set)
  and is queryable in world space via `Atlas::walk_grid` and
  `WalkGrid::walkable(WorldPos)`, but the only callers are tests
  (`core/tests/data_files.rs`). No service reads it.
- **Root cause:** the walk grid is a movement input; the movement service that
  would range a grounded step against it (D1) does not exist yet.
- **Resolution plan (W-MOV):** the movement service validates each grounded
  step against `Atlas::walk_grid(map)` → `WalkGrid::walkable(dest)`; a `Flying`
  entity skips it, gated on `Movement::checks_walkability`. Consumes the shape
  built in Wave A and loaded in Wave B unchanged.
- **Blocked-by:** **W-MOV** — the consuming movement service (D1).

### D5 — `Landing.facing` unspecified-arrival policy (design note)

- **CLOSED (2026-07-03, W-MOV).** `resolve_arrival`
  (`services/movement.rs:209-212`) picks the arrival facing with an explicit
  `match landing.facing { Some(facing) => facing, None => traveler_facing }` —
  the documented service policy is "keep the traveler's prior facing when the
  landing authored none." No `unwrap_or`, no fabricated `Facing`, no default in
  core; `Landing.facing` stays `Option<Facing>` genuine optionality
  (`data/atlas/views.rs:24`). Proof: `arrival_none_facing_keeps_traveler_facing`,
  `arrival_lands_walkable_and_inside` (the `Some` path), and the real-dataset
  `unspecified_landing_facing_keeps_the_traveler_facing`
  (`core/tests/data_files.rs`).
- **Symptom:** `Landing.facing: Option<Facing>` (`core/src/data/atlas.rs`),
  populated from `gate.direction.map(TileFacing::to_facing)`; `None` means the
  gate/target authored no arrival direction.
- **Root cause:** none — this is *correctly* modelled. Absence is genuine
  optionality, not a fabricated `Facing::default()`. The debt marker is
  forward-looking: a warp/movement service that teleports a traveler onto a
  `None`-facing `Landing` must **choose** the arrival facing, and that choice is
  **service policy** (e.g. keep prior facing, or face away from the gate), never
  a core-fabricated default. Recorded so W-MOV does not silently paper over the
  `None` with a default and reintroduce a fabricated value.
- **Resolution plan (W-MOV):** when the movement/warp service consumes a
  `Landing`, `match` on `facing`: `Some(f)` uses `f`; `None` applies an explicit,
  documented arrival-facing policy in the service. No `unwrap_or`, no default in
  core.
- **Blocked-by:** **W-MOV** — the warp / movement service that consumes
  `Landing`.

## Discharge

W-MOV is discharged when the movement service ships with its entity consumer,
the `Fixed` narrowing surface lands with normalize-to-speed, grounded steps
range against the walk grid, and the arrival-facing policy is an explicit
service `match` on `Landing.facing`. At that point D1, D2, D3, and D5 are
removed from `DEBT-INDEX.md` and this record is closed.

**DISCHARGED (2026-07-03, W-MOV).** All four conditions met — `services/movement.rs`
ships `apply_flight_change` / `flight_gate` / `change_flight` (D1),
`components/spatial.rs` ships the `Fixed` `mul`/`div`/`NonZeroFixed`/`isqrt`
narrowing surface consumed by `WorldVec::normalized_to` (D2), `commit` validates
grounded steps against the `WalkGrid` with real-Atlas cross-file tests (D3), and
`resolve_arrival` resolves `Landing.facing` with an explicit `Some`/`None` match
(D5). 209 tests pass (181 lib + 27 `data_files` + 1 smoke). D1, D2, D3, D5
removed from `DEBT-INDEX.md`; this record is CLOSED.
