# mu-core — Workplan

The durable roadmap for mu-core. This is the single doc a session opens to know
what to build next and how. It outlives any one session.

## How to use this doc across sessions

- **Each wave below is independently executable in a fresh session** with zero
  prior context. Everything a session needs — goal, prerequisites, scope in/out,
  types to build, read-first list, pipeline, definition of done, open decisions —
  lives in that wave's section.
- **Pick a wave** using the dependency graph and recommended sequence below.
  Confirm its prerequisites are met (check the per-wave *Status* line and the
  *Status Log* at the bottom).
- **As you work, keep this doc current:** update the wave's **Status** line
  (`NOT STARTED` → `IN PROGRESS` → `DONE`) and append a row to the **Status Log**
  at the bottom for every meaningful event (wave started, wave landed, debt row
  closed, decision pinned). The Status Log is the durable memory across sessions.
- **When a wave closes debt**, remove the corresponding rows from
  `docs/debt/DEBT-INDEX.md` and mark the record closed — the definition of done
  for each wave lists exactly which rows.
- **Do not edit `core/` to write this doc or the plan** — planning is read-only
  against core.

---

## Fresh-session bootstrap (read-first checklist)

A session starting cold must load context in this exact order before touching
anything:

1. **`CLAUDE.md`** (repo root) — the Four Iron Laws, the supporting rules, the
   agent roster, and the pipelines. This is the physics of the codebase.
2. **`README.md`** (repo root) — the six portability rules and the four host
   targets. The Iron Laws are abstract; these are their host-facing consequences.
3. **`docs/WORKPLAN.md`** (this file) — pick the wave, read its section end to end.
4. **`docs/debt/DEBT-INDEX.md`** — the 14 open backlog items (W-SRC, D1–D5,
   T1–T4, Q1–Q4) and which wave owns each. Read the specific debt records the
   chosen wave names.
5. **`docs/adr/`** — the accepted architecture decisions that bind every wave.
   Currently [`0001-event-driven-core-shape.md`](adr/0001-event-driven-core-shape.md):
   the core is event-driven as events-as-**output**, *not* event sourcing — no
   `decide`/`evolve` fold, no event store in core. Do not re-litigate an accepted
   ADR; build to it.
6. **The wave's own "Read first" list** — the exact source files and specs that
   ground that wave in what already exists.

### The non-negotiables (one paragraph, always in force)

mu-core **is** the hexagon: pure domain logic, zero host dependencies, deps are
exactly `serde` + `rand_core`. **Hexagonal purity** — hosts depend on core, never
the reverse; no engine/DB/network/clock/global-RNG concept ever names itself in
core, and no information leaks across module boundaries (a component never rolls
dice; an entity never decides; an event never computes). **Enums everywhere** —
every type with more than one shape is a flat enum; `Option<T>` is genuine
optionality only, never a state flag. **Make illegal states unrepresentable** —
if a function runs, its preconditions are already proven by its types; validation
lives at host boundaries via parse-don't-validate, never inside core. **No
type-system suppressors** — no `unwrap`/`expect`/`panic`/`unreachable`/`todo`,
no wildcard `_ =>` arms on domain enums, no `#[non_exhaustive]`, no inline
`#[allow]`/`#[expect]`, no slice indexing, no lookup-shaped `unwrap_or`, no lossy
`as` casts, no `unsafe`, no fabricated `Default`s; the fix is always upstream in
the type, never a runtime check. **Determinism** — every service is
`(state, input, &mut impl RngCore) -> (new state, events)`; same inputs + same
seed = identical outputs on native, wasm, and FFI; advancing the injected RNG is
the only sanctioned argument mutation. **Events, not effects** — every observable
outcome is a value in the returned events; core has no I/O. "Event-driven" for
this project means events-as-**output** from pure transitions, *not* event
sourcing (no `decide`/`evolve` fold, no event log in core) — see
[`docs/adr/0001-event-driven-core-shape.md`](adr/0001-event-driven-core-shape.md).

**The four green gates** every wave must pass before it is done:

```
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
cargo check -p mu-core --target wasm32-unknown-unknown
```

---

## Current-state snapshot

Verified against the repo on 2026-07-03.

- **Branch:** `spatial-foundation`. Spatial work lands here; merge to `main` is a
  separate step.
- **DONE:**
  - Full static-data layer (`core/src/data/`) — monster/item/skill/map/drop/exp
    definitions, chaos mixes, ancient sets, gates/warps, classes, game-config —
    plus the referential-integrity **`Atlas`** (`core/src/data/atlas.rs`) with
    **total lookups** hosts fill via the `StaticData` port and `Atlas::parse`.
  - **Spatial foundation Wave A + Wave B** (commits `61f1e37`, `5ed218e`):
    `core/src/components/{spatial,tile,movement}.rs` — fixed-point `Q40.24`
    world space, `WorldPos`/`WorldVec`/`Facing`/`Region`/`Radius`, tiles &
    `WalkGrid`, the two-state `Movement` classifier — world-space Atlas migration
    and terrain-backed walk grids loaded from `data/terrain/<map>.bin` sidecars.
  - Green: `clippy -D warnings` clean, ~120 tests passing, wasm check OK.
- **EMPTY / MINIMAL:**
  - `core/src/entities/mod.rs` — doc-comment-only stub. **No entity types yet.**
  - `core/src/events/mod.rs` — doc-comment-only stub. **No event enums yet.**
  - `core/src/services/` — only `chance.rs` (pure RNG rolls: `WeightedTable`,
    `weighted_pick`, `pick_one`, `roll_*`) and `item_rules.rs` (pure const-table
    lookups). **No stateful/behavioral service** (nothing takes entity state and
    returns events).
  - `hosts/` — placeholder `README.md` only. **No host crate.**
- **Backlog:** `docs/debt/DEBT-INDEX.md` — 14 open items: **W-SRC** (data
  provenance), **D1–D5** (movement/flight + world-space resolution),
  **T1–T4** (spatial follow-ups), **Q1–Q4** (tooling/CI quality gaps).

---

## Wave dependency graph + recommended sequence

```
                 (foundation DONE: data layer + Atlas + spatial A/B)
                                   |
                                   v
        +-------------------- W-ENT (entities + spawn) ---------------------+
        |                          |          closes D4, T4                 |
        |                          |                                        |
        v                          v                                        |
     W-CMB  <--- swappable --->  W-MOV                                      |
   (combat)                    (movement/flight, closes D1 D2 D3 D5 T2 T3)  |
        \                          /                                        |
         \                        /                                         |
          +----------> W-HOST <--+  (needs W-ENT state + W-MOV service)     |
                         |                                                   |
                         v                                                   |
                  broader behavioral waves ------------------------- W-SRC (independent,
             (W-CRAFT, W-SHOP, W-PARTY, W-DS, W-BC, W-CC, W-INV, W-EFFECT, W-AI)   data-only)

   W-HARDEN (Q1-Q4 tooling/CI) — no domain prerequisite; schedulable anytime.
   W-SRC (data provenance) — no code prerequisite; independent of every code wave.
```

**Recommended order:**

1. **W-ENT** — live entity aggregates + spawn placement. Unblocks everything
   behavioral; closes D4 and T4. **Run W-HARDEN alongside** it (independent, no
   domain dependency — good parallel/interstitial work).
2. **W-CMB** — combat / kill-reward loop. (Blocked by W-ENT.)
3. **W-MOV** — movement & flight; closes D1/D2/D3/D5/T2/T3. (Blocked by W-ENT.)
   - **W-CMB and W-MOV are swappable** — either can run second; both need only
     W-ENT. Pick by appetite.
4. **W-HOST** — first native host adapter (the hexagon litmus test). Needs W-ENT
   entity state to persist **and** W-MOV as the first stateful, event-producing
   service to drive. (If W-CMB lands before W-MOV, the harness can drive combat
   instead — the adapter shape is identical.)
5. **Broader behavioral backlog** (`W-SRC-PLUS` Part B: W-CRAFT is highest
   leverage, then W-SHOP / W-PARTY / event dungeons / W-INV / W-EFFECT / W-AI).

**Schedulable anytime, independent of the sequence:**

- **W-HARDEN** (Q1–Q4) — tooling & CI hardening. No domain prerequisite.
- **W-SRC** (`W-SRC-PLUS` Part A) — data provenance re-sourcing. Data-only,
  touches zero Rust, blocked by nothing in code.

---

## W-ENT — Live Entity Aggregates + Spawn Placement

- **Status:** NOT STARTED
- **Goal:** Stand up the live, mutable world-presence entities every behavioral
  system acts on — `Character` and `MonsterInstance` composed from the existing
  value components plus new `Placement`/`Pool`/`Vitals`/`Stats` — and a
  deterministic spawn-placement service that resolves authoring-tile spawn
  records to walkable world positions, unblocking combat/movement/AI and closing
  the spawn debt.
- **Prerequisites / blocked-by:** None outstanding. The spatial foundation
  (`core/src/components/spatial.rs`, `tile.rs` with `WalkGrid`, `movement.rs`),
  the whole data layer + referential-integrity `Atlas` (`core/src/data/atlas.rs`,
  incl. `walk_grid`/`monster` lookups and terrain-backed walk grids), and the
  injected-RNG seam (`core/src/rng/mod.rs`: `uniform_below`,
  `uniform_below_usize`) all already exist and compile. `core/src/entities/mod.rs`
  and `core/src/events/mod.rs` are today empty doc-only stubs — this wave fills
  them.
- **Resolves backlog items:** **D4** (spawn world-space resolution — the
  monster-placement clause fully; the `SoccerPitch` clause via the
  world-population entry point — see Open decisions) and **T4** (`walk_grid`
  type-level totality via an Atlas-minted proven-present map handle). Both name
  W-ENT as owner in `docs/debt/DEBT-INDEX.md`.
- **Scope — IN:**
  - New components (value types with invariants, `components/`):
    - `Pool` — a bounded resource `{ current: u32, max: u32 }` with
      `current <= max` proven by smart constructor (`new`) + total `clamped`
      compute-path constructor. The canonical gauge for HP/mana/AG.
    - `Vitals` — a character's three `Pool`s: `health`, `mana`, `ability`.
      (Monsters carry a bare `health: Pool`, not `Vitals` — no fabricated
      mana/AG.)
    - `Stats` — live allocated primary stats, an enum mirroring
      `data::classes::StartingStats`:
      `Standard { strength, agility, vitality, energy }` vs
      `WithCommand { …, command }`, so `command` exists only on the Dark Lord
      shape (illegal state unrepresentable).
    - `Placement` — mobile-entity spatial state
      `{ position: WorldPos, facing: Facing, movement: Movement }`, composing
      `spatial::{WorldPos, Facing}` + `movement::Movement`. Mobile entities only
      (see WorldItem deferral).
  - New aggregates (`entities/`):
    - `MonsterInstance` `{ number: MonsterNumber, placement: Placement,
      health: Pool }` — a live fighting mob. `number` back-references the
      `MonsterDefinition`; combat/behavior columns (`MonsterCombat`,
      `MobBehavior`) stay in the `Atlas` and are read at use-time (no state
      duplication). `health` is seeded from the def's `MonsterCombat.hp` at spawn.
    - `Character` `{ class: CharacterClass, level: Level, experience: Exp,
      stats: Stats, unspent_points: u16, placement: Placement, vitals: Vitals }`
      — a live, gear-less player entity sufficient for combat/movement/leveling
      to act on. Composes `components::class::CharacterClass`,
      `units::{Level, Exp}`, and the new `Stats`/`Vitals`/`Placement`.
    - A spawned-output enum (recommended name `Spawned`) dispatched exhaustively
      over `data::monster_definitions::MonsterRole`: combat-carrying roles
      (`Monster`, `Guard`, `Trap` — each holds `MonsterCombat`) yield
      `Spawned::Mob { instance: MonsterInstance }`; non-combat roles (`Npc`,
      `SoccerBall` — no `MonsterCombat`) yield
      `Spawned::Placed { number, placement }` (position+facing, no fabricated
      `Pool`). Every role handled, no wildcard.
  - New service `services/spawn.rs` (the keystone, resolves D4's monster clause):
    given a `Spawn`, its resolved `MonsterDefinition`, the map's `WalkGrid`, and
    `&mut impl RngCore`, produces the placed live entities, crossing to world
    space only here (per D4's "only the service crosses"):
    - `SpawnPlacement::Fixed { position, facing }` → one instance at
      `position.to_world()` facing `facing.to_facing()`.
    - `SpawnPlacement::Spot { position, quantity }` → `quantity` instances at
      `position.to_world()`.
    - `SpawnPlacement::Area { area, quantity }` → enumerate the walkable tile
      centres inside `area.to_world()` (`WalkGrid::walkable`), then draw
      `quantity` positions uniformly via `rng::uniform_below_usize` — bounded and
      deterministic, never unbounded rejection; an area with no walkable tile
      contributes zero instances.
  - A world-population entry point (recommended
    `services/spawn.rs::populate_map` or a thin `world` service) that runs a
    map's initial `SpawnSchedule::Permanent` spawns through the above and (D4
    soccer clause) resolves that map's `MapDefinition.soccer_pitch`
    `TileArea`/`TileCoord` fields to a world-space `ResolvedSoccerPitch`
    (`ground/left_goal/right_goal: WorldRect`, `left_spawn/right_spawn: WorldPos`)
    when present. This is the single consumer that makes the pitch resolution
    non-scaffolding.
  - Atlas map-handle (resolves T4): a proven-present map token minted by the
    `Atlas` (e.g. `Atlas::map(MapNumber) -> Option<MapHandle>`, then `MapHandle`
    yields `&WalkGrid` + its spawns totally), so the spawn service receives
    `&WalkGrid`, never `Option`.
  - First `events/` outcome enum seeded (recommended
    `SpawnEvent::MonsterSpawned { number, at: WorldPos, facing: Facing }`)
    returned alongside the placed state, so the host can deliver "an entity
    appeared" without re-deriving it.
- **Scope — OUT (deferred):**
  - **Combat math, damage, death, vitals mutation** → **W-CMB**.
    `MonsterInstance`/`Character` carry the state; nothing subtracts HP here.
  - **Movement, flight eligibility, `Movement` mode transitions, warp arrival** →
    **W-MOV** (owns D1/D2/D3/D5). Spawned mobs are placed as `Movement::Grounded`
    (Area placement samples walkable tiles — Grounded is the correct value, not a
    default); flight classification is W-MOV's.
  - **Monster AI, aggro, wandering, respawn-on-death timing** → **W-AI**
    (respawn needs ticks + death events).
  - **`WorldItem`, live `ItemInstance` (rolled rarity/options/durability),
    `Inventory` grid, `Equipment` slots** → **W-INV / loot wave.** An item
    instance is inert without the drop-time option-**roll** service that populates
    it and the container invariants that hold it — both belong to that wave.
    Building the types now, with no service to fill them and no container to hold
    them, is exactly the speculative scaffolding Iron Law 4 forbids. Only two live
    aggregates ship here; `WorldItem` lands with the systems that give it meaning.
    `ItemInstance` will compose the already-built `components/item_options.rs`,
    `levels.rs`, `item_quality.rs` when that wave runs.
  - **Character combat-stat derivation, stat-point allocation, leveling/exp curve
    application** → their own service waves. `Character` holds
    `stats`/`level`/`experience`/`unspent_points` as data; no service computes
    from them here.
- **Key types / modules to build:** `components/pool.rs` (`Pool`),
  `components/vitals.rs` (`Vitals`), `components/stats.rs` (`Stats` — mirrors
  `data/classes.rs::StartingStats`), `components/placement.rs` (`Placement` over
  `spatial::{WorldPos, Facing}` + `movement::Movement`);
  `entities/monster_instance.rs` (`MonsterInstance`), `entities/character.rs`
  (`Character`), `entities/spawned.rs` (`Spawned` enum over
  `data/monster_definitions.rs::MonsterRole`); `services/spawn.rs` (placement +
  `populate_map`, consuming `data/spawns.rs::{Spawn, SpawnPlacement,
  SpawnSchedule}`, `data/monster_definitions.rs::{MonsterDefinition, MonsterRole,
  MonsterCombat}`, `data/map_definitions.rs::{MapDefinition, SoccerPitch}`,
  `components/tile.rs::{TileCoord, TileArea, TileFacing, WalkGrid}`,
  `rng::{uniform_below, uniform_below_usize}`); an `Atlas` map-handle addition in
  `data/atlas.rs` (T4); `events/mod.rs` first enum (`SpawnEvent`). Register every
  new `components`/`entities` module in its `mod.rs` and `services/spawn` in
  `services/mod.rs`.
- **Read first (fresh session):** `CLAUDE.md` + `README.md`. Then:
  `core/src/entities/mod.rs`, `core/src/events/mod.rs`,
  `core/src/components/{spatial,movement,tile,class,units,levels,element,bonus}.rs`,
  `core/src/data/{spawns,monster_definitions,map_definitions,classes,atlas,common}.rs`,
  `core/src/rng/mod.rs`, `core/src/services/chance.rs` (the
  `(…, &mut impl RngCore)` service + injected-RNG pattern, incl. the test
  `TestRng`), `core/tests/data_files.rs` (how the real `Atlas` is loaded in
  tests), and the debt records `docs/debt/DEBT-INDEX.md`,
  `docs/debt/worldspace-resolution-services.md` (D4),
  `docs/debt/spatial-foundation-followups.md` (T4).
- **Pipeline:** Core Domain Feature — `bdd-tdd-spec-writer` →
  `core-architecture-guardian` (plan) → `canon-guardian` (plan) →
  `state-machine-agent` → implementation → `core-architecture-guardian` (code) →
  `deep-module-guardian` → `canon-guardian` (code) → `debt-guardian` →
  `rules-guardian`.
- **Definition of done:** Four green gates (see bootstrap) plus wave acceptance:
  - `Pool::new` rejects `current > max`; `Pool`/`Vitals`/`Stats`/`Placement`
    round-trip serde with stable `kind`-tagged wire shapes (`Stats`
    command-on-DL-only enforced).
  - Spawn service: `Fixed` → exactly one instance at the tile-centre world
    position with the projected facing; `Spot` → `quantity` instances at the tile
    centre; `Area` → `quantity` instances **each on a walkable tile inside the
    area** (proptest over multiple seeds asserts `WalkGrid::walkable` holds for
    every placed position, and an all-blocked area yields zero).
  - Determinism: same `Spawn` + same seed ⇒ identical placements and identical
    RNG-word consumption (assert bit-for-bit equality across two runs).
  - `MonsterInstance.health` initialized `current == max == MonsterCombat.hp`;
    `Spawned` dispatch is exhaustive over all five `MonsterRole` variants with no
    fabricated `Pool` for `Npc`/`SoccerBall`.
  - `Atlas` map handle makes the spawn service's walk-grid access total (no
    `Option`/`unwrap_or` at the call site).
  - `populate_map` on Arena resolves the `SoccerPitch` to world space; the real
    dataset loads and populates every map without error (extend
    `core/tests/data_files.rs`).
  - `D4` and `T4` rows removed from `docs/debt/DEBT-INDEX.md` and their records
    closed (debt-guardian).
- **Open decisions / risks:**
  1. **`SoccerPitch` clause of D4.** Fully closing D4 needs a live consumer of the
     resolved pitch. *Recommended:* resolve it inside `populate_map` (the same
     world-init that runs spawns) so it is not scaffolding, closing D4 whole. If
     the executing session judges the ball entity/kick physics too coupled, split
     D4 (spawn clause closes now; pitch clause rides the Arena/soccer wave) —
     debt-guardian's call.
  2. **Spawn facing for `Spot`/`Area` monsters** (no authored `TileFacing`).
     *Recommended:* draw a cardinal `Facing` via the injected RNG (authentic MU
     spawns face randomly); deterministic fallback is a fixed `Facing::POS_Y`
     (South). Pick one and pin it in a test.
  3. **`Npc`/`SoccerBall` live representation.** *Recommended:* the
     `Spawned::Placed { number, placement }` variant (position+facing only) —
     handles every role exhaustively without a fabricated `Pool`; full NPC
     dialog/soccer-ball physics defer to their waves.
  4. **`WorldItem`/items deferral** (see Scope — OUT). *Recommended:* confirm with
     the user/debt-guardian that shipping two live aggregates (not three) is
     accepted, on the no-scaffolding grounds above.
  5. **T4 handle surface.** Minting a `MapHandle` widens the `Atlas` API.
     *Recommended:* keep it minimal (yields `&WalkGrid` + the map's spawns),
     justified by removing the `Option` at the spawn call site;
     deep-module-guardian confirms it deepens rather than widens.

---

## W-CMB — Combat: Attack Resolution, Skill Damage, and the Kill Reward Loop

- **Status:** NOT STARTED
- **Goal:** Turn the built static-data surface into the live reward loop — a
  struck target resolves to hit/crit/excellent/miss, dies, drops loot, and awards
  experience that levels the killer — as pure
  `(state, input, &mut RngCore) -> (state, events)` services.
- **Prerequisites / blocked-by:** **W-ENT** (blocking) — `core/src/entities/` is
  an empty placeholder. Combat operates on live entity aggregates W-ENT must first
  define: a combatant exposing resolved combat stats (min/max physical damage,
  wizardry damage, defense, attack-rate, defense-rate, and the chance stats
  crit/excellent/defense-ignore/double), current/max `Vitals` (HP/MP/AG), a
  `Level`, accumulated `Exp`, a `WorldPos`, a `Facing`, and
  `PerElement<Resistance>`. Monster base combat columns already exist in
  `core/src/data/monster_definitions.rs` (`MonsterCombat`, `resistances`); the
  *player* resolved profile is W-ENT's aggregation of `CombatBonus`
  (`core/src/components/bonus.rs`). W-CMB consumes these; it does not build stat
  aggregation. Atlas (`core/src/data/atlas.rs`) already exists and is a
  prerequisite artifact.
- **Resolves backlog items:** none. (D2 checked below and explicitly **not**
  pulled in; D4/D1/D3/D5 are W-ENT/W-MOV.) One small in-wave Atlas extension is
  required — see risks.
- **Scope — IN:**
  - **Attack resolution** (physical, PvM): hit-vs-miss from attacker attack-rate
    vs target defense-rate with the 3% floor; base damage rolled uniformly in the
    attacker's `[min, max]` physical span; defense subtraction; the PvM overrate
    0.3 penalty when defender defense-rate exceeds attacker attack-rate; the
    `max(1, attacker_level / 10)` minimum-damage floor; critical (= max damage)
    and excellent (= 1.2 x max damage) rolls from the attacker's chance stats;
    defense-ignore roll. All damage to health (pre-S3; no shield split).
  - **Damaging skill application** with spatial gating: `SkillShape::DirectHit` /
    `Lunge` (single target, range-gated via `WorldPos::within_range`) and
    `SkillShape::Area { pattern }` (multi-target, gated by a `Region` built per
    `AreaPattern`). The 18-variant `AreaPattern` match
    (`core/src/data/skills.rs`) is the promised per-skill routine set: each
    variant constructs the correct `Region::{Circle, Cone, Rect}`
    (`core/src/components/spatial.rs`) from the caster/target `WorldPos`,
    `Facing`, skill `range` (as `Radius`), and a per-pattern `ConeHalfWidth`, then
    filters candidate targets by `Region::contains`. Cast-cost affordability
    (`CastCost` mana/ability) resolves to a `SkillOutcome::Rejected` event (a real
    domain outcome, per the "skill rejected" event in CLAUDE.md), not a defensive
    check. Physical vs wizardry damage per `DamageType`; base skill damage from
    `Skill::attack_damage`.
  - **Element / ailment gating:** on a landed elemental hit, the skill's
    `inflicts: Option<Ailment>` (`core/src/data/effects.rs`) is applied iff the
    target does **not** resist, reusing `services::chance::roll_resistance`
    against the target's `PerElement<Resistance>` element byte. (Ailment
    *timers/magnitudes* and lightning's 1-tile displacement are OUT — see below.)
  - **Death → drop resolution:** when an attack outcome is `Killed`, the per-kill
    category partition from `core/src/data/drop_config.rs` (`DropConfig`: money /
    item / jewel / excellent rolls plus the `nothing_weight()` remainder — a
    five-way partition of `0..10_000`); the item category draws from
    `Atlas::drop_pool().in_window(Interval<u8>)` (`core/src/data/atlas.rs`
    `DropPool::in_window`, window = `[monster_level - gap, monster_level]`) via
    `services::chance::pick_one`; the jewel category from `DropConfig::jewel_drops()`;
    plus per-kill special drops from `Atlas::special_drops()`
    (`SpecialDrop::{LevelBanded via DropBands::item_level_for, MonsterBound,
    MapBound}`, `core/src/data/special_drops.rs`). Dropped-instance
    plus-level/rarity feeds `services::item_rules::effective_drop_level`
    (`core/src/services/item_rules.rs`).
  - **XP award + leveling:** per-kill base experience
    `(target_level + 25) * target_level / 3`, the killer-over-level penalty
    `* (target_level + 10) / killer_level` when `killer_level > target_level + 10`,
    the `+ (target_level - 64) * (target_level / 4)` bonus at `target_level >= 65`,
    the `* 1.25` flat factor, then the uniform `[0.8, 1.2]` jitter drawn through
    the RNG (values already in data:
    `game_config.progression.exp_jitter_percent`, a
    `RatePercentRange = Interval<u16>`). The gained `Exp` accumulates onto the
    killer and is compared against `Atlas::exp_curve()`
    (`ExpCurve::level` / `CurveLevel::total_to_hold`,
    `core/src/data/exp_tables.rs`) to emit `LevelUp` events, capped at
    `ExpCurve::max_level()`.
  - **Events** (the currently-empty `core/src/events/` module gets its first
    residents).
- **Scope — OUT (deferred):**
  - **Non-damaging skill effects** — `SkillShape::{BuffSelf, BuffPlayer,
    BuffPartyMember, BuffParty, Heal, Summon, Teleport, NovaCharge,
    RecallParty}` and the `Buff` roster (`core/src/data/effects.rs`): timed-effect
    application, summons, and teleport belong to a future **W-EFFECT** wave.
    W-CMB's skill service is fed only damaging shapes (see Open decision 1) so its
    dispatch stays total without a placeholder arm.
  - **Ailment lifetime** — poison ticks, freeze/ice movement factors,
    defense-reduction magnitude and expiry: timed-effect bookkeeping,
    **W-EFFECT**. W-CMB only *sets* the ailment on a landed hit.
  - **Monster AI target-selection** — aggro, `view_range` scanning, and which
    target a monster attacks: shared with and owned by **W-MOV** (movement/aggro).
    W-CMB receives an already-chosen attacker+target and resolves the exchange.
  - **Lightning 1-tile displacement** on elemental application — requires a
    movement step; **W-MOV**.
  - **Box opening** (`Atlas::box_drops()`, `core/src/data/box_drops.rs`) — an
    item-*use* action, not a kill drop; a future items/use wave.
  - **PvP** — attack-rate/defense-rate PvP variants and PvP-only damage factors
    (facts doc 6): a later wave; this wave is player-vs-monster only.
  - **Stat aggregation** — folding equipment/option/set `CombatBonus` into the
    resolved combatant profile: **W-ENT**.
- **Key types / modules to build:**
  - `core/src/services/combat.rs` —
    `resolve_attack(attacker: &<W-ENT combatant profile>, target: &<W-ENT
    combatant>, &mut impl RngCore) -> (Vitals, AttackOutcome)`. Composes
    `services::chance::{roll_per_10000, roll_percent}` for
    hit/crit/excellent/defense-ignore, a new `uniform_in_inclusive` primitive
    (below) for the damage span, and the `max(1, level/10)` floor from the
    attacker's `Level` (`core/src/components/units.rs`).
  - `core/src/services/skills.rs` — targeting + damaging-shape dispatch.
    Exhaustive match over `data::skills::AreaPattern` building
    `components::spatial::{Region, Radius, ConeHalfWidth, Facing}`; per-target
    damage via `services::combat`. Consumes `Skill` (`core/src/data/skills.rs`)
    resolved through `Atlas::skill`.
  - `core/src/services/loot.rs` —
    `resolve_kill_drops(killer, victim, map, &Atlas, &mut impl RngCore) ->
    DropResolution`. Category partition over `DropConfig` (`nothing_weight`, four
    rolls) built as a `services::chance::WeightedTable`; pool query via
    `DropPool::in_window` + `Interval` (`core/src/components/interval.rs`);
    special drops via `Atlas::special_drops()`; rarity/level via
    `services::item_rules::effective_drop_level`.
  - `core/src/services/experience.rs` —
    `award_kill_experience(killer, victim, &Atlas, &mut impl RngCore) -> (Exp,
    Vec<LevelUp>)`. Jitter from `Atlas::progression()` (new accessor — see risks),
    curve from `Atlas::exp_curve()`.
  - New RNG-seam primitive in `core/src/services/chance.rs`:
    `uniform_in_inclusive(interval: Interval<u16>, &mut impl RngCore) -> u16`
    (damage span and any inclusive range roll), built on `crate::rng::uniform_below`
    — the one place a bounded roll meets the RNG, consistent with the module's
    existing style.
  - **Events** (`core/src/events/`), flat internally-tagged enums per Iron Law 2:
    - `core/src/events/combat.rs` — `AttackOutcome { Missed | Hit { damage:
      Damage } | Critical { damage: Damage } | Excellent { damage: Damage } |
      Killed { damage: Damage } }` (matching the canonical `AttackOutcome` in
      CLAUDE.md), and a `Damage` newtype (`u32`) with a small `DamageAttributes`
      flag set (defense-ignored / double) as flat fields on the carrying variant.
    - `core/src/events/skills.rs` — `SkillOutcome { Rejected { reason:
      CastRejection } | Cast { hits: Vec<TargetHit> } }`, `CastRejection {
      InsufficientMana | InsufficientAbility | OutOfRange | NoTargetsInRegion }`.
    - `core/src/events/loot.rs` — `Drop { Zen { amount: Zen } | Item { item:
      ItemRef, level: ItemLevel, rarity } | Nothing }` and `DropResolution`
      bundling the per-kill drops.
    - `core/src/events/progression.rs` — `ExpAward { gained: Exp }`, `LevelUp {
      level: Level }`.
    - A top-level `KillResolution` bundling `DropResolution` + `ExpAward` +
      `Vec<LevelUp>` returned by the death pathway.
  - **D2 check (required by the brief):** damage scaling does **not** need the D2
    fixed-point `mul`/`div`/`NonZeroFixed` narrowing surface. Every fractional
    constant here is an exact integer ratio applied to a `u32`/`u64` magnitude —
    overrate `* 3/10`, excellent `* 6/5`, exp `* 5/4`, jitter `[0.8, 1.2]` as an
    `Interval<u16>` of percent points, floor `level / 10` — computed with
    widening + saturating multiply then integer division, never on the spatial
    `Fixed` (`Q40.24`) type. D2's sole first consumer stays W-MOV
    normalize-to-speed. Recommendation: do **not** pull D2 forward; combat gets a
    tiny pure `scale_ratio(value: u32, num, den) -> u32` helper (widen to `u64`,
    saturating-mul, integer-div, narrow with `TryFrom`) local to `services`.
- **Read first (fresh session):** `core/src/services/chance.rs`,
  `core/src/services/item_rules.rs`, `core/src/rng/mod.rs`,
  `core/src/components/spatial.rs`, `core/src/components/units.rs`,
  `core/src/components/levels.rs`, `core/src/components/bonus.rs`,
  `core/src/components/element.rs`, `core/src/components/interval.rs`,
  `core/src/components/collections.rs`,
  `core/src/data/monster_definitions.rs`, `core/src/data/skills.rs`,
  `core/src/data/effects.rs`, `core/src/data/drop_config.rs`,
  `core/src/data/special_drops.rs`, `core/src/data/exp_tables.rs`,
  `core/src/data/game_config.rs`, `core/src/data/atlas.rs`,
  `core/src/data/item_definitions.rs`,
  `docs/reference/openmu-facts/6_game_constants_+_experience_+_damage-rel.md`,
  and whatever W-ENT ships in `core/src/entities/`. (`CLAUDE.md` + `README.md`
  implied.)
- **Pipeline:** Core Domain Feature — `bdd-tdd-spec-writer` →
  `core-architecture-guardian` (plan) → `canon-guardian` (plan) →
  `state-machine-agent` → implementation → `core-architecture-guardian` (code) →
  `deep-module-guardian` → `canon-guardian` (code) → `debt-guardian` →
  `rules-guardian`.
- **Definition of done:** four green gates — with no wildcard arms on the
  `SkillShape`/`AreaPattern`/`AttackOutcome` matches. Plus wave-specific
  acceptance, all via a seeded deterministic RNG (the `SplitMix64` `TestRng`
  pattern in `chance.rs` tests):
  - Same seed + same inputs reproduce identical `AttackOutcome`, drops, and exp
    bit-for-bit (determinism).
  - Hit chance honors the 3% floor at defense-rate >= attack-rate; overrate
    applies the `3/10` scale; the `max(1, level/10)` floor holds at zero net
    damage.
  - Critical yields exactly max damage; excellent yields `6/5 * max`; both gate on
    their chance stats (0% never, 100% always).
  - Region gating: a `Cone` skill hits a target inside its `ConeHalfWidth` and
    range and excludes one outside either; a `Circle` pattern hits within `Radius`
    only — asserted against `spatial` predicates directly.
  - Ailment applies iff `!roll_resistance` for the element's byte (255 = immune,
    0 = always).
  - Drop category partition weights sum to 10,000 (four rolls + `nothing_weight`);
    the item draw lands only in `drop_pool.in_window` for the kill's level band; a
    `LevelBanded` special drop resolves the correct `ItemLevel` via
    `DropBands::item_level_for`.
  - Exp curve: a kill pushing accumulated `Exp` across `CurveLevel::total_to_hold`
    emits exactly one `LevelUp`; crossing several thresholds emits several; award
    at `max_level` emits none.
- **Open decisions / risks:**
  1. **Skill-dispatch totality without a placeholder.** `SkillShape` has 12
     variants but this wave implements only the damaging three. *Recommended:*
     mint a `DamagingSkill` subset (a narrow enum or newtype produced upstream at
     the routing boundary) so `services::skills` receives only `DirectHit | Lunge
     | Area` and its match is genuinely total; the non-damaging shapes route to
     W-EFFECT. Alternative (rejected as a half-fold): match all 12 and return a
     non-damage outcome variant — leaves buff/heal effects unimplemented behind a
     passing match.
  2. **Atlas must retain `ProgressionConfig`.** `Atlas::parse` currently keeps
     only `game_config.drops` and discards `game_config.progression` (exp jitter).
     This wave adds an `Atlas::progression()` accessor retaining it — a small,
     grounded in-wave extension to `core/src/data/atlas.rs`. Confirm this is
     acceptable vs. threading the jitter range in as a separate service input.
  3. **Hardcoded combat constants.** The 3% hit floor, 0.3 overrate, `level/10`
     floor, and `1.25` exp factor are not in our `game_config.json` (facts doc 6
     flags them as OpenMU hardcoded logic, and several sit under the W-SRC
     "OpenMU-invented values" debt). *Recommended:* module-level `const`s in
     `services` with provenance comments; flag to `debt-guardian` for a possible
     W-SRC row rather than inventing config fields.
  4. **Excellent scope.** `DropConfig::excellent_roll` exists; excellent *hit*
     damage needs the attacker's excellent chance/bonus from the W-ENT profile.
     *Recommended:* include the excellent drop category and the excellent-hit roll
     (magnitude read from the resolved profile); if W-ENT does not yet surface
     excellent chance stats, gate the excellent-hit branch on a zero-valued stat
     (never fires) rather than fabricating a value — no `Default` synthesized to
     satisfy the type.
  5. **PvM-only boundary.** Facts distinguish PvM/PvP rate stats; this wave is
     PvM. Confirm no player-target attack path is required before W-ENT surfaces
     player-as-target defense stats.

---

## W-MOV — Movement & Flight

- **Status:** NOT STARTED
- **Goal:** Give the simulation its first movement behavior — grounded steps
  validated against the walk grid, a flight-mode toggle, warp/gate arrival, and
  monster wander/chase/leash AI — all as pure services over the already-built
  spatial foundation and the W-ENT entity state.
- **Prerequisites / blocked-by:** **W-ENT** (entities wave) must ship first — it
  provides the runtime character/monster entities that carry position
  (`WorldPos`), heading (`Facing`), traversal mode (`Movement`), current
  `MapNumber`, wings-equipped / combat-lock flags, and per-entity
  action-cooldown timestamps. W-MOV consumes these; it does not define them. Also
  depends on the shipped spatial foundation (Waves A/B: `spatial.rs`, `tile.rs`,
  `movement.rs`, `Atlas` walk-grid loading) and the loaded `Atlas`.
- **Resolves backlog items:** D1, D2, D3, D5, T2, T3 (from
  `docs/debt/DEBT-INDEX.md`).
- **Scope — IN:**
  - **Fixed-point narrowing surface (D2)** in `core/src/components/spatial.rs`:
    `NonZeroFixed` (smart constructor rejecting zero into a new
    `SpatialError::ZeroFixed`), `Fixed::mul(Fixed) -> Fixed`,
    `Fixed::div(NonZeroFixed) -> Fixed`, private
    `round_shift`/`round_div`/`saturate_i64` helpers (widen to `i128`,
    round-nearest ties-away-from-zero, saturating narrow — no `as`, no `unwrap`,
    no `panic`), and an integer magnitude (`DistanceSq::isqrt() -> u64` or
    `Fixed::isqrt`) — exactly the §2.1 contract pinned but not exposed in Wave A.
    Never `f64::sqrt`.
  - **Normalize-to-speed (D2 first consumer, T3 consumer):**
    `WorldVec::normalized_to(speed: Fixed) -> WorldVec` — consumes
    `WorldVec::length_sq` (T3's missing consumer), takes the magnitude via
    integer `isqrt`, divides each component by that magnitude as a `NonZeroFixed`,
    scales by `speed`. Zero-length input folds to a variant (no direction), never
    a fabricated default.
  - **Tiles→world range conversion:** a helper turning `MobBehavior`'s `u8` tile
    ranges into a `Radius` (`tiles * UNITS_PER_TILE`) so
    `view_range`/`move_range`/`attack_range` become world-space `Radius` for
    `WorldPos::within_range`.
  - **Cadence conversion:** `Ticks(u64)` newtype + `DurationMs::in_ticks(TickDuration)
    -> Ticks` in `core/src/components/units.rs`, turning
    `MobBehavior.move_delay_ms` / `attack_delay_ms` (`DurationMs`) into a per-tick
    cadence the AI readiness check uses.
  - **Grounded-step validation (D3)** in a new `core/src/services/movement.rs`:
    given the entity's `Movement`, its map's `&WalkGrid` (via `Atlas::walk_grid`),
    and a proposed destination `WorldPos`, return a step outcome. `Grounded`
    requires `WalkGrid::walkable(dest)` (keyed off `Movement::checks_walkability`);
    `Flying` skips the check. The step vector is `normalized_to(speed)` applied to
    `target − pos`, added via `WorldPos + WorldVec` (which clamps into world
    bounds).
  - **Flight-toggle FSM (D1):** `FlightChange { EnableFlight, DisableFlight }`
    input enum, `apply_flight_change((Movement, FlightChange)) -> (Movement,
    Vec<MovementModeChanged>)` as a total 2×2 exhaustive tuple match; redundant
    intent is an idempotent no-op emitting nothing. The eligibility gate (wings
    equipped, not combat-locked, `MapEnvironment::Sky` forces flight) runs before
    it and emits its own `FlightDenied` outcome — reading the W-ENT character
    entity and `MapDefinition.environment`.
  - **Warp / gate arrival + landing (D5):** a resolution function consuming a
    `Landing` (from `Atlas::enter_gate_at` or `Atlas::warps`), sampling a walkable
    arrival `WorldPos` inside `Landing.area` (`WorldRect`) via injected `RngCore`
    + walk grid, and an explicit `match` on `Landing.facing`: `Some(f)` uses `f`;
    `None` applies a documented arrival-facing policy (recommend: keep prior
    facing) — no `unwrap_or`, no core-fabricated `Facing`.
  - **Monster AI** in a new `core/src/services/monster_ai.rs`: a decision function
    over a monster entity + nearby target that returns an intent (wander / chase /
    leash / attack-in-range / idle) using `MobBehavior.{view_range, move_range,
    attack_range}` (converted to `Radius`), `WorldPos::within_range`, `Facing`,
    and the walk grid for the resulting grounded step. Wander direction is drawn
    from injected `RngCore`; step cadence gated by the `Ticks` conversion.
  - **Events** in `core/src/events/`: `MovementModeChanged`, `FlightDenied`, a
    step-resolved/step-blocked outcome, a warp-arrival outcome, and a
    monster-action outcome — flat `#[serde(tag="kind")]` enums, one variant per
    shape.
- **Scope — OUT (deferred):**
  - Spawn placement + `SoccerPitch` world-space resolution (D4) → **W-ENT**
    (`docs/debt/worldspace-resolution-services.md`).
  - The character/monster runtime **entity types** and their cooldown-timestamp
    storage → **W-ENT** (built first; W-MOV only reads them).
  - Combat damage / attack resolution (the AI emits an "attack" intent; the damage
    math is not here) → **W-CMB**.
  - Proven-present `MapId` handle retiring `Atlas::walk_grid`'s `Option` (T4) →
    **W-ENT**.
  - Real pathfinding (A*/navmesh). AI takes a greedy single step toward its
    target; multi-step obstacle routing is a later wave.
  - Altitude, air combat, anti-air — explicitly excluded by `Movement`'s doc
    contract; `Movement` stays a two-state classifier.
  - `T1` (`narrow_u8` dead arm) — belongs to the next `tile.rs` touch; W-MOV does
    not edit `tile.rs`.
- **Key types / modules to build:**
  - `core/src/components/spatial.rs` (edit): `NonZeroFixed`,
    `Fixed::mul`/`Fixed::div`, private `round_shift`/`round_div`/`saturate_i64`,
    integer `isqrt`, `WorldVec::normalized_to` (uses existing
    `WorldVec::length_sq`, `WorldVec::dot`), new `SpatialError::ZeroFixed`, and
    the tiles→`Radius` helper.
  - `core/src/components/units.rs` (edit): `Ticks(u64)`,
    `DurationMs::in_ticks(TickDuration)`; consumes existing `TickDuration::millis()
    -> NonZeroU32`.
  - `core/src/services/movement.rs` (new): `FlightChange`, `apply_flight_change`,
    grounded-step resolution, warp/landing resolution + arrival-facing policy.
    Consumes `Movement::checks_walkability` (`movement.rs`), `WalkGrid::walkable`
    (`tile.rs`), `Atlas::{walk_grid, enter_gate_at, warps}` + `Landing`
    (`atlas.rs`), `WorldPos`/`WorldVec`/`Facing`/`Radius` (`spatial.rs`),
    `rng::uniform_below_usize` (`rng/mod.rs`).
  - `core/src/services/monster_ai.rs` (new): AI intent enum + decision function.
    Consumes `MobBehavior` (`monster_definitions.rs`), `WorldPos::within_range`,
    the tiles→`Radius` helper, `Ticks`, `rng`.
  - `core/src/events/mod.rs` (edit): `MovementModeChanged`, `FlightDenied`, step
    outcome, warp-arrival outcome, monster-action outcome.
  - `core/src/services/mod.rs` + `core/src/events/mod.rs` module wiring.
- **Read first (fresh session):** `docs/debt/movement-flight-wave.md`
  (D1/D2/D3/D5 records), `docs/debt/spatial-foundation-followups.md` (T2/T3),
  `docs/specs/2026-07-03-spatial-foundation.md` (§2.1 narrowing contract, §8
  flight-FSM contract),
  `core/src/components/{spatial,tile,movement,units}.rs`,
  `core/src/data/{atlas,gates_warps,monster_definitions,map_definitions}.rs`,
  `core/src/rng/mod.rs`, `core/src/services/chance.rs` (RNG-consuming service
  pattern), `core/src/events/mod.rs`, `core/src/entities/mod.rs` (whatever W-ENT
  populated). (CLAUDE.md + README.md always implied.)
- **Pipeline:** Core Domain Feature — `bdd-tdd-spec-writer` →
  `core-architecture-guardian` (plan) → `canon-guardian` (plan) →
  `state-machine-agent` → implementation → `core-architecture-guardian` (code) →
  `deep-module-guardian` → `canon-guardian` (code) → `debt-guardian` →
  `rules-guardian`.
- **Definition of done:** four green gates plus:
  - `Fixed::mul`/`div` round-nearest-ties-away and saturate on overflow, proven by
    unit tests including the `TILE_SHIFT` scaling identity and a
    `NonZeroFixed::new(0)` rejection; determinism/bit-identity holds (integer-only,
    no float).
  - `WorldVec::normalized_to(speed)` produces a vector whose `length_sq` is within
    one sub-unit-squared of `speed²` across a sampled grid, and folds the zero
    vector to the no-direction variant (test).
  - `apply_flight_change` covers all four `(Movement, FlightChange)` tuples
    exhaustively (no wildcard); redundant intent emits no event; the eligibility
    gate emits `FlightDenied` for no-wings / combat-locked and forces flight on
    `MapEnvironment::Sky` (tests).
  - Grounded step onto a blocked tile is rejected; a `Flying` entity crosses the
    same tile; keyed off `Movement::checks_walkability` — proven against a real
    `Atlas::walk_grid` in `core/tests/`.
  - Warp/gate arrival lands on a walkable `WorldPos` inside `Landing.area`, and
    `Landing.facing == None` resolves via the explicit policy match (test asserts
    prior facing preserved, no fabricated default).
  - Monster AI: within `view_range` chases, beyond leash returns toward anchor,
    within `attack_range` emits attack intent, otherwise wanders — deterministic
    under a fixed RNG seed (tests).
  - T3 discharged: `WorldVec::length_sq` now has a live consumer (`normalized_to`).
    T2 discharged: `TileArea::contains` is confirmed consumer-less (AI leashes in
    world space) and trimmed. D1/D2/D3/D5 rows removed from `DEBT-INDEX.md`;
    `movement-flight-wave.md` closed.
- **Open decisions / risks:**
  - **Absolute-tick type ownership.** AI cadence needs an absolute `Tick(u64)`
    (current sim tick) to compare against an entity's next-action tick. Recommend
    W-ENT owns `Tick(u64)` (entity timestamps) and W-MOV owns the `Ticks(u64)`
    duration + `in_ticks` conversion; if W-ENT did not define `Tick`, W-MOV adds
    it to `components/units.rs`.
  - **Walkable-tile sampler sharing.** Warp-landing sampling ("random walkable
    `WorldPos` in a `WorldRect`") is the same primitive D4's spawn-area sampler
    needs. Recommend reusing the W-ENT sampler if it shipped one; otherwise W-MOV
    introduces `sample_walkable_in(rect, grid, rng)` designed for both callers
    (deepen, don't duplicate).
  - **Flight eligibility surface.** The gate reads wings-equipped + combat-lock
    off the W-ENT character entity. Recommend the service takes those as an
    explicit eligibility input value the entity exposes, keeping
    `apply_flight_change` a pure `(Movement, FlightChange)` transition and the gate
    a separate function — matching the §8 contract (gate runs before, emits its
    own denial).
  - **Arrival-facing policy for `Landing.facing == None`.** Recommend "keep the
    traveler's prior facing." The alternative (face away from the gate) needs the
    gate's geometry and is not justified by current data; pick keep-prior unless a
    scenario demands otherwise.
  - **AI stepping vs pathfinding.** Greedy single-step-toward-target can wedge
    against concave walls. Accepted for this wave (leash pulls the monster back);
    real pathfinding is explicitly out of scope — flag so a reviewer does not read
    the greedy step as an incomplete pathfinder.
  - **`normalized_to` at very small magnitudes.** Sub-tile step vectors lose
    precision through `isqrt`; the 16 fractional bits (Q40.24) give ~1/65536-tile
    grain, ample for gameplay speeds — verify the chosen monster/character speeds
    stay well above the quantization floor in tests.

---

## W-HOST — First Host Adapter Slice (Native Headless Harness): the hexagon litmus test

- **Status:** NOT STARTED
- **Goal:** Prove `mu-core` is genuinely host-agnostic by building the first real
  adapter — a native, dependency-light binary that loads static data, drives one
  stateful core service over scripted input with an injected seeded RNG, persists
  returned entity state in-memory, and delivers returned events to stdout — with
  every game rule staying in core and none leaking into the host.
- **Prerequisites / blocked-by:**
  - **W-ENT** (character/monster entity aggregates) — the host has nothing to
    persist until entity state types exist. Today `core/src/entities/mod.rs` is a
    doc-comment-only module (no types), so there is no state to store.
  - **W-MOV** (first stateful, event-producing service) — the litmus test requires
    a service with the canonical `(state, input, &mut impl RngCore) -> (new state,
    Vec<Event>)` shape. Today `core/src/services/` holds only `chance.rs` (pure
    RNG rolls) and `item_rules.rs` (pure const-table lookups); neither takes entity
    state nor returns events, so there is nothing for a delivery adapter to
    deliver. W-MOV (movement/flight service, debt D1/D3) is the concrete first
    behavior that yields an entity-state-in / events-out call the harness can
    drive.
  - **Events module populated** — `core/src/events/mod.rs` is doc-comment-only
    today; W-MOV must land at least one outcome enum there (e.g. a movement
    outcome) for the delivery adapter to have a typed value to route.
  - **Atlas + StaticData port** (`core/src/data/atlas.rs`) — **already exists**;
    the data-loading port is ready now.
- **Resolves backlog items:** none. (This wave opens the `hosts/` track; it does
  not close any current D#/T#/Q# in `docs/debt/DEBT-INDEX.md`. It will likely
  retire the placeholder `hosts/README.md`'s "server/" bullet as the first
  realized crate.)
- **Scope — IN:**
  - A new workspace member `hosts/harness/` (native binary crate embedding
    `mu-core`), registered in the root `Cargo.toml` `members` list.
  - **codec / data-loading adapter** — reads the real `/data/*.json` files and the
    11 `data/terrain/<map>.bin` sidecars from disk, deserializes each into
    `mu_core::data::common::DataFile<T>` (and `MapTerrain`), fills every field of
    `mu_core::data::atlas::StaticData`, and calls `Atlas::parse` once.
    Parse-don't-validate: raw bytes become the resolved `Atlas` exactly once at
    startup; `AtlasError` is surfaced as a host-level startup failure (printed,
    non-zero exit), never a panic in core.
  - **codec / input adapter** — parses a scripted input source (a line-oriented
    command file or stdin) into the domain input type(s) that the W-MOV service
    accepts. One `parse_*(&str) -> Result<DomainInput, HostParseError>` per
    command; malformed lines are rejected at the boundary, never forwarded.
  - **persistence adapter** — an in-memory store (plain
    `std::collections::HashMap` keyed by the domain newtype id, e.g. the character
    id from W-ENT) holding the entity aggregates between service calls. No DB, no
    engine, no ORM — proving persistence is a swappable adapter concern.
  - **handlers** — the drive loop: for each parsed input, read current entity
    state from the store, call the core service with a host-owned `&mut impl
    RngCore`, write the returned new state back to the store, and hand the returned
    events to delivery. No domain decision inline.
  - **delivery adapter** — renders each returned event (from `core/src/events/`)
    to stdout as a stable text line. This is the "log" adapter; it owns
    formatting/envelope, core owns the event value.
  - **Host-owned RNG seam** — the binary constructs a concrete seeded `RngCore`
    implementer (a real crate dependency, allowed because this is a host, not
    core) from a CLI-supplied `--seed`, and threads `&mut` it through every service
    call. Proves portability rule 5 end-to-end from the host side.
  - **Determinism acceptance test** — an integration test (or a checked-in golden
    transcript) proving: same seed + same scripted input ⇒ byte-identical event
    output. Run it twice; diff must be empty.
- **Scope — OUT (deferred):**
  - **SpacetimeDB module host** (`hosts/server` real reducers/tables) — a later
    host wave once the harness proves the seam; reuses this wave's
    codec/persistence/delivery split against real tables.
  - **Browser wasm host** (`hosts/wasm`) — later host wave; this wave keeps
    `cargo check -p mu-core --target wasm32-unknown-unknown` green but does not
    build a wasm host.
  - **Unity FFI host** (`hosts/ffi`, C ABI, the crate-wide `unsafe` exception) —
    later host wave.
  - **Networking / transport framing, wire versioning, event envelopes** — belongs
    to the SpacetimeDB/wasm host waves; the harness's delivery is plain stdout
    text.
  - **Combat / drop / leveling services** — those are their own core-behavior
    waves; the harness drives whatever stateful services exist when it lands
    (W-MOV first), and gains more command handlers as later services ship.
  - **Any new game rule** — if the harness needs a decision, that decision is a
    missing core service, not host code (hard rule).
- **Key types / modules to build (grounded in what exists):**
  - `hosts/harness/Cargo.toml` — depends on `mu-core` (path), `serde_json`
    (deserialize `DataFile<T>` exactly as `core/tests/data_files.rs:80` already
    does), and one concrete RNG crate implementing `rand_core = "0.9"`'s `RngCore`
    (see open decisions). `mu-core` itself gains **zero** new dependencies.
  - `hosts/harness/src/codec.rs` — `fn load_static_data(dir: &Path) ->
    Result<StaticData, HostLoadError>` filling every field of
    `mu_core::data::atlas::StaticData` (`maps`, `gates_warps`, `monsters`,
    `spawns`, `skills`, `items`, `box_drops`, `special_drops`, `ancient_sets`,
    `chaos_mixes`, `classes`, `exp_tables`, `game_config`, `terrain`) from
    `/data`; then `Atlas::parse(data) -> Result<Atlas, AtlasError>`. Mirror the
    exact record types imported in `core/tests/data_files.rs` (`MapDefinition`,
    `GateWarpRecord`, `MonsterDefinition`, `Spawn`, `Skill`, `ItemDefinition`,
    `BoxDrop`, `SpecialDropRecord`, `AncientSet`, `ChaosMix`, `ClassRecord`,
    `ExpTable`, `GameConfig`, `MapTerrain`).
  - `hosts/harness/src/persistence.rs` — `struct World { characters:
    HashMap<CharacterId, Character>, .. }` over the W-ENT entity aggregate type(s)
    from `mu_core::entities`. Reads/writes only; unaware of transport and
    decisions.
  - `hosts/harness/src/handlers.rs` — the per-command drive functions: read state
    → call the `mu_core::services` W-MOV function with `&Atlas`, the entity, the
    parsed input, and `&mut rng` → write back → return events.
  - `hosts/harness/src/delivery.rs` — `fn render(event: &MovementEvent) -> String`
    (or `Display`-based) over the W-MOV outcome enum in `mu_core::events`,
    exhaustive `match`, one arm per variant.
  - `hosts/harness/src/main.rs` — parse `--seed` and input path, wire codec →
    persistence → handlers → delivery, non-zero exit on `HostLoadError`/`AtlasError`.
  - `hosts/harness/tests/determinism.rs` — drives a fixed script under a fixed seed
    twice; asserts identical event transcripts (satisfies Q1-style drift-pinning at
    the host boundary).
- **Read first (fresh session):** (CLAUDE.md + README.md always implied)
  - `README.md` — the six portability rules and the four host targets.
  - `CLAUDE.md` — the "Host Adapter Shape" section (handlers / codec / persistence
    / delivery) and Iron Law 1 (Dependency Rule, litmus test).
  - `core/src/data/atlas.rs` — the `StaticData` port (every field the codec must
    fill) and `Atlas::parse` / `AtlasError`.
  - `core/src/data/common.rs` — `DataFile<T>` deserialize shape.
  - `core/tests/data_files.rs` — the exact working recipe for loading every `/data`
    file and the terrain sidecars into `StaticData` (the codec is the production
    version of this test harness).
  - `hosts/README.md` — the placeholder plan for `server/`, `wasm/`, `ffi/`.
  - `core/src/entities/mod.rs` and `core/src/events/mod.rs` — to see the
    W-ENT/W-MOV types the persistence and delivery adapters consume (these must be
    non-empty by the time this wave runs).
  - `core/src/rng/mod.rs` — the injected-randomness seam the host feeds.
- **Pipeline:** Refactor/Cleanup pipeline (this wave writes host adapter code, not
  new core rules): `core-architecture-guardian` (code — verify no rule leaked into
  the host and core stayed untouched) → `deep-module-guardian` → `canon-guardian`
  (code — verify the ports-and-adapters split is textbook, not a custom shape) →
  `debt-guardian` → `rules-guardian`. Run `core-architecture-guardian` (plan) +
  `canon-guardian` (plan) up front to confirm the adapter boundary before writing
  code.
- **Definition of done:** Four green gates (clippy now spans the new
  `hosts/harness` member) plus wave-specific criteria:
  - `git diff --stat` shows **zero** changes under `core/` (litmus, mechanical
    form): the entire host was added without editing a single line of core or its
    public API.
  - `mu-core`'s dependency set is still exactly `serde` + `rand_core` (host deps
    like `serde_json`/the RNG crate live only in `hosts/harness/Cargo.toml`).
  - Running the harness on a scripted input with a fixed `--seed` produces a
    deterministic event transcript; a second run with the same seed is
    byte-identical, and a different seed can differ — the determinism test asserts
    this.
  - A malformed static-data file or a dangling cross-reference produces a printed
    `AtlasError`/`HostLoadError` and a non-zero exit — never a panic (no
    `unwrap`/`expect` in the host's happy or error path either; the host obeys the
    same suppressor bans).
  - Every `delivery.rs` render is an exhaustive `match` over the event enum with no
    wildcard arm — a new event variant breaks the host build.
  - **Stated litmus test (must be answered "yes" in the wave's closing note):**
    *Could you delete `hosts/harness` and drop in a SpacetimeDB module (reducers +
    tables) or a wasm host — reusing the identical `mu-core` crate, the identical
    `Atlas`/`StaticData` port, and the identical service calls — with core and its
    public API untouched?* If any game decision, event-formatting rule, or
    state-transition lives in the harness rather than core, the answer is "no" and
    the wave is not done.
- **Open decisions / risks:**
  - **Which stateful service does the harness drive first?** Recommended default:
    the **W-MOV movement service** (debt D1/D3) — it is the next scheduled behavior
    wave, has clear entity-state-in / events-out shape, and needs no combat
    modeling. If a combat service lands before W-MOV, drive that instead; the
    adapter shape is identical.
  - **Concrete RNG crate for the host.** Recommended default: `rand_pcg` (or
    `rand_chacha`) pinned to a `rand_core 0.9`-compatible release, seeded from
    `--seed`. Risk: version-mismatch with core's `rand_core = "0.9"` — verify the
    chosen crate re-exports the same `RngCore` 0.9 trait before committing. This
    choice is host-local and swappable; it does not touch core.
  - **First target justification (native headless harness vs SpacetimeDB/Unity).**
    Recommended and assumed default: the **native headless harness**. It is the
    smallest artifact that exercises all four adapter sub-shapes
    (codec/persistence/delivery/handlers) and both the data-loading port and the
    RNG port, with zero DB/engine/network/FFI surface — so a failure is
    unambiguously a *core leak*, not adapter-infrastructure noise. SpacetimeDB
    (async reducers, real tables, module ABI) and Unity FFI (C ABI, the crate's one
    `unsafe` exception) each add large host-specific surface that would mask the
    litmus signal; they become their own host waves once the harness proves the
    seam is clean.
  - **Input source format** (line-oriented command file vs stdin vs a tiny scripted
    DSL). Recommended default: a checked-in line-oriented command file passed by
    path, one command per line — makes the golden-transcript determinism test
    trivial and reviewable.
  - **Persistence key type.** Depends on the entity id newtype W-ENT mints (e.g.
    `CharacterId`); the `HashMap` key must be that exact domain newtype, not a
    host-invented id — confirm against `mu_core::entities` when the wave runs.

---

## W-HARDEN — Tooling & CI Hardening (Q1–Q4)

- **Status:** NOT STARTED
- **Goal:** Give the four review-only / cross-target guarantees (wire stability,
  invariant coverage, ban enforcement, bit-for-bit determinism) the same
  build-failing, mechanical enforcement the clippy-covered rules already have —
  while the surface is small enough to pin cheaply.
- **Prerequisites / blocked-by:** none. All four items are marked schedulable-now
  in `docs/debt/practices-transfer-quality.md`; nothing is blocked by a domain
  wave. **Schedulable anytime** — good work to run alongside W-ENT.
- **Resolves backlog items:** Q1, Q2, Q3, Q4 (from `docs/debt/DEBT-INDEX.md`).
  Closing all four discharges the `practices-transfer-quality.md` record; remove
  all four rows from `DEBT-INDEX.md`.
- **Scope — IN:**
  - **Q1 — serialized-shape drift-pins.** Exact-JSON assertions for every
    host-facing wire type (internally-tagged `kind` enums and
    `#[serde(into/try_from)]` newtypes), so a rename/reorder/tag change is a red
    test, not a silent wire break. Extends the existing style already in the repo:
    the exact-string asserts in `core/src/components/spatial.rs` (e.g.
    `region_containment_and_wire`, line ~945; `cone_wire_round_trip`, line ~880)
    and the `serialize_identity_is_stable` test (line 950). Consolidate into one
    dedicated integration test file, `core/tests/wire_drift.rs`, that pins one
    canonical value per type.
  - **Q2 — expanded proptest invariants.** New `proptest!` blocks covering the
    non-spatial invariants currently unit-tested only:
    - `services/chance.rs` `WeightedTable` / `weighted_pick`: `total()` equals the
      exact weight sum; every constructed table's pick lands in a real bucket for
      all rolls `0..total`; a higher-weight bucket is selected at least as often as
      a lower over a large sample (monotonicity, not exact distribution).
    - `services/chance.rs` `pick_one` over `components/collections.rs::OneOrMore`:
      result is always an element of the list; index coverage across the list
      length.
    - `rng/mod.rs` `uniform_below` / `uniform_below_usize`: output always `<
      bound`; no-modulo-bias (equidistribution) check.
    - Newtype wire round-trips across full valid ranges: `Level`, `Zen`, `Exp`
      (`components/units.rs`), `Interval` (`components/interval.rs`), `Radius` /
      `Fixed` (`components/spatial.rs`) — `from_str(to_string(v)) == v` for all
      valid `v`, and out-of-range inputs are rejected.
  - **Q3 — AST ban scanner + hooks.** A new `xtask` binary crate (`xtask/`) added
    to `[workspace] members`, using `syn` + `walkdir` to parse every
    `core/src/**/*.rs` and flag the four **review-enforced** bans that no clippy
    lint covers (enumerated in `CLAUDE.md` Iron Law 3):
    1. `.unwrap_or(..)` / `.unwrap_or_default()` whose receiver is a lookup-shaped
       call (`.get(..)`, `.first()`, `.last()`, `.find(..)`, `.iter().find(..)`).
    2. Inline `#[expect(..)]` attributes anywhere in `core/`.
    3. `#[non_exhaustive]` on any `enum`.
    4. Fabricated `Default`/zeroed values used to satisfy a signature — scoped
       concretely to a hand-derivable rule (see Open decisions) to avoid false
       positives.
    Non-zero exit with file:line on any hit. Wired into (a) a git pre-commit hook
    and (b) a new CI step.
  - **Q4 — OS matrix + wasm test-run.** Rewrite `.github/workflows/ci.yml` to a
    `strategy.matrix.os = [ubuntu-latest, macos-latest, windows-latest]` job, plus
    a separate leg that *runs* the test suite under wasm (not just `cargo check`)
    via `wasm32-wasip1` + `wasmtime` as the cargo target runner, proving
    cross-target execution and determinism.
- **Scope — OUT (deferred):**
  - FFI-target determinism execution (the third leg of the "native/wasm/FFI"
    guarantee) — deferred until the FFI host crate exists under `hosts/` (no such
    crate today; `hosts/` holds placeholders only).
  - Browser `wasm32-unknown-unknown` *test execution* via `wasm-bindgen-test` +
    headless browser — deferred; core has no wasm-bindgen surface, and
    `wasm32-wasip1`+wasmtime exercises the identical pure-integer code paths far
    more cheaply. Keep the existing `wasm32-unknown-unknown` `cargo check` as the
    browser-target *compile* gate.
  - Extending the Q3 scanner to host crates — deferred to the wave that introduces
    the first host crate.
  - Statistical goodness-of-fit (chi-square) testing of the RNG — out; Q2 uses
    deterministic coverage/equidistribution bounds, not flaky sampling.
- **Key types / modules to build:**
  - **Q1:** `core/tests/wire_drift.rs` (new integration test). Pins canonical JSON
    for: `Region` (all three `kind` variants — `circle`/`rect`/`cone`), `WorldPos`,
    `WorldVec`, `Facing`, `ConeHalfWidth`, `WorldRect`, `Radius`, `Fixed` (all in
    `components/spatial.rs`); the kind-tagged data enums `SpecialDrop`
    (`data/special_drops.rs`, `#[serde(tag=...)]` — grep `tag = "kind"` to
    enumerate the full set); `CharacterClass` (`components/class.rs`); `ItemRarity`
    (`components/item_quality.rs`); `EnhanceLevel`/`AmmoLevel`
    (`components/levels.rs`); `Level`/`Zen`/`Exp` (`components/units.rs`);
    `Interval` (`components/interval.rs`); `TileCoord` (`components/tile.rs`). One
    `assert_eq!(serde_json::to_string(&value).unwrap(), r#"..."#)` per type.
  - **Q2:** new `proptest!` blocks added inside the existing `#[cfg(test)] mod
    tests` of `core/src/services/chance.rs` and `core/src/rng/mod.rs`, and a `mod
    tests` in `core/src/components/units.rs`. Reuse the `TestRng` (`SplitMix64`)
    already defined in `chance.rs` tests, or a `proptest`-driven seed feeding it,
    for the RNG-consuming properties. No new lib types — proptest is already a
    `[dev-dependencies]` in `core/Cargo.toml`.
  - **Q3:** new crate `xtask/` with `xtask/Cargo.toml` (`[dependencies] syn = {
    version = "2", features = ["full","visit"] }`, `walkdir`, `quote`/`proc-macro2`
    as needed) and `xtask/src/main.rs` (a `syn::visit::Visit` implementation per
    ban). **Does not** carry `[lints] workspace = true` (it is a dev tool, not core
    — it may use `unwrap`/`expect` freely and must not inherit the Iron-Law lints).
    `.cargo/config.toml` gains `[alias] xtask = "run --package xtask --"`. Hook
    script `scripts/pre-commit` (or `.githooks/pre-commit`) runs `cargo xtask
    scan`; installed via `git config core.hooksPath .githooks` (currently unset —
    verified) documented in `README.md` under Development.
  - **Q4:** `.github/workflows/ci.yml` (rewrite). Add `dtolnay/rust-toolchain` with
    `targets: wasm32-wasip1` and a `wasmtime` install step (e.g.
    `bytecodealliance/actions` or `cargo install wasmtime-cli`), plus
    `CARGO_TARGET_WASM32_WASIP1_RUNNER: wasmtime` env, then `cargo test -p mu-core
    --target wasm32-wasip1`.
- **Read first (fresh session):** `CLAUDE.md` and `README.md` (always);
  `docs/debt/practices-transfer-quality.md` (the Q1–Q4 spec);
  `docs/debt/DEBT-INDEX.md` (rows to remove on close); `.github/workflows/ci.yml`;
  `clippy.toml` and root `Cargo.toml` (`[workspace.lints]`) — to see which bans are
  *already* lint-covered so Q3 does not duplicate them; `core/Cargo.toml` (proptest
  is already a dev-dep); `core/src/components/spatial.rs` (the existing drift-pin
  and proptest patterns to extend); `core/src/services/chance.rs` and
  `core/src/rng/mod.rs` (Q2 targets, incl. the reusable `TestRng`);
  `core/tests/data_files.rs` (integration-test + macro conventions).
- **Pipeline:** This is tooling/tests, not a core domain feature, and touches no
  domain logic or types. Run the **Refactor/Cleanup** pipeline:
  `core-architecture-guardian` → `deep-module-guardian` → `canon-guardian` →
  `debt-guardian` → `rules-guardian`. Notes for the guardians: `canon-guardian`
  should verify the Lemire/rejection reasoning in the Q2 RNG property and the
  standard `xtask` pattern for Q3; `core-architecture-guardian` should confirm the
  `xtask` crate stays outside core's dependency graph (mu-core's `[dependencies]`
  remain exactly `serde` + `rand_core` — `syn`/`walkdir` live only in `xtask`,
  never reachable from the lib target).
- **Definition of done:** Four green gates PLUS:
  - **Q1:** `cargo test --test wire_drift` passes; deliberately renaming a `kind`
    tag or reordering a field makes exactly that pin fail (spot-verify once during
    development, then revert).
  - **Q2:** the new proptest cases run under `cargo test --workspace` and hold at
    default case counts; `uniform_below` output is proven `< bound` and
    equidistributed within the stated deterministic bound.
  - **Q3:** `cargo xtask scan` exits 0 on the current clean tree; injecting each of
    the four banned patterns into a throwaway file makes it exit non-zero with
    file:line (verify all four, then revert); the pre-commit hook blocks a commit
    containing a violation; the same command runs as a CI step.
  - **Q4:** CI is green on ubuntu **and** macOS **and** windows; the
    `wasm32-wasip1` leg actually executes `cargo test` under wasmtime (job log
    shows tests *run*, not just compiled) and passes — proving native and wasm
    produce identical results.
  - `DEBT-INDEX.md` no longer lists Q1–Q4 and the `practices-transfer-quality.md`
    record is closed.
- **Open decisions / risks:**
  - **Q3 scope of the "fabricated Default" rule.** A fully general "is this
    `Default` fabricated to satisfy a signature?" detector is undecidable and will
    false-positive. **Recommended default:** scope it to a mechanically-decidable
    pattern — flag `Default::default()` / `T::default()` / `..Default::default()`
    and `#[derive(Default)]` appearing on domain enums/structs in `core/src`, and
    let a curated allowlist (or a doc-comment opt-in marker) exempt the
    genuinely-legitimate uses. Start narrow (zero false positives on today's tree)
    and widen only with evidence.
  - **Q3 crate placement.** `xtask` as a workspace member (canonical Rust pattern,
    gets `cargo xtask` alias, shares the lockfile) vs. a standalone out-of-workspace
    crate. **Recommended default:** workspace member — but it must **omit** `[lints]
    workspace = true` so it does not inherit core's Iron-Law lints, and
    `core-architecture-guardian` must confirm it never enters mu-core's dependency
    graph.
  - **Q4 proptest under `wasm32-wasip1`.** `proptest` (and `serde_json`) must
    compile and run under wasi/wasmtime; `proptest`'s optional `fork`/timeout
    features are off by default and should not be enabled. **Risk:** if a
    dev-dependency drags in a non-wasi-buildable transitive crate, the wasm
    test-run leg won't build. **Fallback:** gate the wasm-executed leg to a
    dedicated determinism integration test that avoids proptest (plain `#[test]`
    fns asserting fixed seed → fixed output), keeping proptest to the native legs.
    Decide only if the full `cargo test --target wasm32-wasip1` fails to build.
  - **Q4 runner choice.** `wasm32-wasip1` + `wasmtime` (recommended — trivial for a
    pure-compute crate, truly executes tests) vs. `wasm32-unknown-unknown` +
    `wasm-bindgen-test` + node/headless-chrome (heavier, matches the browser wire
    target exactly). Recommend wasip1 for execution and retain the existing
    unknown-unknown `cargo check` as the browser compile gate; they cover different
    things and both stay.
  - **Hook install friction.** `core.hooksPath` is currently unset. Committing
    hooks under `.githooks/` and documenting the one-time `git config
    core.hooksPath .githooks` in `README.md` is the least-magic option; CI is the
    real gate, the hook is a fast local pre-flight. Recommend documenting, not
    auto-installing.

---

## Long-Term Tracks — W-SRC (data provenance) + broader behavioral backlog

Two independent tracks. **Part A** is a single, self-contained data-provenance
wave (`W-SRC`) — data-only, touches zero Rust. **Part B** is the
behavioral-systems backlog: each system is its own future wave, none of which
`W-SRC` blocks or is blocked by.

### Part A — W-SRC — Re-source the 185 OpenMU-invented data values against authentic classic files

- **Status:** NOT STARTED
- **Goal:** Every `review` flag in the v2 dataset is discharged — each
  OpenMU-default / era-doubtful value is either confirmed against an authentic
  classic 0.75 / 0.95d source (flag deleted) or corrected (value re-extracted), so
  no shipped number carries silent, unverified OpenMU provenance.
- **Prerequisites / blocked-by:** Authentic classic client data files or documented
  GS config for the pre-Season-3 baseline (`Monster.txt`, `Skill.txt`,
  `Item*.txt`, `Gate.txt`, and the exp/drop GS config for 0.75 and 0.95d),
  obtained **independent of OpenMU**. No code wave blocks this; it consumes only
  the existing extractor scripts and JSON. Each of the 18 value families (below)
  can be sourced and closed on its own — resolving one never unblocks another.
- **Resolves backlog items:** `W-SRC` (the `docs/debt/DEBT-INDEX.md` row and its
  record `docs/debt/openmu-default-values.md`).
- **Scope — IN:**
  - The 185 `review` strings across the 13 v2 files (2388 records), grouped as 18
    value families A–L in `docs/debt/openmu-default-values.md`: character-creation
    defaults (`classes.py`), item modeling + S6 backports (`items.py`), monster
    combat modeling (`monsters.py`), skills (`skills-effects.py`), maps + warps
    (`maps.py`), ancient-set ordering (`options-sets.py`), special + box drops
    (`drops.py`), chaos crafting economics (`chaos.py`), exp curve + game-config
    defaults (`constants-exp.py`).
  - Per family: locate an authentic source, then **either** delete the `review`
    string at the owning extractor and re-run it (confirmed), **or** correct the
    value at the extractor and re-run (differs), **or** explicitly record the value
    as an accepted-default-with-standing-flag in `openmu-default-values.md`
    (genuinely unsourceable).
  - After every edit: re-run `tools/extract/validate_refs.py` and `cargo test
    --test data_files` to prove the dataset still parses and cross-resolves through
    `Atlas::parse` (`core/src/data/atlas.rs`).
- **Scope — OUT (deferred):**
  - Any change to a Rust type, service, or the record *shape* of a file — this wave
    edits only values and the `review` field (`Provenance.review: Option<String>`
    in `core/src/data/common.rs`, read by no service). A shape change is a schema
    wave, not this one.
  - The four S6-second-tier / S6-backport families (A-4, B-6, B-7, D-11 in the
    record) *if* the pre-S3 era scope excludes that content — those get
    accepted-and-recorded, not fabricated (see Open decisions).
  - Any behavioral consumer of the re-sourced data (drops, crafting, combat) —
    Part B waves.
- **Key types / modules to build:** None — this wave writes no Rust. It edits the
  **extractor scripts** `tools/extract/{classes,items,monsters,skills-effects,maps,
  options-sets,drops,chaos,constants-exp}.py` and their emitted `data/*.json`, then
  re-runs `tools/extract/validate_refs.py`. The only Rust type in the loop is the
  existing `Provenance` (with its `review: Option<String>`) in
  `core/src/data/common.rs`; the values flow into the already-built record structs
  (`MonsterDefinition`, `ItemDefinition`, `Skill`, `ClassRecord`, `AncientSet`,
  `SpecialDropRecord`, `ChaosMix`, `MapDefinition`, `GateWarpRecord`, `ExpTable`,
  `GameConfig`, `BoxDrop`) unchanged.
- **Read first (fresh session):** `docs/debt/openmu-default-values.md` (the
  family-by-family resolution plan and per-file counts), `docs/openmu-reference.md`
  (the extract-WHAT-never-HOW protocol — authentic values only, launder nothing),
  `tools/extract/` (the scripts, especially `common.py` and `validate_refs.py`),
  `core/src/data/common.rs` (the `Provenance`/`review` shape), and the specific
  `data/*.json` + owning `.py` for the family being sourced.
- **Pipeline:** Data-only — the CLAUDE.md agent pipelines are code pipelines and do
  not apply to value edits. Run per family: source → edit extractor → re-run
  extractor → `validate_refs.py` → `cargo test --test data_files`. `rules-guardian`
  runs last as the final audit that no flag was dropped without either a confirming
  source or an accepted-default record (the record's own discharge criterion).
  `canon-guardian` may audit a family for authenticity-of-source where the classic
  value is contested.
- **Definition of done:** four green gates — all green. **Plus:**
  `tools/extract/validate_refs.py` passes and `cargo test --test data_files` passes
  after the final extractor run; every one of the 185 `review` strings is either
  **removed** (confirmed/corrected against a cited authentic source) or
  **explicitly retained-and-recorded** as an accepted default in
  `openmu-default-values.md` with its decision rationale; the DEBT-INDEX `W-SRC`
  row is closed.
- **Open decisions / risks:**
  - **Era scope for S6 content (families A-4, B-6, B-7, D-11 — ~90 of the 185
    flags).** If second-tier classes, S6 base items, second wings, and
    S6-backported skills are out of the pre-S3 baseline, they cannot be sourced
    from 075/095d files. *Recommended default:* do not fabricate — mark each
    accepted-default-with-standing-flag and record the era decision, rather than
    invent a classic value.
  - **Source availability per family.** Some families (e.g. F-13 0.95d warp
    fee/level table, K-18 per-era exp caps) may have no surviving authentic table.
    *Recommended default:* a shipped default with a *standing, recorded* flag is
    acceptable per the record; a shipped default with *no* flag is not — never
    delete a flag without a citation.
  - **Kantata data-bug (family G-14).** OpenMU's own fix (excellent-damage-chance
    `10.0 → 0.10`) must be verified against a classic `SetItemOption` source, not
    trusted transitively.

### Part B — Broader behavioral systems (future waves)

Each row is a distinct future wave with its own scope. The shared prerequisite is
a **character/inventory entity** — the `core/src/entities/` module is currently
empty, and world-space entry needs the Atlas-minted map/world handles that
**W-ENT** owns (DEBT-INDEX `D4`/`T4`). RNG is already injectable
(`core/src/services/chance.rs`).

| System | Proposed wave | Prerequisite wave | Data status |
|---|---|---|---|
| Parties / trade | `W-PARTY` | `W-ENT` (character entity + inventory/zen) | No static data needed — pure entity + transition logic; nothing exists yet in `entities/`. |
| Chaos-machine crafting | `W-CRAFT` | `W-ENT` (inventory to place ingredients) | **Data ready.** `ChaosRecipe` enum fully models 11 recipe families in `core/src/data/chaos_mixes.rs`, resolved through `Atlas`. Missing only the crafting **service** (place-ingredients → match recipe → roll → result/consume). |
| Devil Square (event dungeon) | `W-DS` | `W-ENT`; ticket via `W-CRAFT` | **Partial.** Map 9 exists in `data/map_definitions.json`; `DevilSquareTicket` recipe in `chaos_mixes.rs`. Missing: monster-wave spawn schedule, entry gating, round timers. |
| Blood Castle (event dungeon) | `W-BC` | `W-ENT`; ticket via `W-CRAFT` | **Partial.** `BloodCastleTicket` recipe in `chaos_mixes.rs`. Missing: **no map record** (map 11 absent) — needs map + spawn extraction; plus bridge/statue/quest-objective logic. |
| Chaos Castle (event dungeon) | `W-CC` | `W-ENT` | **Greenfield.** No map (18 absent), no ticket recipe, no spawns. Needs full data extraction (map, entry item, monster set, shrink-arena rules) before any service. |
| NPC shops | `W-SHOP` | `W-ENT` (inventory + zen) | **Partial.** `ItemDefinition` carries a `price` field. Missing: per-NPC shop stock lists — no shop-inventory data extracted. Needs a shop-catalog data file + buy/sell service. |

Notes on the two "data-ready-ish" cases:

- **Chaos-machine crafting (`W-CRAFT`)** is the highest-leverage next behavioral
  wave: the entire recipe catalog is already modeled and Atlas-resolved; only the
  pure crafting service is absent. Its economics values are among the `W-SRC` flags
  (family J), but re-sourcing those is data-only and does not block building the
  service.
- **Devil Square (`W-DS`)** is the only event dungeon whose *map* already ships
  (map 9). Blood Castle and Chaos Castle both need map/spawn extraction first.

---

## Status Log

Append one row per meaningful event (wave started/landed, debt row closed,
decision pinned). Most recent at the bottom.

| Date | Wave | Event |
|---|---|---|
| 2026-07-03 | (foundation) | Spatial foundation Waves A + B landed on branch `spatial-foundation` (commits `61f1e37`, `5ed218e`): world-space `Fixed` Q40.24, `WorldPos`/`WorldVec`/`Facing`/`Region`, tiles + `WalkGrid`, two-state `Movement`, world-space Atlas + terrain walk-grids. clippy `-D warnings` clean, ~120 tests, wasm check OK. Deferrals recorded in DEBT-INDEX (D1–D5, T1–T4). |
