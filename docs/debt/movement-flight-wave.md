# Debt record: movement / flight wave (W-MOV) deferrals

- **ID:** W-MOV (covers D1, D2, D3, D5)
- **Status:** OPEN
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
