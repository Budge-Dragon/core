# Debt record: hardcoded & invented combat constants (`CMB-CONST`)

- **ID:** CMB-CONST (data provenance — in-code constants)
- **Status:** OPEN
- **Owner wave:** W-SRC (source-verification pass)
- **Created:** 2026-07-03, during the combat wave (W-CMB).
- **Category:** code (provenance)
- **Severity:** medium
- **Location:**
  - `core/src/services/combat.rs:22` — `HIT_CHANCE_FLOOR_PER_10000 = 300`
  - `core/src/services/combat.rs:24` — `OVERRATE_NUM = 3`
  - `core/src/services/combat.rs:26` — `OVERRATE_DEN = 10`
  - `core/src/services/combat.rs:28` — `MIN_DAMAGE_FLOOR_DIVISOR = 10`
  - `core/src/services/experience.rs:22` — `OVER_LEVEL_GAP = 10`
  - `core/src/services/experience.rs:24` — `HIGH_LEVEL_VICTIM = 65`
  - `core/src/services/experience.rs:26` — `EXP_FACTOR_NUM = 5`
  - `core/src/services/experience.rs:28` — `EXP_FACTOR_DEN = 4`
  - `core/src/services/loot.rs:31` — `BASE_MONEY_DROP = 7`
  - `core/src/services/loot.rs:33` — `DROP_LEVEL_WINDOW_GAP = 11`
  - `core/src/services/skills.rs:37` — `ONE_TILE_SPEED = 1 tile`
  - `core/src/services/skills.rs:39` — `DASH_SPEED = 8 tiles`
  - `core/src/services/skills.rs:42` — `SINGLE_TARGET_RADIUS_TILES = 1`

## Symptom

Thirteen combat/experience/drop/displacement magnitudes are authored as in-code
`const`s rather than `game_config.json` fields. None is carried by the v2
dataset: the game-config file exposes drop rates, the jitter band, and the
experience curve, but not the hit-chance floor, the overrate penalty ratio, the
min-damage divisor, the era experience factor, the money-drop bonus, the
drop-level window width, or any skill displacement grain. Each therefore lives in
Rust, and each already carries a `// W-SRC:` provenance comment stating its
origin plainly (no `TODO` / `for now` / `temporary` language).

They split into two provenance classes:

- **OpenMU hardcoded combat logic** (extracted WHAT-not-HOW from OpenMU's combat
  routines, but never exposed as a tunable field): `HIT_CHANCE_FLOOR_PER_10000`
  (3% = 300/10000), `OVERRATE_NUM/DEN` (3/10), `MIN_DAMAGE_FLOOR_DIVISOR`
  (`max(1, level/10)`), `EXP_FACTOR_NUM/DEN` (5/4), `OVER_LEVEL_GAP` (10),
  `HIGH_LEVEL_VICTIM` (65), `BASE_MONEY_DROP` (7), `DROP_LEVEL_WINDOW_GAP` (11,
  from the classic `DropLevel > monsterLevel - 12` band). Provenance is *named*
  (OpenMU) but not yet cross-checked against an authentic classic source — the
  same doubt tracked for the data-file values in
  [openmu-default-values.md](openmu-default-values.md).
- **Invented magnitudes** (the locked spec named the constant but not its value):
  `DASH_SPEED` (8 tiles — a lunge dash long enough to close any melee gap),
  `SINGLE_TARGET_RADIUS_TILES` (1 — the clicked cell's pick radius), and
  `ONE_TILE_SPEED` (1 tile — the knockback/shove grain, identical in spirit to
  `MOB_STEP_SPEED`). These are the **same species** as
  [mob-step-speed-provenance.md](mob-step-speed-provenance.md) — an authored
  value with no authentic-source diff.

## Root cause

Classic MU's combat tuning constants and skill displacement grains are unmodeled
in the v2 dataset. Some are genuine OpenMU-hardcoded logic (not data at all in
the source), and some are magnitudes the sourcing left open, filled in Rust to
ship a complete, deterministic wave. Neither class has been diffed against an
authentic classic source. Because these live in `services/*.rs` rather than a
`/data/*.json` file, W-SRC's data-file `review`-string scan would not surface
them — they must be tracked as a debt row instead, exactly as `MOB-SPD` is.

## Why not fixed now

W-SRC (the source-verification pass) is not running, and no authentic-classic
diff for these combat constants is available in this wave. The shipped code is
clean: every value is a correctly-typed module `const` (`u32`/`u16`/`u64`/
`Fixed`/`u8`), placed as the single source of truth in the service that consumes
it, and carries a `// W-SRC:` provenance comment with no forbidden phrase. The
formulas that consume them are the canon-verified OpenMU forms (pinned in
`.claude/agent-memory/canon-guardian/wcmb-canon-pins.md`); only the *provenance*
of the bare magnitudes is open, not their correctness of use.

## Resolution plan (W-SRC)

1. For each **OpenMU-hardcoded** constant, confirm the value against an authentic
   classic source (0.75 / 0.97d combat, experience, and drop routines). Most are
   expected to be confirm-and-keep — the 3% floor, `·3/10` overrate, `max(1,
   L/10)` floor, `·5/4` factor, `+7` money bonus, and the 11-level drop window
   are long-documented classic constants.
2. For each **invented** magnitude (`DASH_SPEED`, `SINGLE_TARGET_RADIUS_TILES`,
   `ONE_TILE_SPEED`), confirm the classic value. Tile-grid movement makes one
   tile the expected knockback/pick grain (likely confirm-and-keep, shared with
   `MOB-SPD`); the lunge dash distance needs a real skill-range source or a
   design decision recorded here.
3. If a value is confirmed: keep the `const` and record the confirmation here.
4. If a value differs: correct the `const` at its `services/*.rs` site and re-run
   `cargo test -p mu-core`.

Kept **out of** [openmu-default-values.md](openmu-default-values.md) so that
record's per-file `review`-string accounting stays exact; these in-code constants
are cross-referenced to the same W-SRC wave instead, mirroring how `MOB-SPD` is
tracked separately.

## Discharge

Debt is discharged when every listed constant is either confirmed against an
authentic classic source (kept, with the confirmation recorded here) or corrected
at its service site. Remove `CMB-CONST` from `DEBT-INDEX.md` at that point.
