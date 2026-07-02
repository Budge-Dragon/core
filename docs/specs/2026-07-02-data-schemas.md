# Data Schemas — MU Online Static Data (pre-Season-3)

> **⚠️ SUPERSEDED — historical record only.** This v1 spec was contaminated by OpenMU's own data model (a stat catalog and stat-slug references, a generic attribute-evaluator vocabulary with aggregate operators, GUID-shaped cross-file slugs, and float probabilities) — none of which are domain facts. It is replaced by [`docs/specs/2026-07-03-data-schemas-v2.md`](./2026-07-03-data-schemas-v2.md), the AS-BUILT v2 spec produced by the OpenMU purge, which documents what is actually implemented (13 `/data` files, 2388 records; the `core/src/data` + `core/src/components` Rust types). Do not build against this file. Its content is retained below unchanged as the historical record of the pre-purge design.

Proposed JSON schemas for `/data/*.json`, extracted from OpenMU initialization code
(see [`docs/reference/openmu-facts/`](../reference/openmu-facts/) for the domain-fact
inventories these are designed from). Each schema mirrors a future Rust struct in
`core/src/data/`. **No extraction starts until these are approved.**

Everything here follows CLAUDE.md: plain serializable data, kind-tagged enums over
optional soup, no OpenMU structure, no GUIDs, no persistence/networking concerns.

## Conventions (apply to every file)

- **File envelope:** every file is `{ "schema_version": 1, "records": [...] }` — including single-record files (`exp_tables`, `game_constants`) and mixed files (`gates_warps`, kind-tagged records).
- **Field names:** `snake_case`. Simple closed sets: `snake_case` strings. Variants that carry data: `{ "kind": "...", ...fields }` — a field that only exists for some kinds lives inside that kind's object, never as a nullable sibling.
- **Identity:**
  - Numbers that are *domain facts* (the game's own identities) stay numeric: item `{group, number}`, monster `number`, skill `number`, map `{number, discriminator}`, gate `number`, warp `index`, crafting `number`. Class and magic-effect records also carry their client-facing `number` as data.
  - **References between files are by slug** for everything OpenMU keys by GUID *plus* classes and magic effects: stats (`"total_strength"`), option definitions (`"excellent_physical"`), bonus tables (`"weapon_damage"`), set groups (`"warrior_leather"`), drop groups (`"default_money"`), classes (`"dark_knight"`), effects (`"greater_damage"`). Items/monsters/skills/maps/gates are referenced by their numeric identities.
- **Version tagging:** every record carries `"source_version": "075" | "095d" | "s6"` (s6 only for curated pre-S3 backports). Questionable records additionally carry `"review": "<why>"` and land in the coverage report.
- **Stat references:** always by slug into `stats.json`. Class-local intermediates (e.g. Elf's `total_strength_and_agility`) are declared there too, `"scope": "intermediate"`.
- **Chances:** fractions `0.0..1.0` (f64). **One carve-out:** crafting success rates (§14) are integer *percent points* — the domain couples them to zen cost (`zen_per_success_percent`) and additive percent bonuses; converting to fractions would obscure the source values. Fields there are suffixed `_percent`. **Resistances:** fractions (source `n/255`, conversion documented in the extractor).
- **Durations:** integer milliseconds, `_ms` suffix, everywhere. Core converts to `Tick` at load using `tick_duration_ms` from `game_constants.json` (decision 4).
- **Rectangles:** `{ "x1": u8, "y1": u8, "x2": u8, "y2": u8 }`; points have `x1==x2, y1==y2`.
- **Shared power-up shape:** `{ "stat": "<slug>", "value": f64, "aggregate": "add_raw" | "multiplicate" | "add_final" | "maximum" }`, optionally `"scaled_by": [{ "stat", "operator", "operand": f64 }]` and `"max": f64`. In item context it may additionally carry `"bonus_table": "<slug>"` (§3/§4). Aggregation semantics (domain fact): `total = (sum of add_raw) * (product of multiplicate) + (sum of add_final)`; `maximum` takes the max.
- **Absent optional = "not applicable"** only for genuine domain optionality (a monster with no attack skill). Where absence would encode a *state*, a kind-tagged variant is used instead. `"chance"` absent on an effect means 1.0 — parsed to a concrete value at load, never re-checked downstream.

---

## 1. `stats.json` *(addition to your list — required)*

The stat catalog is the universal currency: classes, items, options, monsters, and
effects all reference stats. ~80–100 pre-S3 entries.

```json
{ "id": "attack_speed", "max_value": 200.0, "scope": "derived", "source_version": "075" }
{ "id": "base_strength", "scope": "base", "source_version": "075" }
{ "id": "is_shield_equipped", "scope": "flag", "source_version": "075" }
{ "id": "total_strength_and_agility", "scope": "intermediate", "source_version": "075" }
```

- `scope` ∈ `base` (player-increasable) | `derived` | `resource` (current/maximum pairs) | `flag` (0/1) | `intermediate` (formula helper).
- `max_value` optional cap (only 6 stats carry one pre-S3).

## 2. `character_classes.json`

```json
{
  "id": "dark_knight", "number": 4, "source_version": "075",
  "created_by_player": true, "creation_unlock_level": 0,
  "home_map": { "number": 0, "discriminator": 0 },
  "points_per_level": 5, "fruit_calculation": "default",
  "warp_level_reduction_percent": 0,
  "base_stats": [{ "stat": "base_strength", "value": 28, "increasable": true }],
  "const_values": [{ "stat": "maximum_health", "value": 35.0 }],
  "stat_formulas": [
    { "target": "defense_base", "input": "total_agility", "operator": "multiply",
      "operand": { "kind": "constant", "value": 0.3333333333 }, "aggregate": "add_raw" }
  ]
}
```

- `stat_formulas` are the attribute relationships (data, not code). `operator` ∈ `multiply | add | exponentiate | exponentiate_by_attribute | minimum | maximum`. `operand` is kind-tagged: `{ "kind": "constant", "value": f64 }` or `{ "kind": "stat", "stat": "<slug>" }` (dynamic multipliers and 0/1 conditional gates).
- `fruit_calculation` ∈ `default | magic_gladiator | dark_lord`.
- `warp_level_reduction_percent`: 34 for MG (and DL if included), 0 otherwise.
- `evolution`: `{ "class": "blade_knight", "at_level": 150 }` or `null` — second-class evolution (1.0-era backport; approved under decision 1 option b).
- Global formulas/const values shared by all classes live in one pseudo-record `"id": "global"` (includes the classic-PvP rules: no shield stats; `attack_rate_pvp = attack_rate_pvm`).

## 3. `item_definitions.json`

```json
{
  "id": { "group": 0, "number": 5 }, "name": "Blade", "source_version": "075",
  "width": 1, "height": 3, "slot": "left_or_right_hand",
  "drops_from_monsters": true, "drop_level": 36, "maximum_drop_level": null,
  "max_item_level": 11, "durability": 39, "value": 0,
  "skill": { "kind": "granted_while_equipped", "skill": 22 },
  "consume_effect": null, "is_ammunition": false,
  "classes": ["dark_knight", "magic_gladiator"],
  "requirements": [{ "stat": "total_strength_requirement", "value": 80 }],
  "base_power_ups": [
    { "stat": "minimum_phys_base_dmg_by_weapon", "value": 36.0,
      "aggregate": "add_raw", "bonus_table": "weapon_damage" }
  ],
  "possible_options": ["luck", "physical_attack"],
  "possible_set_groups": [],
  "box_drops": []
}
```

- `slot` ∈ `left_hand | right_hand | left_or_right_hand | helm | armor | pants | gloves | boots | wings | pet | pendant | ring` (absent = not equippable).
- `skill`: `{ "kind": "granted_while_equipped", "skill": n }` (weapons/shields; active when the instance rolled +Skill) or `{ "kind": "taught_on_consume", "skill": n }` (orbs/scrolls) or `null`. OpenMU conflates these in one field; we split by kind.
- `box_drops` (Box of Luck-style, applied per source item level): shared fields `source_item_level`, `chance`, `required_character_level`, plus a kind: `{ "kind": "item_list", "items": [...], "level_range": [6, 6] }` | `{ "kind": "random_item", "level_range": [min, max] }` | `{ "kind": "money", "amount": 10000 }`.
- `possible_set_groups` → `item_sets.json` (§6); the field stays regardless of decision 5 because generic (non-ancient) armor sets use it too.
- Requirement *scaling* (`(3|4) · dropLevel · raw / 100 + 20`, +25 excellent / +30 ancient / +3·itemLevel) and the NPC price formula are rules → decision 3.

## 4. `item_level_bonus_tables.json`

```json
{ "id": "weapon_damage", "source_version": "075",
  "values_by_level": [0, 3, 6, 9, 12, 15, 18, 21, 24, 27, 31, 36] }
```

Dense array, index = item level, length = cap + 1 (decision 2). Includes: weapon damage,
staff rise (even/odd), armor defense, shield defense, shield defense-rate, wing absorb /
wing damage, jewelry resistance, ammunition damage (095d), durability-per-level.

## 5. `item_options.json`

```json
{
  "id": "excellent_physical", "option_type": "excellent", "source_version": "095d",
  "adds_randomly": true, "add_chance": 0.001, "max_per_item": 2,
  "options": [
    { "number": 3, "kind": "fixed",
      "power_up": { "stat": "attack_speed_any", "value": 7.0, "aggregate": "add_raw" } },
    { "number": 5, "kind": "fixed",
      "power_up": { "stat": "physical_base_dmg", "value": 0.0, "aggregate": "add_raw",
        "scaled_by": [{ "stat": "total_level", "operator": "multiply", "operand": 0.05 }] } }
  ]
}
{
  "id": "physical_attack", "option_type": "option", "source_version": "075",
  "adds_randomly": true, "add_chance": 0.25, "max_per_item": 1,
  "options": [
    { "number": 1, "kind": "per_level", "level_type": "option_level",
      "stat": "physical_base_dmg", "aggregate": "add_raw",
      "levels": [{ "level": 1, "value": 4.0 }, { "level": 2, "value": 8.0 },
                  { "level": 3, "value": 12.0 }, { "level": 4, "value": 16.0 }] }
  ]
}
```

- `option_type` ∈ `option | luck | excellent | ancient_option | ancient_bonus | wing` (harmony/guardian/socket post-S3, excluded).
- Option entries are kind-tagged: `fixed` (flat power-up) or `per_level` (leveled values; `level_type` ∈ `option_level` (jewel-raised) | `item_level` (follows +level)).
- The implicit excellent/ancient base bonuses (e.g. armor `def += base·12/dropLevel + dropLevel/5 + 4`) are rules → decision 3.

## 6. `item_sets.json` *(generalized from `ancient_sets` — covers generic armor sets too)*

```json
{
  "id": "warrior_leather", "number": 1, "source_version": "s6",
  "min_item_count": 2, "count_distinct": true, "set_level": 0, "always_applies": false,
  "pieces": [
    { "item": { "group": 11, "number": 5 }, "discriminator": 1, "bonus_stat": "base_vitality" },
    { "item": { "group": 13, "number": 12 }, "discriminator": 1, "bonus_stat": null }
  ],
  "set_options": [{ "number": 1,
    "power_up": { "stat": "base_strength", "value": 10.0, "aggregate": "add_raw" } }]
}
```

- `discriminator` is **per piece** (1 or 2; several sets mix both — Mist, Rave, Drake, Gywen, Agnis, Chrono); `0` = generic non-ancient set piece. `bonus_stat` nullable (Gywen's pendant carries no per-piece bonus).
- Per-piece ancient bonus values (+5 level 1 / +10 level 2, level rolled at drop) are the `ancient_bonus` definition in `item_options.json`.
- Generic armor sets (075/095d `BuildSets`: full-set defense-rate ×1.1; defense ×(1+(setLevel−9)·0.05) at set level ≥10) use `set_level` > 0, `always_applies: true`, `min_item_count` = set size. **Extractor must resolve a source contradiction:** one fact file says 075/095d build these, another says 075 has only a TODO — report actuals per version.
- Application rule (k of n pieces ⇒ options 1..k−1; full set ⇒ all) is a rule in services.
- Ancient records pending decision 5.

## 7. `skills.json`

```json
{
  "number": 19, "id": "falling_slash", "name": "Falling Slash", "source_version": "075",
  "attack_damage": 0, "damage_type": "physical",
  "behavior": { "kind": "direct_hit" },
  "target": "explicit", "target_restriction": "none", "range": 3,
  "implicit_target_range": 0, "hits_per_attack": 1,
  "moves_to_target": true, "moves_target": true,
  "element": null, "skip_elemental_modifier": false,
  "effect": null,
  "requirements": [], "consume": [{ "stat": "current_mana", "value": 9 }],
  "classes": ["dark_knight"]
}
```

- `behavior` is kind-tagged; variant-only data lives inside it:
  - `{ "kind": "direct_hit" }`
  - `{ "kind": "area_automatic", "area": {...} }` / `{ "kind": "area_explicit", "area": {...} }` / `{ "kind": "area_explicit_target", "area": {...} }`
  - `{ "kind": "buff" }` / `{ "kind": "regeneration" }` / `{ "kind": "passive" }`
  - `{ "kind": "summon", "monster": 26 }`
  - `{ "kind": "other" }` (e.g. Teleport)
- `area`: `{ "geometry": { "kind": "frustum", "start_width": 1.0, "end_width": 4.5, "distance": 7.0 } | { "kind": "circle", "diameter": 2.0 } | { "kind": "none" }, "deferred_hits": bool, "delay_per_tile_ms": u32, "delay_between_hits_ms": u32, "hits_per_target": [min, max], "hits_per_attack_range": [min, max], "hit_chance_per_distance": f64, "projectile_count": u8, "effect_range": u8 }`.
- `effect` (slug into §8) stays top-level: any skill kind may apply one (buffs and elemental side effects alike) — genuine optionality.
- `damage_type` ∈ `none | physical | wizardry`. `target` ∈ `explicit | implicit_party | implicit_players_in_range | implicit_npcs_in_range | implicit_all_in_range | explicit_with_implicit_in_range | self`. `target_restriction` ∈ `none | self | party | player`. `element` ∈ `ice | poison | lightning | fire | earth | wind | water | null`.
- Optional `damage_scaling: [{ "stat", "operator", "operand" }]` — per-skill damage scaling; only needed by curated 1.0-era backports (Nova, Earthshake — decision 1).
- No cooldown field exists in the source model; omitted (decision 8).

## 8. `magic_effects.json` *(addition — required by skills & consumables)*

```json
{
  "id": "greater_damage", "number": 1, "source_version": "075",
  "sub_type": 1, "stop_by_death": true,
  "duration": { "constant_ms": 60000,
    "scaled_by": [{ "stat": "total_energy", "operator": "multiply", "operand": 100.0 }],
    "max_ms": null },
  "power_ups": [{ "stat": "greater_damage_bonus", "value": 3.0, "aggregate": "add_raw",
    "scaled_by": [{ "stat": "total_energy", "operator": "multiply", "operand": 0.142857 }] }]
}
```

- Referenced by slug; `number` is the client wire id (data, not a reference key).
- `sub_type`: effects sharing a `sub_type` replace each other (no stacking).
- `duration.scaled_by` operates in ms (source "seconds per N stat" converted by extractor).
- `chance` absent = 1.0 (see conventions). `InformObservers`/`SendDuration` are client-protocol concerns → not in core data. PvP-variant fields never populated pre-S3 → omitted.

## 9. `monster_definitions.json`

```json
{
  "number": 7, "name": "Giant", "source_version": "075",
  "role": { "kind": "monster" },
  "move_range": 3, "attack_range": 1, "view_range": 6,
  "move_delay_ms": 400, "attack_delay_ms": 1600, "respawn_ms": 3000,
  "max_item_drops": 1, "attack_skill": null,
  "stats": [
    { "stat": "level", "value": 17.0 }, { "stat": "maximum_health", "value": 400.0 },
    { "stat": "minimum_phys_base_dmg", "value": 57.0 }, { "stat": "defense_rate_pvm", "value": 6.0 },
    { "stat": "poison_resistance", "value": 0.0117647 }
  ],
  "drop_groups": []
}
```

- `role` is kind-tagged; kind-specific data lives inside:
  - `{ "kind": "monster" }` — default aggressive AI
  - `{ "kind": "npc", "window": "merchant" | "storage" | "vault" | "chaos_machine" | "guild_master" | "devil_square" | "legacy_quest" | null }`
  - `{ "kind": "guard" }` — guard AI
  - `{ "kind": "trap", "ai": "attack_single_pressed" | "attack_area_pressed" | "random_in_range" | "area_target_in_direction" }`
  - `{ "kind": "soccer_ball" }` (Arena battle soccer)
- OpenMU's opaque `Attribute` byte (semantics unknown even to them) is dropped; `role` discriminates. `gate`/`statue`/`destructible` kinds are S6 event objects — excluded.
- Deferred, like merchant inventories: **`merchant_stores.json`** (NPC shop contents) and **`quests.json`** (legacy quest defs — 095d has Sevina's quests; drops are already representable in §13) — both listed in the review list, extracted in a follow-up wave.

## 10. `spawn_areas.json`

```json
{ "map": { "number": 0, "discriminator": 0 }, "monster": 7,
  "area": { "x1": 5, "y1": 5, "x2": 240, "y2": 240 }, "quantity": 45,
  "direction": null, "trigger": { "kind": "automatic" }, "source_version": "075" }
```

- `trigger` kinds: `automatic`, `wandering`, `once_at_event_start`, `manually_for_event`, `{ "kind": "automatic_during_wave", "wave": 2 }`, `{ "kind": "once_at_wave_start", "wave": 10 }` — wave number lives on the wave variants only.
- `direction` ∈ `west | south_west | south | south_east | east | north_east | north | north_west | null` (8-way compass; absence is `null`, no `undefined` variant).

## 11. `map_definitions.json`

```json
{
  "number": 0, "discriminator": 0, "id": "lorencia", "name": "Lorencia", "source_version": "075",
  "terrain": "terrain/075_lorencia.bin", "exp_multiplier": 1.0,
  "safezone_map": { "number": 0, "discriminator": 0 },
  "requirements": [], "character_power_ups": [],
  "drop_groups": ["default_money", "default_random_item", "default_jewels"],
  "battle_zone": null
}
```

- **Terrain sidecars** (`/data/terrain/*.bin`): raw 65,536-byte grid (256×256, `index = y*256 + x`), our own bit flags `safezone=1, blocked=2, no_ground=4, water=8` (re-encoded from .att; runtime occupancy flag dropped). Not JSON — 64 KB of cells has no business in a text file.
- `battle_zone` (Arena only): `{ "battle_type": "normal" | "soccer", "ground": rect, "left_goal": rect, "right_goal": rect, "left_spawn": { "x": u8, "y": u8 }, "right_spawn": { "x": u8, "y": u8 } }`.
- Icarus: `requirements: [{ "stat": "can_fly", "value": 1 }]`. Atlans: `character_power_ups: [{ "stat": "is_underwater", "value": 1, "aggregate": "add_raw" }]`.

## 12. `gates_warps.json`

One file, kind-tagged records (standard envelope):

```json
{ "kind": "exit_gate", "number": 17, "map": { "number": 0, "discriminator": 0 },
  "area": { "x1": 133, "y1": 118, "x2": 151, "y2": 135 },
  "direction": null, "is_spawn_gate": true, "source_version": "075" }
{ "kind": "enter_gate", "number": 1, "map": { "number": 0, "discriminator": 0 },
  "area": { "x1": 121, "y1": 232, "x2": 121, "y2": 233 },
  "target_gate": 2, "min_level": 20, "source_version": "075" }
{ "kind": "warp", "index": 2, "name": "Lorencia", "cost_zen": 2000, "min_level": 10,
  "target_gate": 17, "source_version": "075" }
```

- Gates are keyed by `number` (Gate.txt identity); warps by `index` (warp-list position) — both domain facts, hence the different key names.

## 13. `drop_groups.json`

```json
{ "id": "default_money", "kind": "money", "chance": 0.5, "source_version": "075" }
{ "id": "default_random_item", "kind": "random_item", "chance": 0.3, "source_version": "075" }
{ "id": "default_jewels", "kind": "jewel", "chance": 0.001, "source_version": "075" }
{ "id": "default_excellent", "kind": "excellent", "chance": 0.0001, "source_version": "095d" }
{ "id": "dragon_invasion_box", "kind": "item_list", "chance": 1.0,
  "items": [{ "group": 14, "number": 11 }], "item_level": 0,
  "monster": 44, "min_monster_level": null, "max_monster_level": null, "source_version": "095d" }
```

- `kind` ∈ `money | random_item | jewel | excellent | ancient | item_list` (socket post-S3). `items`/`item_level` exist on `item_list` only; `monster` + monster-level bounds are genuine optionality on any kind.
- Drop *mechanics* (item level = `(monsterLevel − dropLevel)/3` capped, pool gap 12, money = exp + 7, 50% skill roll, excellent pool at −25) are rules → decision 3.

## 14. `chaos_mixes.json`

```json
{
  "number": 1, "id": "chaos_weapon", "name": "Chaos Weapon", "source_version": "075",
  "behavior": "chaos_weapon_and_first_wings",
  "cost": { "flat_zen": 0, "zen_per_success_percent": 10000 },
  "success": { "base_percent": 0, "max_percent": 100, "npc_price_divisor": 20000,
               "luck_bonus_percent": 0 },
  "inputs": [
    { "match": { "kind": "any_item", "min_level": 4, "max_level": 11,
                 "required_option_types": ["option"] },
      "amount": { "min": 1, "max": 1 },
      "on_success": "disappear", "on_fail": "downgrade_chaos_weapon",
      "npc_price_divisor": null, "add_percent_per_extra": 0, "ref": null },
    { "match": { "kind": "specific_items", "items": [{ "group": 12, "number": 15 }] },
      "amount": { "min": 1, "max": null },
      "on_success": "disappear", "on_fail": "disappear",
      "npc_price_divisor": null, "add_percent_per_extra": 0, "ref": null }
  ],
  "results": [
    { "kind": "create", "item": { "group": 2, "number": 6 }, "level_range": [0, 4],
      "durability": null }
  ],
  "result_selection": "any", "multiple_allowed": false,
  "result_chances": { "luck_percent": 0, "skill_percent": 0, "excellent_percent": 0 }
}
```

- `match` is kind-tagged: `{ "kind": "specific_items", "items": [...] }` or `{ "kind": "any_item", "min_level", "max_level", "required_option_types" }` — no null-as-"any".
- `amount.max: null` = unbounded (consume all matching). `amount.min: 0` = optional ingredient.
- `ref` links an input to a result for **in-place modification** (item-upgrade mixes): a result `{ "kind": "modify", "ref": 1, "add_level": 1 }` upgrades the input carrying `"ref": 1`. `null` = unlinked. `results[].kind` ∈ `create | modify`.
- Per-input `npc_price_divisor` adds `sum(NPC prices of matched items)/divisor` percent (2nd wings use it); the settings-level divisor in `success` *replaces* the additive path (chaos weapon / 1st wings).
- `behavior` ∈ `simple | chaos_weapon_and_first_wings | second_wings | dinorant | ticket_devil_square | ticket_blood_castle` (post-S3 handlers excluded). Behavior-specific option formulas (chaos-weapon option chance `rate/5 + 4(i+1)` etc.) are rules. `on_fail` ∈ `disappear | stays | downgrade_chaos_weapon`. `result_selection` ∈ `any` (one random) | `all`.
- Jewel mixes (Lahap packing): pending decision 5 — S6-only data, era-questionable item ids.

## 15. `exp_tables.json`

```json
{ "max_level": 400,
  "formula": "10*(level+8)*(level-1)^2 [+ 1000*(level-247)*(level-256)^2 for level>=256]",
  "total_exp_by_level": [0, 0, 100, 396, 1080, "... 402 entries, u64"] }
```

- Precomputed dense array, index = level; `formula` is documentation only — core reads the table. This file owns `max_level` (not duplicated in constants).
- Per-kill experience (`(lvl+25)·lvl/3`, level-gap scaling, ≥65 bonus, ×1.25, ×random[0.8, 1.2]) is a rule with constants → decision 3.

## 16. `game_constants.json`

Single record of global scalars (source values identical across OpenMU versions):

```json
{
  "source_version": "075",
  "tick_duration_ms": 100,
  "recovery_interval_ms": 3000, "info_range": 12,
  "item_drop_duration_ms": 60000, "max_item_option_level_drop": 3,
  "excellent_drop_level_delta": 25, "max_option_level": 4,
  "should_drop_money": true, "money_amount_rate": 1.0,
  "base_money_drop": 7, "drop_level_max_gap": 12, "skill_drop_chance": 0.5,
  "max_inventory_money": 2000000000, "max_vault_money": 2000000000,
  "clamp_money_on_pickup": false,
  "maximum_party_size": 5, "max_characters_per_account": 5,
  "character_name_regex": "^[a-zA-Z0-9]{3,10}$",
  "prevent_experience_overflow": false, "area_skill_hits_player": false,
  "random_exp_multiplier_range": [0.8, 1.2],
  "damage_per_one_item_durability": 2000, "damage_per_one_pet_durability": 100000,
  "hits_per_one_item_durability": 10000,
  "minimum_hit_chance": 0.03, "overrate_damage_factor": 0.3,
  "inventory": { "equipped_slots": 12, "main_rows": 8, "main_columns": 8,
                 "store_slots": 32, "vault_slots": 120, "temp_storage_slots": 32 },
  "movement": { "running_gear_min_level": 5, "running_gear_speed": 15.0,
                "wing_speed": 15.0, "fast_wing_speed": 16.0, "iced_speed_factor": 0.5 }
}
```

- `tick_duration_ms` is ours, not OpenMU's (decision 4). Zen caps are the classic 2,000,000,000, not OpenMU's i32::MAX (decision 7).
- Letters (`max_letters` 50, `letter_send_price` 1000) are social/persistence features — host-owned, excluded here; flagged in case you want them.

---

## Excluded wholesale (post-S3 / non-domain)

Sockets & seeds, harmony (Jewel of Harmony) options, guardian/380 options, master
skill tree (S4), shield/SD stat system (classic PvP instead), Summoner & Rage Fighter,
3rd wings & capes, castle siege skills/potions/maps, Kanturu/Illusion Temple/S6 maps
& minigames, inventory extensions, extended vault, trainable pets (Dark Horse/Raven,
Fenrir — decision 5), item levels 12–15 (decision 2), packed jewels/refine stones,
`LevelDependentDamage` (dead data even in OpenMU), localization (plain strings),
client-notification flags (`InformObservers`, `SendDuration`, drop effects, option
visibility), letters/inbox, account/password constants, S6 event objects
(gate/statue/destructible roles), `maximum_health_override` on spawns (S6-only usage).

## Deferred (schema later, not this wave)

- `merchant_stores.json` — NPC shop inventories (075 `MerchantStores.cs`).
- `quests.json` — legacy quest definitions (095d Sevina; quest *drop groups* already fit §13).

## Review list — your call before extraction (decisions)

1. **Target baseline.** OpenMU pre-S3 datasets: 0.75 (196 items, 3 classes, 30 skills, 8 maps) and 0.95d (~215 items, 4 classes, 35 skills, 14 maps + Devil Square). Historical "pre-S3" (0.97–1.0) content (Dark Lord, class evolution at 150, Soul Barrier/Nova/Rageful Blow/Death Stab/Ice Arrow/combo, 2nd wings, Blood Castle, ancient sets, fruits, personal store) exists **only in the S6 dataset** and needs curated backporting (some of it needs `damage_scaling`, §7). Options: (a) 0.95d baseline now, curated 1.0-era backports as a follow-up wave — **my recommendation**; (b) 0.95d + curation in one wave; (c) 0.75 only.
2. **Item level cap: 11 or 15.** Both pre-S3 datasets cap at +11 (bonus tables end there). Recommendation: 11.
3. **Formula constants: data or Rust.** Requirement scaling, implicit excellent/ancient bonuses, drop item-level formula, durability-per-level bonus, per-kill exp, crafting option formulas, **NPC item price formula** (feeds `npc_price_divisor`). Recommendation: scalar knobs → `game_constants.json`; formula *shapes* → named constants in `services/` (rules, not data). Alternative: everything data.
4. **Durations.** Data keeps source milliseconds (`_ms`) + `tick_duration_ms` in constants; core converts at load. Requires a tick rate — 100 ms proposed. Alternative: bake ticks into the JSON now.
5. **Era-ambiguous content — include/exclude each:** ancient sets (36 in S6 data, S1-era content — curate to ~30?); Dark Lord; class evolution; jewel mixes; fruits; personal store; Fenrir (S2); DK combo; 2nd wings; Devil Square (0.95d ships it — I'd keep); wandering merchants; guild alliances (`maximum_alliance_size`, S6-only value); Kantata set's `10.0` excellent-damage value (OpenMU data bug — adopt `0.10`?).
6. **Exp curve:** accept OpenMU's max level 400 + two-piece formula for all versions (historically 0.75 capped lower)? Recommendation: accept — it's the shipped data.
7. **Zen cap:** 2,000,000,000 (classic) vs i32::MAX (OpenMU). Recommendation: classic.
8. **Skill cooldown field:** absent from OpenMU's model. Omit (recommendation) or add speculatively?
