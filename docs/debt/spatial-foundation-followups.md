# Debt record: spatial-foundation tech-debt follow-ups (T1–T4)

- **ID:** T1–T4 (spatial-foundation tech debt)
- **Status:** OPEN
- **Owner wave:** per sub-item (next `tile.rs` touch / W-MOV / W-ENT)
- **Created:** 2026-07-03, during the spatial-foundation work (Waves A + B,
  commits `61f1e37` and `5ed218e` on branch `spatial-foundation`).
- **Scope:** four small, guardian-flagged items in the shipped spatial code.
  Each compiles clean and passes CI today; none is a banned suppressor. They are
  root-cause-tracked here so they surface the moment their area is next touched
  and are not forgotten.

## Why this is tech debt, not a violation

Every item below works and is lint-clean. Each is a *shape* smell — a
provably-dead defensive branch, or a foundational operation shipped just ahead
of its consumer — that the type system could make impossible or that a consumer
will justify. Fix each when the owning wave touches its file; do not fabricate a
consumer or a reshape early just to clear the flag.

## Sub-items

### T1 — `narrow_u8` provably-unreachable saturation arm

- **Location:** `core/src/components/tile.rs` — `narrow_u8`, called only by
  `TileCoord::from_world`.
- **Symptom:** `from_world` computes `tx`/`ty` via `(raw >> TILE_SHIFT).clamp(0, 255)`,
  so the value handed to `narrow_u8` is already in `[0, 255]`; `narrow_u8`'s
  `Err(_) => if value < 0 { u8::MIN } else { u8::MAX }` arm is therefore
  provably unreachable. It is a *checked* `u8::try_from` (not a banned
  suppressor — no `as`, no `unwrap`, no `panic`), but it is dead defensive code.
- **Root cause:** the in-range guarantee established by `clamp(0, 255)` is proven
  at the value level, not the type level, so the narrowing conversion still has
  to model a failure path that cannot occur.
- **Resolution plan:** reshape so the in-range value is proven by type — e.g.
  have `from_world` derive the tile index through a total shift-into-`u8`
  construction (mask/clamp expressed as a total function returning `u8`
  directly), removing the fallible `try_from` and its dead arm entirely.
- **Owner:** next `tile.rs` touch (opportunistic). **Blocked-by:** none.

### T2 — `TileArea::contains` has no live consumer — CLOSED (2026-07-03, W-MOV)

- **Status:** CLOSED by **trim**. W-MOV was the wave that could have earned this
  method a consumer and did not — the movement service leashes in **world** space
  (`WorldPos::within_range`) and lands via `WalkGrid::walkable_positions_in` →
  `WorldRect::contains`; `Landing.area` is a `WorldRect`, so every live
  containment query runs on `WorldRect`, never `TileArea`. With the "earn a
  consumer or trim" question answered `trim`, the method and its lone unit test
  (`tile_area_contains_is_inclusive`) were removed from
  `core/src/components/tile.rs`. Grep before removal confirmed the only reference
  was that test (the other `.contains(` calls in the file are `WorldRect::contains`
  on a `rect` binding). This edit is a minimal, safe deletion of a confirmed-dead
  method; it does **not** touch `narrow_u8` (T1 stays open). Four gates green
  after removal (208 tests). T2 removed from `DEBT-INDEX.md`.
- **Location:** `core/src/components/tile.rs` — `TileArea::contains` (removed).
- **Symptom:** the only callers were its own unit test
  (`tile_area_contains_is_inclusive`). Every live containment query runs in
  world space through `WorldRect::contains` (e.g. `Atlas::enter_gate_at` tests a
  trigger `WorldRect`).
- **Root cause:** tile-space containment is authoring-time geometry with no
  live query — the runtime works in world space. The method existed ahead of any
  caller that never came.
- **Resolution:** trimmed (no consumer materialized in W-MOV). An unused public
  API was not kept "for symmetry."
- **Owner:** W-MOV. **Blocked-by:** none.

### T3 — `WorldVec::length_sq` has no consumer — CLOSED (2026-07-03, W-MOV)

- **Status:** CLOSED. `WorldVec::length_sq` (`components/spatial.rs:371`) now has
  two live consumers, both landed by W-MOV: `WorldVec::normalized_to`
  (`spatial.rs:331`, `self.length_sq().isqrt()` derives the magnitude divisor)
  and the arrival-clamp `seek` (`services/movement.rs:151-152`, comparing the
  remaining offset's `length_sq` against a one-step reach so a greedy step never
  overshoots the target). The foundational vector op is no longer consumer-less.
  Proof: `normalized_to_lands_within_one_sub_unit_of_speed`,
  `isqrt_is_the_integer_floor_square_root` (both `components/spatial.rs`),
  `step_at_target_is_unchanged_no_op` (`services/movement.rs`). T3 removed from
  `DEBT-INDEX.md`.
- **Location:** `core/src/components/spatial.rs` — `WorldVec::length_sq`.
- **Symptom:** defined (`dot(self, self)` → `DistanceSq`) but called nowhere in
  `core/src`. Distance queries go through `WorldPos::distance_sq` /
  `within_range`; the cone test inlines its own dot products.
- **Root cause:** `length_sq` is a foundational vector op whose natural consumer
  is normalize-to-speed / magnitude comparison, which is W-MOV's.
- **Resolution plan:** lands with `normalize` in W-MOV (the movement service's
  speed clamp compares a step's `length_sq` against a max). If W-MOV ends up not
  needing it, trim it then.
- **Owner:** W-MOV. **Blocked-by:** W-MOV (normalize consumer).

### T4 — `walk_grid` type-level totality (proven-present map handle) — CLOSED (2026-07-03, W-ENT)

- **Status:** CLOSED. W-ENT introduced the Atlas-minted `MapHandle`
  (`core/src/data/atlas.rs`): private fields, no public fabricating constructor,
  minted only from resolved state by `Atlas::map_handles()` (total iterator over
  the 11 resolved maps, no `Option`) and `Atlas::map_handle(MapNumber)` (open key
  → `Option<MapHandle>`). `MapHandle::walk_grid()` returns `&WalkGrid` — total,
  no `Option`, no `unwrap_or` at the call site; the spawn service reaches the
  grid through `handle.walk_grid()` (`services/spawn.rs::populate_map`). The
  open-key `Atlas::walk_grid(MapNumber) -> Option<&WalkGrid>` is retained for
  untrusted numbers (the "absent map 200 → None" path), so the pattern stays
  uniform with the other open-key accessors (`item`/`monster`/`skill`, all still
  `Option`). Presence for the total path is backed by the parse-proven
  map↔terrain bijection in `index_terrain`. T4 removed from `DEBT-INDEX.md`; the
  handle bundles definition + walk grid + joined spawns (richer than a bare
  `MapId`, same type-level-totality goal).

- **Location:** `core/src/data/atlas.rs` — `Atlas::walk_grid(MapNumber) -> Option<&WalkGrid>`
  over `walk_grids: BTreeMap<MapNumber, WalkGrid>`.
- **Symptom:** `walk_grid` returns `Option` even though `Atlas::parse` proves via
  `index_terrain` that *every* map carries exactly one walk grid. The `Option` is
  the correct end-state for an **open** `MapNumber` key (a raw number from
  outside the resolved set may not name a map) — but a `MapNumber` taken from a
  resolved edge (spawn, gate, landing, class home) is already proven present, so
  its caller pays an `Option` the type could retire.
- **Root cause:** map presence is proven at parse but re-expressed as runtime
  optionality at the query, because there is no distinct "proven-present map"
  type; the open `MapNumber` and the resolved-edge map share one type.
- **Resolution plan (optional):** have the `Atlas` mint a proven-present `MapId`
  handle for the maps it resolved; `walk_grid(MapId) -> &WalkGrid` becomes total
  by type, and only the open-key path (`MapNumber` → `Option<MapId>`) keeps the
  optionality. Apply consistently with the `item`/`monster`/`skill` accessors
  (all `Option` today for the same open-key reason) so the pattern is uniform,
  not one-off.
- **Owner:** W-ENT. **Blocked-by:** W-ENT (the entity/handle layer that would
  hold and pass `MapId`).

## Discharge

Each sub-item is closed independently as its owning wave touches the file: T2 and
T3 resolved by W-MOV (T2 trimmed, T3 consumer earned), T4 by W-ENT's
proven-present map handle. T1 remains the sole open item. Remove each ID from
`DEBT-INDEX.md` as it closes; close this record when T1 resolves.

**T4 CLOSED (2026-07-03, W-ENT)** — Atlas-minted `MapHandle` made the resolved
walk-grid path total by type.

**T3 CLOSED (2026-07-03, W-MOV)** — `WorldVec::length_sq` earned two live
consumers (`normalized_to` and the arrival-clamp `seek`); removed from
`DEBT-INDEX.md`.

**T2 CLOSED (2026-07-03, W-MOV)** — `TileArea::contains` proved world-space-only
(the movement service never queries tile-space containment), so it was trimmed
along with its lone unit test; removed from `DEBT-INDEX.md`.

**T1 remains OPEN** (an opportunistic `narrow_u8` reshape on the next `tile.rs`
edit); this record stays open until T1 resolves.
