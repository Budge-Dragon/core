# Coverage Report — mu-core v2 data extraction

Assembled 2026-07-03 from `data/_coverage/*.json` and the regenerated `/data`.
Contract: the v2 data schema (supersedes v1), as accepted verbatim by the R2 Rust serde
types in `core/src/data/*.rs` and `core/src/components/*.rs` — the authoritative shape.
Review-flagged values are inventoried in section 2.

Baseline: full 0.95d dataset plus curated 1.0-era backports from the Season 6 dataset.
Version policy: `075` = value ships in a Version075 initializer (reused by 0.95d),
`095d` = 0.95d-specific, `s6` = 1.0-era backport. Every `s6` record carries a `review`,
as do OpenMU-default and era-doubtful values in the older tiers (see section 2).

The v2 dataset is purely numeric-identity: the v1 slug vocabulary
(stat / option / set / bonus_table / effect / drop_group / class slugs, the `global`
pseudo-record) is gone — folded into Rust enums and services constants. The dead v1
files (`character_classes.json`, `stats.json`, `stat_map` imports, `item_options.json`,
`item_sets.json`, `magic_effects.json`, `item_level_bonus_tables.json`,
`drop_groups.json`, `spawn_areas.json`, `game_constants.json`) no longer exist.

## 1. Record counts by category and source_version

| Category | File | 075 | 095d | s6 | Total |
|---|---|---:|---:|---:|---:|
| classes | `classes.json` | 3 | 1 | 4 | 8 |
| items | `item_definitions.json` | 196 | 14 | 33 | 243 |
| monsters | `monster_definitions.json` | 73 | 27 | 0 | 100 |
| monsters | `spawns.json` | 1563 | 284 | 0 | 1847 |
| skills | `skills.json` | 30 | 5 | 16 | 51 |
| maps | `map_definitions.json` | 7 | 4 | 0 | 11 |
| maps | `gates_warps.json` | 66 | 4 | 0 | 70 |
| maps | `terrain/` binaries | — | — | — | 11 files |
| options/sets | `ancient_sets.json` | 0 | 0 | 36 | 36 |
| drops | `special_drops.json` | 0 | 5 | 4 | 9 |
| drops | `box_drops.json` | 0 | 1 | 0 | 1 |
| chaos | `chaos_mixes.json` | 1 | 5 | 4 | 10 |
| constants+exp | `exp_tables.json` | 1 | 0 | 0 | 1 |
| constants+exp | `game_config.json` | 1 | 0 | 0 | 1 |
| **Total (JSON records)** | | **1941** | **350** | **97** | **2388** |

Plus 11 terrain binaries (`terrain/0.bin`..`terrain/10.bin`, 65536 bytes each; keyed by
map number, not versioned JSON records).

Structural breakdowns:
- **gates_warps.json** by kind: spawn_gate 12, target_gate 22, enter_gate 22, warp 14.
- **monster_definitions.json** by role: monster 75, npc 18, trap 4, guard 2, soccer_ball 1.
- **spawns.json** by placement: spot 1579, fixed 244, area 24; by schedule: permanent 1845, wandering 2.
- **special_drops.json** by kind: level_banded 4, monster_bound 3, map_bound 2.
- **item_definitions.json** by kind: weapon 45, shield 15, helm 17, body_armor 18,
  pants 18, gloves 18, boots 18, staff 9, crossbow 8, wings 8, bow 7, pendant 6, ring 6,
  jewel 5, orb 5, mix_material 5, pet 4, event_ticket 2, arrows 1, bolts 1, stat_fruit 1,
  transformation_ring 1, lucky_box 1, consumable 10, skill_scroll 14.

## 2. Review-flagged values (185 review strings total)

Every OpenMU-default / OpenMU-modeling / era-doubtful value carries a `review` string,
grouped into named families, each slated for independent re-sourcing against on-era
references. Counts per file:

| File | review strings | families |
|---|---:|---|
| `monster_definitions.json` | 48 | water→lightning remap (48); phantom skill-150 (14 ⊂); golden-era doubt (2 ⊂) |
| `item_definitions.json` | 41 | 33 S6 backports + 8 durability-3 potions |
| `ancient_sets.json` | 36 | S6-transcribed set ordering (all 36); Kantata data-bug fix note |
| `skills.json` | 17 | 16 S6 backports + cometfall AoE-encoding doubt |
| `gates_warps.json` | 14 | 095d warp fee/level list reused from 0.75 |
| `classes.json` | 8 | ability seed, fruit divisor, MG/DL warp fraction, S6 second tiers |
| `special_drops.json` | 7 | drop chances + band edges; box encoding + Feather/Crest era doubt |
| `chaos_mixes.json` | 6 | crafting economics, counts, success splits |
| `game_config.json` | 3 | drop rates; option-roll chances; exp jitter + personal store |
| `map_definitions.json` | 3 | Devias terrain tag; Arena pitch placement; Devil Square collapse |
| `exp_tables.json` | 1 | one 400-cap curve over every era |
| `box_drops.json` | 1 | Box of Luck fixed level 6 + 50%/10k-zen split |
| `spawns.json` | 0 | — |

## 3. Referential integrity

`tools/extract/validate_refs.py` cross-checks all 13 files (2388 records): every item
`{group,number}`, monster/skill/map number, gate target, transformation skin, chaos /
special / box / jewel-drop item ref, ancient-set piece, and class home-map resolves;
every file envelope is exactly `{ "records": [...] }` (no `schema_version` field); every
record carries a valid `source_version` (with an optional `review`). Result: **PASSED**.
The same proof runs in Rust via `Atlas::parse` over the real data in
`core/tests/data_files.rs`.

## 4. Named scope boundaries (not gaps)

Deferred by the R2 contract, consumed by later waves — no data shipped, by design:
- **WalkableGrid / TileTerrain** — host-parsed 256×256 runtime grid from the `terrain/*.bin`
  sidecars; no JSON record references a terrain path.
- **drop-resolution pools** (`drop_pool.rs`) and combat/craft/option/set resolvers —
  W-CMB / W-ENT; the drops *data* records (`DropConfig`, `SpecialDropRecord`, `BoxDrop`)
  ship and are Atlas-checked.
- Values re-homed to Rust: level-bonus curves, generic armor-set ×1.1/×1.05 constants,
  fruit stat weights, per-kill exp knobs, view range, and the killed `stat`/`PowerUp`/
  `Aggregate` vocabulary — all now services constants or enums, not data.

## 5. Category gaps

Per-category `data/_coverage/*.json` records the detailed gap lists. Recurring era-scope
gaps: Devil Square wave rows (map 9, owned by W-DS), Box of Kundun +1..+5 and seasonal
boxes (need an excellent/normal box discriminator), Kalima / castle-siege / post-S3
content, quest and event-reward drops, and the Summoner class line (Red Wing gear ships
as ancient-set pieces only, `classes` empty / unequippable).
