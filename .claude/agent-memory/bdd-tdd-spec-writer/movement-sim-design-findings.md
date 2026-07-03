---
name: movement-sim-design-findings
description: W-MOV design characteristics surfaced while designing the movement e2e simulation — non-walkable Fixed/Spot spawns, leash=view one-tile-overshoot, WorldPos bound is type-enforced
metadata:
  type: project
---

Design characteristics of the shipped W-MOV / W-ENT code that shape how movement
invariants must be written. All verifiable by re-running scripts over `/data`; kept
here because they are non-obvious cross-references (spawns × roles × terrain × service logic).

**Fixed/Spot spawns are NOT walkability-checked.** `spawn::place_spawn` only filters
walkable tiles for `Area` placements; `Fixed` and `Spot` place at the authored tile
centre with no walk-grid check. In the real dataset **122 permanent mob-producing
Fixed/Spot spawns sit on non-walkable tiles** — mostly Devias (map 1) traps deliberately
on walls, plus real `monster`-role Spot spawns on Atlans (map 7, underwater), e.g. monsters
48 and 51.
- **Why:** traps sit on walls by design; the Atlans cases are likely a latent data/placement
  question for the user, not confirmed intended.
- **How to apply:** never write an *absolute* "every grounded mob position is walkable at
  every tick" invariant — it fails at tick 0 for non-movement reasons. Use the delta form:
  `walkable(pos_before) ⟹ walkable(pos_after)` (walkability is absorbing under the movement
  service). Flag the absolute-vs-delta distinction to the user.

**Leash radius == view range → leash-return is a narrow one-tile corrective.** `monster_ai`
sets `leash_radius = Radius::from_tiles(view_range)` and `MOB_STEP_SPEED = 1 tile`. A mob is
"within leash" up to and including exactly `view_range` tiles from anchor. Any single action
(wander drift or a chase step at the leash edge toward a radially-further target) can push it
at most `1 tile + ≤1 sub-unit` beyond the leash edge; leash-return (higher precedence than
chase/attack) then yanks it back next action. So the max excursion from anchor is bounded by
`view_range*tile + one tile-step + ~1 sub-unit`.
- **Why:** the in-code comment claims "a chase toward a target it can see stays inside the
  leash" — that is slightly optimistic; a boundary chase step CAN overstep by one tile.
- **How to apply:** the leash-bound invariant is real and satisfiable; assert the `+one step`
  bound, not strict containment. To trigger leash deterministically in a targeted test, place
  the mob exactly at the leash edge with a target one tile beyond and `attack_range=0`.

**WorldPos bounds are type-enforced, not a runtime concern.** `WorldPos` fields are private
and only built via `clamped`/`new`, both of which keep components in `[0, WORLD_EXTENT]`; the
`Add<WorldVec>` op clamps. An out-of-world position is unrepresentable.
- **How to apply:** a whole-run "position component in `[0, WORLD_EXTENT]`" assertion is
  tautological (restates the type) and has no teeth — cut it. World-edge clamp *behavior* is
  already unit-tested (`drift_clamps_at_the_world_edge`).

**Cadence edge:** 3 trap mobs (100/101/102) carry `move_delay_ms = 0`; combined with
`move_range = 0` they are Idle-and-reschedule-by-zero → ready every tick but never move.
Scope any "action count ≈ N / period" cadence assertion to *moving* mobs (`move_range > 0`,
`move_delay_ms > 0`; 77 mobs at 400ms, 1 at 500ms).

**Real environments present:** map 10 (Icarus) is the only `Sky` map (7395 walkable tiles);
map 7 (Atlans) is the only `Underwater` map. Both usable for real-terrain flight/arrival tests.
