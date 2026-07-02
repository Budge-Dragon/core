# Data Schemas v2 — MU Online Static Data (pre-Season-3), AS-BUILT

> **Status: CURRENT — implemented and green.** Dated 2026-07-03.
>
> This document supersedes [`docs/specs/2026-07-02-data-schemas.md`](./2026-07-02-data-schemas.md)
> (the v1 spec, which was contaminated by OpenMU's own data model — a stat catalog,
> a generic attribute-evaluator vocabulary, GUID-shaped slug references, and
> aggregate-operator power-ups that are OpenMU's persistence shape, not domain
> facts). v2 is the corrected design, and this file documents **what is actually
> built** — not a proposal. It is the reference the future feature waves build
> against.
>
> As-built surface it describes:
> - **13 JSON files under `/data`, 2388 records total**, regenerated from the v2
>   extractors, parsing green through the Rust types and the load-time Atlas proof.
> - The record types in **`core/src/data/*.rs`** and the value vocabulary in
>   **`core/src/components/*.rs`** — these Rust types are authoritative on every
>   shape, field name, enum value, and encoding. Where this prose and the code
>   disagree, the code wins.
> - **11 terrain sidecar files** (`/data/terrain/<map>.bin`), host-parsed, not part
>   of the JSON record set.

Everything obeys `CLAUDE.md`: hexagonal purity, enums over optional soup, illegal
states unrepresentable, parse-don't-validate at the load boundary, zero debt.

---

## The frame

v2 is the OpenMU-purge design realized. Eight binding principles, and how each
landed in code:

1. **`stats.json` is deleted with no successor.** There is no stat catalog, no
   stat slug, no `StatId`, no stat reference anywhere. Trainable-stat shape,
   derived-stat formulas, caps, and regen constants are Rust — typed columns in
   the domains that own them and (future-wave) exhaustive-match functions in
   `services/stats/`. A class's starting stats are the classes record's
   `starting_stats` field, not a row in a catalog.

2. **The generic attribute-evaluator vocabulary is deleted.** No
   `aggregate: add_raw | multiplicate | add_final | maximum`, no `scaled_by`
   operator chains, no `bonus_table` slugs, no power-up struct. OpenMU's runtime
   stat-composition engine never crosses into core; each producing domain does
   its own scaling and emits a concrete, already-resolved contribution.

3. **One `CombatBonus` currency.** `components/bonus.rs::CombatBonus` is the single
   resolved-contribution enum every item / option / set / effect / pet producer
   emits (kind-tagged, flat variants). `ancient_sets.json` serializes `CombatBonus`
   values inline; pet `bonuses` are `CombatBonus` inline. The only sanctioned
   residue is the small `ConditionalSetBonus` enum (one variant), whose effect
   depends on a runtime equipment fact and so cannot be resolved at load.

4. **Closed sets are Rust enums; open rosters are data.** The element vocabulary
   (7), the class roster (8), damage types, skill shapes, area patterns, buffs,
   ailments, jewel kinds, consume effects, map environments, NPC windows, trap
   targeting, chaos-recipe families, item kinds — every closed pre-S3 set is an
   enum, so a new variant breaks the build until every dispatch handles it. The
   open rosters (which monsters, which items, which spawns exist) are the JSON
   record lists.

5. **Integer-first, deterministic encodings.** Every probability, unit, and
   identity is an exact integer newtype guarded at the parse boundary — never a
   float. `ChancePer10000` (the classic `rand()%10000` grain) and whole `Percent`
   are the only two probability units; there is no per-mille and no fraction.
   Same seed + same inputs = same outputs on every target.

6. **Numeric-identity references + one load-time Atlas proof.** Cross-file edges
   are the game's own numeric identities (`ItemRef`, `MonsterNumber`,
   `SkillNumber`, `GateNumber`, `MapNumber`) — never slugs. `data/atlas.rs::Atlas::parse`
   is the single referential-integrity proof over the whole dataset: per-file
   identity uniqueness, and resolution of every declared cross-file reference, in
   one pass. Downstream every accessor is total or genuinely optional.

7. **Single ownership.** Each fact lives in exactly one place. The effective-drop-
   level rule, the enhancement curves, the chance seam — each has one home. No
   fact is duplicated across a data file and a service.

8. **Provenance preserved; OpenMU-invented values flagged, never laundered.**
   Every record carries `source_version` (`075` / `095d` / `s6`). Any value that
   is an OpenMU default, modeling artifact, or era-doubtful backport carries a
   `review` string naming the doubt; those 185 flags are the debt backlog
   [`docs/debt/openmu-default-values.md`](../debt/openmu-default-values.md) (wave
   W-SRC). `review` is documentation-for-humans, never read by any service. Zero
   debt otherwise.

---

## Conventions

### File envelope

Every `/data/*.json` file is:

```json
{ "records": [ /* ... */ ] }
```

The single top-level key is `records`. **There is no `schema_version` field** — it
was removed (`core/src/data/common.rs::DataFile<T>` carries only `records`). This
holds uniformly, including the single-record files (`exp_tables`, `game_config`).
Hosts load each file as `DataFile<T>` for the file's record type `T`, then hand
the whole set to `Atlas::parse`.

### Integer encoding table

Every numeric value is a guarded newtype; the wire form is a bare integer. Parse
rejects out-of-range input at the load boundary (`try_from`); the compute path
uses total `clamped` constructors where noted.

| Unit / newtype | Wire | Range / rule | Home |
|---|---|---|---|
| `ItemRef` | `{ "group": u8, "number": u16 }` | object, two bare ints | `data/common.rs` |
| `MonsterNumber`, `SkillNumber`, `GateNumber` | bare `u16` | newtype | `data/common.rs` |
| `WarpIndex` | bare `u16` | newtype | `data/gates_warps.rs` |
| `ClassNumber` | bare `u8` | open wire byte (0/2/4/6/8/10/12/16) | `components/class.rs` |
| `MapNumber` | bare `u8` | single byte, no discriminator | `data/common.rs` |
| `Level` | bare `u16` | `>= 1`; `0` rejected (`NonZeroU16`) | `components/units.rs` |
| `ItemLevel` | bare `u8` | `0..=15` (4-bit wire field) | `components/units.rs` |
| `EnhanceLevel` | bare `u8` | `0..=11` enum (one variant per level) | `components/levels.rs` |
| `OptionLevel` | bare `u8` | `1..=4` enum | `components/levels.rs` |
| `DurationMs` | bare `u32`, field `*_ms` | milliseconds | `components/units.rs` |
| `TickDuration` | bare `u32`, field `tick_duration_ms` | `NonZeroU32` | `components/units.rs` |
| `Zen` | bare `u64` | fees, prices, caps | `components/units.rs` |
| `Exp` | bare `u64` | experience totals/gains | `components/units.rs` |
| `ChancePer10000` | bare `u16`, field `*_per_10000` | `0..=10000` | `components/units.rs` |
| `Percent` | bare `u8` | `0..=100` (crafting success unit) | `components/units.rs` |
| `Resistance` | bare `u8` | `0..=255` (raw `n/255` byte) | `components/units.rs` |
| `SetNumber` | bare `u8` | `NonZeroU8` (`>= 1`) | `data/ancient_sets.rs` |
| `NonZeroU32 / U16 / U8` | bare int | rejects `0` | `core::num` |
| `SourceVersion` | `"075" \| "095d" \| "s6"` | era tag | `data/common.rs` |

Rate multipliers that legitimately exceed 100% (e.g. `exp_jitter_percent`
`80..=120`) are plain `u16` `_percent` fields, **not** `Percent` and **not**
`ChancePer10000` — the distinction is deliberate.

Enums serialize `snake_case` (`#[serde(rename_all = "snake_case")]`). Data-carrying
variants are internally tagged: `{ "kind": "...", ...fields flat }`. A field that
exists only on one variant lives inside that variant's object, never as a nullable
sibling.

### Provenance

`Provenance` (`data/common.rs`) is embedded with `#[serde(flatten)]`, so its fields
sit at the record's top level — **not** in a nested object:

```json
"source_version": "075",
"review": "…why this value needs an authentic source before trust"
```

- `source_version` is always present.
- `review` is optional: omitted when the value is uncontested
  (`skip_serializing_if = "Option::is_none"`).

**ChaosMix carve-out.** `chaos_mixes.rs::ChaosMix` inlines the same pair as two
named fields (`source_version`, `review?`) rather than a flattened `Provenance`.
This is the single deliberate exception — it keeps the `recipe` object as the
record's one nested payload. The wire shape of the pair is identical either way.

### Geometry

`components/geometry.rs`:

- `Point` — `{ "x": u8, "y": u8 }` on the 256×256 tile grid.
- `Rect` — `{ "x1": u8, "y1": u8, "x2": u8, "y2": u8 }`, inclusive, **edge-ordered**
  (`x1 <= x2`, `y1 <= y2`) proven at parse. A single tile is `x1==x2, y1==y2`.
- `Direction` — 8-way compass, `snake_case`: `west`, `south_west`, `south`,
  `south_east`, `east`, `north_east`, `north`, `north_west`. Core assigns no wire
  ordinals.

### Intervals

`components/interval.rs::Interval<T>` is the one inclusive-range type,
`{ "min": T, "max": T }` with `min <= max` proven at parse. It is reused as
`ItemLevelRange` (box drops), `ItemLevelWindow` (chaos recipes), and the `u16`
`RatePercentRange` (exp jitter). The wire field names are always `min` / `max`.

### Identities & optionality

References across files are numeric identities only, resolved by `Atlas::parse`.
`Option` on the wire is genuine domain optionality (a monster with no attack skill,
a ring with no resistance), never an implicit state flag — a stateful distinction
is a kind-tagged variant instead. Optional fields are omitted when absent; a few
extractors emit an explicit `null` (skills' `element` / `inflicts`), and the parse
accepts either (`default` + `skip_serializing_if`).

---

## Data files

| # | File | Record type (`core/src/data/…`) | Records |
|---|---|---|---:|
| 1 | `classes.json` | `classes::ClassRecord` | 8 |
| 2 | `item_definitions.json` | `item_definitions::ItemDefinition` | 243 |
| 3 | `ancient_sets.json` | `ancient_sets::AncientSet` | 36 |
| 4 | `skills.json` | `skills::Skill` | 51 |
| 5 | `monster_definitions.json` | `monster_definitions::MonsterDefinition` | 100 |
| 6 | `spawns.json` | `spawns::Spawn` | 1847 |
| 7 | `map_definitions.json` | `map_definitions::MapDefinition` | 11 |
| 8 | `gates_warps.json` | `gates_warps::GateWarpRecord` | 70 |
| 9 | `special_drops.json` | `special_drops::SpecialDropRecord` | 9 |
| 10 | `box_drops.json` | `box_drops::BoxDrop` | 1 |
| 11 | `chaos_mixes.json` | `chaos_mixes::ChaosMix` | 10 |
| 12 | `exp_tables.json` | `exp_tables::ExpTable` | 1 |
| 13 | `game_config.json` | `game_config::GameConfig` | 1 |
| | **Total** | | **2388** |

Plus 11 terrain sidecars (`/data/terrain/0.bin … 10.bin`) — see the note after §13.

---

### 1. `classes.json` — `ClassRecord` (8 records)

The eight playable classes, one record each. **No `"global"` record** — class-scope
config only. `ClassRecord` is parsed through `RawClassRecord` (`try_from`), which
proves the cross-field invariant: `with_command` starting stats appear on the
`dark_lord` record and nowhere else, and Dark Lord's creation energy meets the
command floor of 15. The eight records assemble into `ClassTable` via
`TryFrom<Vec<ClassRecord>>` (all eight classes present exactly once, all class
numbers distinct).

```json
{
  "class": "dark_lord",
  "number": 16,
  "creation": { "kind": "unlocked_at", "level": 250 },
  "evolution": { "kind": "terminal" },
  "home_map": 0,
  "points_per_level": 7,
  "starting_stats": {
    "kind": "with_command",
    "strength": 26, "agility": 20, "vitality": 20, "energy": 15, "command": 25
  },
  "starting_vitals": { "health": 90, "mana": 40, "ability": 1 },
  "fruit_points_divisor": 500,
  "warp_requirement": { "kind": "fraction", "numerator": 2, "denominator": 3 },
  "source_version": "s6",
  "review": "1.0-era backport; warp fraction/rounding pending classic verification; …"
}
```

Shapes:
- `class` — `CharacterClass`, `snake_case` (`dark_wizard`, `soul_master`,
  `dark_knight`, `blade_knight`, `fairy_elf`, `muse_elf`, `magic_gladiator`,
  `dark_lord`). The record key.
- `number` — `ClassNumber` (bare `u8`).
- `creation` — `CreationGate`, kind-tagged: `always` | `unlocked_at { level: Level }`
  | `evolution_only`.
- `evolution` — `Evolution`, kind-tagged:
  `evolves { into: CharacterClass, at_level: Level }` | `terminal`.
- `home_map` — `MapNumber`; Atlas-proven to resolve.
- `points_per_level` — `u8` (5; MG/DL 7).
- `starting_stats` — `StartingStats`, kind-tagged:
  `standard { strength, agility, vitality, energy: u16 }` |
  `with_command { …, command: u16 }` (Dark Lord only).
- `starting_vitals` — `{ health, mana, ability: u16 }`.
- `fruit_points_divisor` — `NonZeroU32` (division by it is total).
- `warp_requirement` — `WarpRequirement`, kind-tagged: `full` |
  `fraction { numerator, denominator: NonZeroU16 }`.
- `source_version`, `review?` — flattened provenance.

---

### 2. `item_definitions.json` — `ItemDefinition` (243 records)

One kind-tagged record per item. Shared columns are the authentic Item.txt-era
facts; everything behavioral lives on the `kind`, flattened so the record carries
`kind` plus the variant's fields inline.

```json
{
  "id": { "group": 0, "number": 0 },
  "name": "Kris",
  "source_version": "075",
  "width": 1, "height": 2,
  "drops_from_monsters": true,
  "drop_level": 6,
  "max_item_level": 11,
  "durability": 20,
  "price": { "kind": "formula" },
  "kind": "weapon",
  "handling": "one_handed",
  "min_damage": 6, "max_damage": 11, "attack_speed": 50,
  "classes": ["dark_wizard","soul_master","dark_knight","blade_knight","fairy_elf","muse_elf","magic_gladiator"],
  "wear": { "level": 0, "strength": 40, "agility": 40, "vitality": 0, "energy": 0, "command": 0 }
}
```

Shared columns: `id` (`ItemRef`, the identity key), `name` (`String`, display only),
flattened provenance, `width`/`height` (`u8` inventory cells), `drops_from_monsters`
(`bool`), `drop_level` (`u8`, base drop-band floor), `max_item_level` (`ItemLevel`),
`durability` (`u8`, raw Item.txt Dur column), `price` (`ItemPrice`:
`fixed { zen: Zen }` | `formula`), then the flattened `kind`.

`classes` is `ClassSet` — a `snake_case` name array (empty array = monster-only /
no class admitted; a duplicated entry is a parse error). `wear` is
`WearRequirements { level, strength, agility, vitality, energy, command: u16 }` (raw
columns; `0` = no requirement). `learn` (orbs/scrolls) is
`LearnRequirements { level, strength, agility, energy: u16 }`.

**`ItemKind` — the 25 variants** (tag `kind`), each carrying exactly its family's
columns:

- `weapon` — `handling` (`one_handed` | `two_handed`), `min_damage`, `max_damage`,
  `attack_speed: u16`, `skill?: SkillNumber`, `classes`, `wear`.
- `bow`, `crossbow` — as `weapon` minus `handling`.
- `arrows`, `bolts` — `classes` only (durability is the round count).
- `staff` — `min_damage`, `max_damage`, `attack_speed`, `magic_power: u16`,
  `skill?`, `classes`, `wear`.
- `shield` — `defense`, `defense_rate: u16`, `skill?`, `classes`, `wear`.
- `helm`, `body_armor`, `pants`, `boots` — `defense: u16`, `classes`, `wear`.
- `gloves` — `defense`, `attack_speed: u16`, `classes`, `wear`.
- `wings` — `tier` (`first` | `second`), `defense`, `absorb_percent: u8`,
  `damage_percent: u8`, `jol_options: [NormalOption]`, `classes`, `wear`.
- `pet` — `ride` (`not_rideable` | `ground_mount` | `flying_mount`),
  `bonuses: [CombatBonus]`, `skill?`, `classes`, `wear`.
- `ring` — `resistance?: Element`, `option: NormalOption`, `classes`, `wear`.
- `pendant` — `resistance?: Element`, `option: NormalOption`,
  **`excellent: ExcellentCategory`** (an object — see below), `classes`, `wear`.
- `transformation_ring` — `skins: [MonsterNumber; 6]` (parsed to the total
  `TransformationSkins`), `classes`, `wear`.
- `orb`, `skill_scroll` — `teaches: SkillNumber`, `learn: LearnRequirements`,
  `classes`.
- `jewel` — `jewel` (`bless` | `soul` | `chaos` | `life` | `creation`).
- `consumable` — `effect: ConsumeEffect`, kind-tagged:
  `healing { tier: apple|small|medium|large }` | `mana { tier: small|medium|large }`
  | `antidote` | `alcohol` | `town_portal`.
- `lucky_box` — tag only.
- `event_ticket` — `event` (`devil_square` | `blood_castle`).
- `mix_material` — tag only.
- `stat_fruit` — tag only.

`NormalOption` (`snake_case`): `physical_damage`, `wizardry_damage`, `defense`,
`defense_rate`, `health_recovery_pct`, `max_mana_pct`, `max_ability_pct`.

**`ExcellentCategory`** (`components/item_options.rs`, tag `set`) — the pendant
`excellent` field is this **object**, not a string:

```json
"excellent": { "set": "weapon", "damage": "wizardry" }
```

`{ "set": "armor" }` | `{ "set": "weapon", "damage": "physical" | "wizardry" }`.

**`CombatBonus`** inline shapes (pet `bonuses`, ancient `set_options`), tag `kind`:
`amount` variants carry `u16`/`u32` (`strength`, `agility`, `vitality`, `energy`,
`command`, `max_health`, `max_mana`, `max_ability`, `ability_recovery`, `defense`,
`defense_rate`, `attack_rate`, `attack_speed`, `physical_damage`,
`min_physical_damage`, `max_physical_damage`, `wizardry_damage`, `skill_damage`,
`damage`, `critical_damage`, `excellent_damage`); `percent` variants carry `Percent`
(`max_health_pct`, `max_mana_pct`, `max_ability_pct`, `health_recovery_pct`,
`defense_pct`, `defense_rate_pct`, `wizardry_damage_pct`,
`two_handed_weapon_damage_pct`, `damage_pct`, `critical_chance_pct`,
`excellent_chance_pct`, `double_damage_chance_pct`, `defense_ignore_chance_pct`,
`incoming_damage_pct`, `damage_reflect_pct`, `zen_drop_pct`); unit variants carry
only their kind (`health_per_kill`, `mana_per_kill`); and the two element variants
(`elemental_resistance { element, amount: Resistance }`,
`elemental_damage { element, amount: u32 }`). Example:
`{ "kind": "incoming_damage_pct", "percent": 20 }`,
`{ "kind": "max_health", "amount": 50 }`.

---

### 3. `ancient_sets.json` — `AncientSet` (36 records)

The ancient-set roster. Pieces reference base items by `ItemRef` (Atlas-proven);
`set_options` are the ordered unlock sequence.

```json
{
  "set_number": 1,
  "name": "Warrior Leather",
  "source_version": "s6",
  "review": "s1-era ancient set backported from s6; ordering pending classic SetItemOption verification",
  "pieces": [
    { "item": { "group": 11, "number": 5 }, "discriminator": 1, "bonus_stat": "vitality" },
    { "item": { "group": 2,  "number": 1 }, "discriminator": 1, "bonus_stat": "strength" }
  ],
  "set_options": [
    { "kind": "strength", "amount": 10 },
    { "kind": "critical_chance_pct", "percent": 5 },
    { "kind": "strength", "amount": 25 }
  ]
}
```

Shapes:
- `set_number` — `SetNumber` (`NonZeroU8`; enforced `>= 1`, roster spans `1..=36`). Review-flagged (ordering
  transcribed from OpenMU's initializer).
- `name` — `String`, display only.
- flattened provenance (`source_version`, `review?`).
- `pieces` — `[AncientSetPiece]`: `item: ItemRef`, `discriminator`
  (`AncientDiscriminator`, wire `1` | `2` — the client's own selector for the ≤2
  ancient sets a base item can belong to), `bonus_stat?`
  (`strength` | `agility` | `vitality` | `energy`; absent = the piece grants no
  per-piece stat bonus).
- `set_options` — `[AncientSetOption]`, in unlock order (k distinct equipped pieces
  unlock the first k−1; the complete set unlocks all). **Untagged**: each entry is
  either a `CombatBonus` (`{ "kind": … }`) or a `ConditionalSetBonus`
  (`{ "kind": "defense_with_shield_pct", "percent": u8 }`). The two `kind`
  namespaces are disjoint, so the untagged split is unambiguous at parse.

The roster loads into `AncientRoster` (`build(Vec<AncientSet>)`, plus
`membership(item, discriminator) -> Option<(&AncientSet, &AncientSetPiece)>`).

---

### 4. `skills.json` — `Skill` (51 records)

The pre-S3 skill roster. `element` and `inflicts` are shipped as explicit `null`
when absent (the parse also accepts omission).

```json
{
  "number": 5,
  "name": "Flame",
  "source_version": "075",
  "attack_damage": 25,
  "damage_type": "wizardry",
  "element": "fire",
  "inflicts": null,
  "range": 6,
  "shape": { "kind": "area", "pattern": "flame" },
  "cost": { "mana": 50, "ability": 0 },
  "learn": { "level": 0, "energy": 160, "command": 0 },
  "classes": ["dark_wizard","magic_gladiator"]
}
```

Shapes:
- `number` — `SkillNumber`; `name` — `String`; flattened provenance.
- `attack_damage` — `u16` (0 for weapon-carried / non-damage skills).
- `damage_type` — `none` | `physical` | `wizardry`.
- `element?` — `Element` (`ice`, `poison`, `lightning`, `fire`, `earth`, `wind`,
  `water`); `null`/absent = non-elemental.
- `inflicts?` — `Ailment` (`poisoned` | `iced` | `frozen` | `defense_reduction`);
  `null`/absent = inflicts nothing.
- `range` — `u8` (0 = caster-centered/self).
- `shape` — `SkillShape`, kind-tagged: `direct_hit`; `lunge`;
  `area { pattern: AreaPattern }`; `buff_self { buff }`; `buff_player { buff }`;
  `buff_party_member { buff }`; `buff_party { buff }`; `heal`;
  `summon { monster: MonsterNumber }`; `teleport`; `nova_charge`; `recall_party`.
- `cost` — `{ mana, ability: u16 }`.
- `learn` — `{ level, energy, command: u16 }`.
- `classes` — `ClassSet` (all-false = monster-only).

`AreaPattern` (18, `snake_case`): `flame`, `twister`, `evil_spirit`, `hellfire`,
`aqua_beam`, `cometfall`, `inferno`, `triple_shot`, `ice_storm`, `nova`,
`twisting_slash`, `rageful_blow`, `death_stab`, `penetration`, `fire_slash`,
`power_slash`, `fire_burst`, `earthshake`. `Buff` (8): `defense`, `greater_damage`,
`greater_defense`, `soul_barrier`, `swell_life`, `critical_damage_increase`,
`infinite_arrow`, `alcohol`. `Buff`/`Ailment` live in `data/effects.rs` — a
Rust-only roster; there is no `magic_effects.json`.

---

### 5. `monster_definitions.json` — `MonsterDefinition` (100 records)

The classic Monster.txt roster: monsters, NPCs, guards, traps, the soccer ball —
one kind-tagged `role`, carrying only the data that kind has.

```json
{
  "number": 0,
  "name": "Bull Fighter",
  "source_version": "075",
  "role": {
    "kind": "monster",
    "combat": { "level": 6, "hp": 100, "min_phys_damage": 16, "max_phys_damage": 20,
                "defense": 6, "attack_rate": 28, "defense_rate": 6 },
    "resistances": { "ice": 0, "poison": 0, "lightning": 0, "fire": 0, "earth": 0, "wind": 0, "water": 0 },
    "behavior": { "move_range": 3, "attack_range": 1, "view_range": 5,
                  "move_delay_ms": 400, "attack_delay_ms": 1600, "respawn_ms": 3000 },
    "attack": { "kind": "plain" }
  }
}
```

```json
{ "number": 235, "name": "Sevina the Priestess", "source_version": "095d",
  "role": { "kind": "npc", "window": "quest" } }
```

Shapes:
- `number` — `MonsterNumber`; `name` — `String`; flattened provenance.
- `role` — `MonsterRole`, kind-tagged:
  - `monster` — `combat`, `resistances`, `behavior`, `attack`.
  - `guard` — `combat`, `resistances`, `behavior` (**no `attack`** — guards use
    plain attacks by rule; the absence is structural).
  - `trap` — `targeting` (`single_when_pressed` | `area_when_pressed` |
    `directional`), `combat`, `resistances`, `behavior`, `attack`.
  - `npc` — `window?` (`merchant` | `vault` | `chaos_machine` | `guild_master` |
    `devil_square` | `quest`; absent = opens nothing).
  - `soccer_ball` — tag only.
- `combat` — `MonsterCombat { level: Level, hp: u32, min_phys_damage, max_phys_damage,
  defense, attack_rate, defense_rate: u16 }`.
- `behavior` — `MobBehavior { move_range, attack_range, view_range: u8,
  move_delay_ms, attack_delay_ms, respawn_ms: DurationMs }`.
- `resistances` — `PerElement<Resistance>`: **all 7 keys required**
  (`ice`, `poison`, `lightning`, `fire`, `earth`, `wind`, `water`, each `u8` 0..=255).
- `attack` — `MonsterAttack`, kind-tagged: `plain` | `skill { skill: SkillNumber }`
  (Atlas-proven; a `summon` skill's monster is also proven).

---

### 6. `spawns.json` — `Spawn` (1847 records)

World population — the classic MonsterSetBase roster. The largest file.

```json
{ "map": 0, "monster": 0,
  "placement": { "kind": "area", "area": { "x1": 135, "y1": 20, "x2": 240, "y2": 88 }, "quantity": 45 },
  "schedule": { "kind": "permanent" },
  "source_version": "075" }
```

```json
{ "map": 0, "monster": 248,
  "placement": { "kind": "fixed", "position": { "x": 6, "y": 145 }, "facing": "south_east" },
  "schedule": { "kind": "wandering" },
  "source_version": "075" }
```

Shapes:
- `map` — `MapNumber`; `monster` — `MonsterNumber` (both Atlas-proven).
- `placement` — `SpawnPlacement`, kind-tagged:
  - `fixed { position: Point, facing: Direction }` — one stationary object (NPCs,
    guard posts, traps, the ball); always one instance.
  - `spot { position: Point, quantity: u16 }` — mobile monsters at one tile.
  - `area { area: Rect, quantity: u16 }` — mobile monsters at random walkable tiles
    in a rectangle.
- `schedule` — `SpawnSchedule`, kind-tagged: `permanent` | `wandering` (at most one
  wandering spawn active world-wide at a time).
- flattened provenance.

---

### 7. `map_definitions.json` — `MapDefinition` (11 records)

One record per game map.

```json
{ "number": 0, "name": "Lorencia", "environment": "ground", "source_version": "075" }
```

```json
{ "number": 6, "name": "Arena", "environment": "ground",
  "soccer_pitch": {
    "ground":     { "x1": 55, "y1": 141, "x2": 69, "y2": 180 },
    "left_goal":  { "x1": 61, "y1": 139, "x2": 63, "y2": 140 },
    "right_goal": { "x1": 61, "y1": 181, "x2": 63, "y2": 182 },
    "left_spawn":  { "x": 60, "y": 156 },
    "right_spawn": { "x": 60, "y": 164 }
  },
  "source_version": "075", "review": "pitch coordinates measurable from Arena terrain; …" }
```

Shapes:
- `number` — `MapNumber` (identity key); `name` — `String`.
- `environment` — `MapEnvironment`: `ground` | `underwater` (Atlans) | `sky`
  (Icarus, entry requires flight).
- `soccer_pitch?` — `SoccerPitch { ground, left_goal, right_goal: Rect,
  left_spawn, right_spawn: Point }`; present only on Arena.
- flattened provenance.

`WalkableGrid` / `TileTerrain` are **deferred** — no JSON grid ships here; the
walkable data is the terrain sidecars (see the note after §13).

---

### 8. `gates_warps.json` — `GateWarpRecord` (70 records)

Gate.txt gate roles and the Move.txt warp list, in one kind-tagged file. Gate kinds
are Gate.txt's flag column (0 = spawn, 1 = enter, 2 = target).

```json
{ "kind": "spawn_gate", "number": 17, "map": 0,
  "area": { "x1": 133, "y1": 118, "x2": 151, "y2": 135 }, "source_version": "075" }
```
```json
{ "kind": "enter_gate", "number": 1, "map": 0,
  "area": { "x1": 121, "y1": 232, "x2": 123, "y2": 233 },
  "target_gate": 2, "min_level": 20, "source_version": "075" }
```
```json
{ "kind": "target_gate", "number": 4, "map": 0,
  "area": { "x1": 121, "y1": 231, "x2": 123, "y2": 231 },
  "direction": "west", "source_version": "075" }
```
```json
{ "kind": "warp", "index": 1, "name": "Arena", "cost_zen": 2000,
  "min_level": 50, "target_gate": 50, "source_version": "075", "review": "…" }
```

Variants (tag `kind`):
- `spawn_gate` — `number: GateNumber`, `map: MapNumber`, `area: Rect`,
  `direction?: Direction`, provenance.
- `target_gate` — same shape as `spawn_gate`; a landing reachable only as a travel
  target.
- `enter_gate` — `number`, `map`, `area`, `target_gate: GateNumber`,
  `min_level?: Level` (absent = unrestricted), provenance.
- `warp` — `index: WarpIndex`, `name: String`, `cost_zen: Zen`, `min_level: Level`,
  `target_gate: GateNumber`, provenance.

`Atlas::parse` proves: gate numbers unique, warp indices unique, every gate's `map`
resolves, every enter/warp `target_gate` resolves to a spawn- or target-gate (never
an enter gate), and Lorencia (map 0) carries a spawn gate (the respawn fallback).

---

### 9. `special_drops.json` — `SpecialDropRecord` (9 records)

Per-fact special drops keyed by the game's own identities. The record is the drop
(`#[serde(flatten)]`) plus flattened provenance.

```json
{ "kind": "level_banded", "item": { "group": 14, "number": 17 }, "chance_per_10000": 100,
  "bands": [ { "min_monster_level": 2, "item_level": 1 }, { "min_monster_level": 36, "item_level": 2 },
             { "min_monster_level": 47, "item_level": 3 }, { "min_monster_level": 60, "item_level": 4 } ],
  "source_version": "095d", "review": "…" }
```
```json
{ "kind": "monster_bound", "monster": 43, "items": [ { "group": 14, "number": 11 } ],
  "item_level": 0, "source_version": "095d" }
```
```json
{ "kind": "map_bound", "map": 10, "min_monster_level": 82, "item": { "group": 13, "number": 14 },
  "item_level": 0, "chance_per_10000": 10, "source_version": "s6", "review": "…" }
```

`SpecialDrop` (tag `kind`):
- `level_banded` — `item: ItemRef`, `chance_per_10000: ChancePer10000`,
  `bands: DropBands` — a non-empty `[{ min_monster_level: Level, item_level: ItemLevel }]`
  with **strictly ascending** thresholds proven at parse (gaps and overlaps
  unrepresentable; the last band is open-ended).
- `monster_bound` — `monster: MonsterNumber`, `items: OneOrMore<ItemRef>` (a JSON
  array, non-empty; a single entry is a fixed drop), `item_level: ItemLevel`.
- `map_bound` — `map: MapNumber`, `min_monster_level: Level`, `item: ItemRef`,
  `item_level: ItemLevel`, `chance_per_10000: ChancePer10000`.

---

### 10. `box_drops.json` — `BoxDrop` (1 record)

Openable-box contents keyed by the box item and its own plus-level.

```json
{
  "box_item": { "group": 14, "number": 11 },
  "box_level": 0,
  "item_roll_per_10000": 5000,
  "items": [ { "group": 0, "number": 3 }, { "group": 0, "number": 5 }, "…66 entries total" ],
  "item_level_range": { "min": 6, "max": 6 },
  "money_fallback": 10000,
  "source_version": "095d",
  "review": "095d Box of Luck: OpenMU fixed level 6 + 50%/10,000-zen split — verify vs classic"
}
```

Shapes:
- `box_item` — `ItemRef`; `box_level` — `ItemLevel` (the box's own plus-level this
  record applies to).
- `item_roll_per_10000` — `ChancePer10000` (chance of an item yield; on failure the
  box yields `money_fallback`).
- `items` — `OneOrMore<ItemRef>` (uniform pick on an item yield).
- `item_level_range` — `ItemLevelRange` = `Interval<ItemLevel>` (`{ min, max }`,
  inclusive, uniform).
- `money_fallback` — `Zen`.
- flattened provenance.

---

### 11. `chaos_mixes.json` — `ChaosMix` (10 records)

The chaos machine's closed recipe catalog. The `kind` **is** the recipe family;
each variant carries its own facts and economics only. **No `number`/`id` field** —
recipes are keyed by family. Provenance is the ChaosMix carve-out (inlined
`source_version` + `review?`, not flattened), so `recipe` is the one nested payload.

```json
{
  "name": "Chaos Weapon",
  "source_version": "075",
  "recipe": {
    "kind": "chaos_weapon",
    "sacrifice_levels": { "min": 4, "max": 11 },
    "weapons": [ { "group": 2, "number": 6 }, { "group": 4, "number": 6 }, { "group": 5, "number": 7 } ]
  }
}
```
```json
{
  "name": "Devil's Square Ticket", "source_version": "095d", "review": "…",
  "recipe": {
    "kind": "devil_square_ticket",
    "eye":        { "group": 14, "number": 17 },
    "key":        { "group": 14, "number": 18 },
    "invitation": { "group": 14, "number": 19 },
    "fee_zen_by_level":         [100000, 200000, 400000, 700000, 1100000, 1600000, 2000000],
    "success_percent_by_level": [80, 80, 80, 80, 70, 70, 70]
  }
}
```

`ChaosRecipe` (tag `kind`) — the 10 families:
- `chaos_weapon` — `sacrifice_levels: Window`, `weapons: [ItemRef; 3]`.
- `first_wings` — `chaos_weapons: [ItemRef; 3]`, `chaos_weapon_levels: Window`,
  `extra_sacrifice_levels: Window`, `wings: [ItemRef; 3]`.
- `second_wings` — `first_wings: [ItemRef; 3]`, `wing_levels: Window`,
  `excellent_levels: Window`, `feather: ItemAtLevel`, `economics: WingEconomics`,
  `wings: [ItemRef; 4]`.
- `cape_of_lord` — `first_wings: [ItemRef; 3]`, `wing_levels: Window`,
  `excellent_levels: Window`, `crest: ItemAtLevel`, `economics: WingEconomics`,
  `cape: ItemRef`.
- `item_upgrade` — `target` (`plus_ten` | `plus_eleven`), `bless: NonZeroU8`,
  `soul: NonZeroU8`, `base_success_percent: Percent`, `fee_zen: Zen`.
- `dinorant` — `horn: ItemRef`, `horn_count: NonZeroU8`, `success_percent: Percent`,
  `fee_zen: Zen`, `dinorant: ItemRef`.
- `fruits` — `catalyst: ItemRef`, `success_percent: Percent`, `fee_zen: Zen`,
  `fruit: ItemRef`.
- `devil_square_ticket` — `eye`, `key`, `invitation: ItemRef`,
  `fee_zen_by_level: [Zen; 7]`, `success_percent_by_level: [Percent; 7]`.
- `blood_castle_ticket` — `scroll`, `bone`, `cloak: ItemRef`,
  `fee_zen_by_level: [Zen; 8]`, `success_percent_by_level: [Percent; 8]`.

Helper shapes: `Window` = `ItemLevelWindow` = `Interval<ItemLevel>` (`{ min, max }`);
`ItemAtLevel` = `{ item: ItemRef, level: ItemLevel }`;
`WingEconomics` = `{ fee_zen: Zen, max_success_percent: Percent,
wing_value_zen_per_percent: Zen, excellent_value_zen_per_percent: Zen,
luck_chance_percent: Percent, excellent_chance_percent: Percent }`. The per-level
ticket arrays are read through `DevilSquareLevel` / `BloodCastleLevel`
(`components/levels.rs`) via `.pick(&array)` — total, no indexing.

---

### 12. `exp_tables.json` — `ExpTable` (1 record)

The experience curve; owns the level cap.

```json
{
  "source_version": "075",
  "max_level": 400,
  "total_exp_by_level": [0, 100, 440, 1080, 2080, "… 400 dense entries, index = level - 1"],
  "review": "one 400-cap curve flattened onto every era — accepted as shipped data"
}
```

Shapes:
- flattened provenance.
- `max_level` — `Level` (the cap).
- `total_exp_by_level` — `[Exp]`, **dense**, `length == max_level`, `index = level − 1`.

Parsed once into `ExpCurve` (`ExpCurve::parse` proves length and
monotonic-non-decreasing totals). Lookup: `curve.level(raw: u16) -> Result<CurveLevel, …>`
mints a `CurveLevel` proven in `1..=max_level`; the total is then read via
`CurveLevel::total_to_hold() -> Exp` (the accessor lives on `CurveLevel`, not
`ExpCurve` — see deviations).

---

### 13. `game_config.json` — `GameConfig` (1 record)

Dataset-scoped configuration of one game edition, grouped by domain concern.

```json
{
  "source_version": "075",
  "tick_duration_ms": 100,
  "item_drop_duration_ms": 60000,
  "drops": {
    "money_roll_per_10000": 5000, "item_roll_per_10000": 3000,
    "jewel_roll_per_10000": 10, "excellent_roll_per_10000": 1, "skill_roll_per_10000": 5000,
    "jewel_drops": [ { "group": 14, "number": 13 }, { "group": 14, "number": 14 }, { "group": 12, "number": 15 } ],
    "review": "…"
  },
  "option_roll": {
    "item_option_roll_per_10000": 2500, "luck_roll_per_10000": 2500,
    "extra_excellent_option_roll_per_10000": 10, "second_wing_bonus_roll_per_10000": 1000,
    "dinorant_option_roll_per_10000": 3000, "max_excellent_options_per_drop": 2,
    "max_dropped_option_level": 3, "review": "…"
  },
  "progression": { "max_party_size": 5, "exp_jitter_percent": { "min": 80, "max": 120 } },
  "zen_caps": { "inventory": 2000000000, "vault": 2000000000 },
  "inventory": {
    "main":           { "rows": 8,  "columns": 8 },
    "vault":          { "rows": 15, "columns": 8 },
    "personal_store": { "rows": 4,  "columns": 8 },
    "trade":          { "rows": 4,  "columns": 8 },
    "chaos_machine":  { "rows": 4,  "columns": 8 }
  },
  "review": "…"
}
```

Shapes:
- flattened provenance.
- `tick_duration_ms` — `TickDuration` (`NonZeroU32`).
- `item_drop_duration_ms` — `DurationMs`.
- `drops` — `DropConfig` (parsed through `RawDropConfig`, which proves
  `money + item + jewel + excellent` numerators sum `<= 10000`; `skill_roll` is a
  separate per-drop chance, not in the sum): five `ChancePer10000` fields
  (`money_roll_per_10000`, `item_roll_per_10000`, `jewel_roll_per_10000`,
  `excellent_roll_per_10000`, `skill_roll_per_10000`), `jewel_drops: OneOrMore<ItemRef>`,
  `review?`.
- `option_roll` — `OptionRollPolicy`: five `ChancePer10000` fields
  (`item_option_roll_per_10000`, `luck_roll_per_10000`,
  `extra_excellent_option_roll_per_10000`, `second_wing_bonus_roll_per_10000`,
  `dinorant_option_roll_per_10000`), `max_excellent_options_per_drop: u8`,
  `max_dropped_option_level: OptionLevel` (bare `1..=4`), `review?`.
- `progression` — `{ max_party_size: u8, exp_jitter_percent: RatePercentRange }`
  where `RatePercentRange = Interval<u16>` (may exceed 100 — a rate, not a
  probability).
- `zen_caps` — `{ inventory, vault: Zen }`.
- `inventory` — `InventoryGeometry`: five `GridSize { rows, columns: u8 }` fields
  (`main`, `vault`, `personal_store`, `trade`, `chaos_machine`).

(`EquipmentSlot` — the 12 classic slots — is a Rust enum in `game_config.rs`, used
by future host/services; it carries no JSON in this file.)

---

### Terrain sidecars (host-parsed, not JSON records)

`/data/terrain/<map>.bin` — **11 files** (`0.bin` … `10.bin`, keyed by
`MapNumber`), each exactly **65 536 bytes = 256 × 256** raw walkability/attribute
bytes for that map's tile grid. These are **not** part of the 2388 JSON records and
have no core record type in v2. The runtime walkable grid (`WalkableGrid` /
`TileTerrain`) is a **deferred host-parsed structure** consumed only by future
spawning/travel services; no total `u8 × u8` accessor exists without index-slicing,
so it is a named scope boundary, not debt.

---

## Moved to Rust / services boundary

What left the data layer entirely, and where it lives now.

**Built and green:**
- **Enhancement / durability / staff-rise curves** → `services/item_rules.rs`. Named
  `const [T; 12]` families read through an exhaustive match over `EnhanceLevel`
  (total, no indexing): `weapon_damage_bonus`, `armor_defense_bonus`,
  `shield_defense_bonus`, `shield_defense_rate_bonus`, `wing_defense_bonus`,
  `staff_rise_x2` (doubled to carry the client's half-point steps integer-exact),
  `wing_absorb_percent`, `wing_damage_percent`, `jewelry_resistance`,
  `max_durability`, plus `ammunition_damage_percent(AmmoLevel)`.
- **`effective_drop_level`** → `services/item_rules.rs`, the **one home** of the
  classic rule `drop_level + 3·item_level (+25 excellent / +30 ancient)`, total over
  `EnhanceLevel` × `ItemRarity`.
- **The chance seam** → `services/chance.rs`: `roll_per_10000`, `roll_percent`,
  `roll_resistance`, `WeightedTable<T>` (derived total, held-apart last bucket) +
  `weighted_pick`, `pick_one` over `OneOrMore`. Every draw goes through
  `rng::uniform_below` (Lemire, cast-free); there is no `% next_u32` anywhere.
- **`Buff` / `Ailment`** → `data/effects.rs`, Rust-only rosters (killed
  `magic_effects.json`).
- **`CombatBonus` / `ConditionalSetBonus`** → `components/bonus.rs`; the level-key
  enums → `components/levels.rs`; `ItemRarity` → `components/item_quality.rs`;
  `PerElement<T>` / `Element` → `components/element.rs`; `ClassSet` /
  `CharacterClass` → `components/class.rs`; the item-option vocabulary
  (`NormalOption`, `ExcellentCategory`, `Excellent{Armor,Weapon}Option`,
  `DinorantOption`, `SecondWingBonus`, `AncientBonusLevel`) →
  `components/item_options.rs`.

**Explicitly future wave (not written — no bodyless signatures, no `todo!`):**
- **The per-class stat model and derived-stat formulas.** `stats.json` and the
  evaluator are deleted *now*; the typed replacement (`components/stats.rs`
  `ClassStats`/`CoreStats`/`LordStats`, and `services/stats/` exhaustive-match
  formula functions with named coefficients) is designated Rust but belongs to
  **W-ENT** — it does not exist in the tree yet.
- **Combat / drop / craft resolution** and the option/set resolvers, aggregate
  stats, travel, spawning, progression, character creation, warp, buff/effect
  magnitudes → **W-CMB / W-ENT**. `entities/` and `events/` are doc-only
  placeholder modules.
- **Drop pools** (`PoolEntry`, `DropPools`, `SpecialDropTable`, `BoxTable`,
  `DropContents`, `DropOutcome`, …) — their constructor and consumers are
  drop-resolution, → **W-CMB / W-ENT**. The drop **data** record types
  (`DropConfig`, `SpecialDropRecord` + `DropBands`, `BoxDrop` + `ItemLevelRange`)
  are shipped and Atlas-checked.
- **Runtime entities**, the **terrain grid** (`WalkableGrid` / `TileTerrain`), and
  the terrain-sidecar parser.

---

## Deviations from the design draft

Where the built shapes differ from the v2 design sections
(`scratchpad/v2-sections-r3/`), the code wins. The material deviations:

| # | Design-draft shape | As-built shape | Why |
|---|---|---|---|
| 1 | Envelope `{ "schema_version": 2, "records": […] }` | `{ "records": […] }` — **no `schema_version`** | Version negotiation is a host-boundary concern, not a core data fact; `DataFile<T>` carries only `records`. |
| 2 | Chaos item-level windows `{ min_level, max_level }` | `{ "min", "max" }` (via the one `Interval<T>`) | Window is just an inclusive interval; unified onto `components/interval.rs::Interval` so every bounded range shares one guarded type. |
| 3 | Pendant `excellent` a string (e.g. `"wizardry_weapon"`) | `ExcellentCategory` **object** `{ "set": "weapon", "damage": "wizardry" }` \| `{ "set": "armor" }` | Nesting the damage kind on the weapon variant makes the armor/weapon pairing illegal-state-unrepresentable; locked by test. |
| 4 | `ItemRarity` shown in the items domain | Homed in `components/item_quality.rs` | It is the quality-band value the drop-level and durability rules key off — a shared component, not item-file data. |
| 5 | Per-range bespoke min/max types across domains | One generic `Interval<T>` (`ItemLevelRange`, `ItemLevelWindow`, `RatePercentRange`) | Single guarded range type; `min <= max` proven once. |
| 6 | Nested `provenance` object | Flattened `source_version` + `review?` at record top level; **ChaosMix inlines** the pair as named fields | Flatten keeps records shallow; the ChaosMix carve-out keeps `recipe` the one nested payload. |
| 7 | `ExpCurve::total_to_hold(&self, CurveLevel)` | `CurveLevel::total_to_hold(self)` (total resolved at mint by `curve.level(raw)`) | Moves the total off `ExpCurve` so it is index-free and lint-clean (no `unused_self`); `CurveLevel` carries `(Level, Exp)` resolved once. |
| 8 | `WalkableGrid` / `TileTerrain` in `map_definitions` | **Deferred**; `map_definitions.rs` ships `MapDefinition` / `MapEnvironment` / `SoccerPitch` only; walkability is the 11 terrain `.bin` sidecars, host-parsed | A total `u8 × u8` grid accessor cannot exist without index-slicing; named scope boundary for the future travel/spawning services (W-ENT), not debt. |
