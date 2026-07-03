# Debt record: spawn + SoccerPitch world-space resolution (D4)

- **ID:** D4 (owner wave W-ENT)
- **Status:** OPEN
- **Owner wave:** W-ENT (entities wave — spawn / soccer service)
- **Created:** 2026-07-03, during the spatial-foundation work (Waves A + B,
  commits `61f1e37` and `5ed218e` on branch `spatial-foundation`).
- **Scope:** the two data records that still carry classic **tile** authoring
  types and must resolve to **world space** at the point a service uses them —
  spawn placement and the Arena soccer pitch.

## Why this is deferred, not a violation

Tile coordinates are the *authoring* unit — the format the classic client
files are written in — and Wave A deliberately quarantines them in
`core/src/components/tile.rs` as authoring-only, with `to_world` the single
sanctioned integer→integer projection. It is correct for `spawns.json` and
`map_definitions.json` to keep authoring tiles on disk; the world-space
resolution belongs to the **service** that acts on them, not to the record
shape. That service (spawn placement / soccer setup) needs the runtime pieces
Wave A/B do not yet have — the `WalkGrid` (loaded in Wave B) plus injected RNG,
to answer "spawn at a random *walkable* world tile inside this area." Resolving
at the record shape now would either duplicate that logic in data (wrong layer)
or fabricate a resolution with no RNG/walk-grid context.

## Symptom

- `SpawnPlacement` (`core/src/data/spawns.rs`) variants `Fixed`, `Spot`, `Area`
  hold `TileCoord` / `TileArea` / `TileFacing`.
- `SoccerPitch` (`core/src/data/map_definitions.rs`) holds `TileArea` (ground,
  both goals) and `TileCoord` (both team spawns).

Both are authoring tiles with no world-space resolution at a live query — the
resolution point does not exist yet.

## Root cause

The consuming spawn / soccer service lives in W-ENT (it operates on runtime
entities and needs `WalkGrid` + injected `RngCore`), and `core/src/entities/`
is an empty placeholder. Resolution is a service responsibility, deferred with
its service.

## Resolution plan (W-ENT)

A spawn/soccer service resolves authoring tiles to world space at the point of
use, never on the record:

1. `SpawnPlacement::Fixed { position, facing }` → `position.to_world()` +
   `facing.to_facing()` for one stationary instance.
2. `SpawnPlacement::Spot { position, quantity }` → `position.to_world()` for
   each of `quantity` instances.
3. `SpawnPlacement::Area { area, quantity }` → sample random **walkable** world
   tiles inside `area.to_world()` using `Atlas::walk_grid(map)` +
   `WalkGrid::walkable` and the injected `RngCore`.
4. `SoccerPitch` fields → `ground.to_world()` / `left_goal.to_world()` /
   `right_goal.to_world()` (`WorldRect`) and `left_spawn.to_world()` /
   `right_spawn.to_world()` (`WorldPos`) when the pitch is set up.

The record shapes stay in tile space (correct authoring layer); only the
service crosses to world space.

## Blocked-by

**W-ENT** — the spawn / soccer service that consumes these records needs runtime
entity state, the loaded `WalkGrid`, and injected RNG.

## Discharge

Closed when the W-ENT spawn/soccer service resolves every `SpawnPlacement`
variant and every `SoccerPitch` field to world space via `to_world` at the
point of use, with `Area` placement using the walk grid + RNG. Then D4 leaves
`DEBT-INDEX.md`.
