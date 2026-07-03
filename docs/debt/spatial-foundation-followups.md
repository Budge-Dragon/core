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

### T2 — `TileArea::contains` has no live consumer

- **Location:** `core/src/components/tile.rs` — `TileArea::contains`.
- **Symptom:** the only callers are its own unit test
  (`tile_area_contains_is_inclusive`). Every live containment query runs in
  world space through `WorldRect::contains` (e.g. `Atlas::enter_gate_at` tests a
  trigger `WorldRect`).
- **Root cause:** tile-space containment is authoring-time geometry with no
  live query — the runtime works in world space. The method exists ahead of any
  caller.
- **Resolution plan:** earn a consumer (a spawn/soccer service that genuinely
  answers a tile-space containment question) or trim the method. Do not keep an
  unused public API "for symmetry."
- **Owner:** W-MOV / next `tile.rs` touch. **Blocked-by:** W-MOV (whether a tile
  consumer materializes).

### T3 — `WorldVec::length_sq` has no consumer

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

### T4 — `walk_grid` type-level totality (proven-present map handle)

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

Each sub-item is closed independently as its owning wave touches the file: T1 on
the next `tile.rs` edit, T2/T3 when W-MOV resolves their consumer question, T4
if/when W-ENT introduces the proven-present map handle. Remove each ID from
`DEBT-INDEX.md` as it closes; close this record when all four are resolved.
