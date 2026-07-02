# Debt record: OpenMU-invented default values in the v2 dataset

- **ID:** W-SRC (data provenance re-sourcing)
- **Status:** OPEN
- **Owner wave:** W-SRC (source-verification pass)
- **Created:** 2026-07-03, during the Wave R3 v2-data integration.
- **Scope:** every `/data/*.json` value that ships with a `review` string because it
  is an OpenMU default, an OpenMU modeling artifact, or an era-doubtful backport —
  not a value scraped verbatim from an authentic pre-Season-3 client file.

## Why this is debt, not a bug

The Iron Rule for extraction is *preserve every authentic number, launder nothing*.
Where OpenMU is the only available source and its value is a modern default (or a
1.0-era / Season-6 backport reused for a pre-S3 baseline), the number is shipped
**as-is** and flagged in the record's `review` field rather than silently trusted or
silently dropped. The flag is the debt marker: it names the doubt and defers
resolution to W-SRC, which will diff each family against an authentic classic source
(Monster.txt / Skill.txt / Item*.txt / Gate.txt / GS config for 0.75 and 0.95d) and
either confirm the value (drop the flag) or replace it (re-extract).

`review` is `Option<String>` on every record's `Provenance` (`core/src/data/common.rs`)
and never read by any service — it is documentation-for-humans, invisible to the
simulation. Removing a flag is a data-only edit at the owning extractor; it never
touches Rust.

## Scale

185 `review` strings across the 13 v2 files (2388 records). By file:

| File | review strings | note |
|---|---:|---|
| `monster_definitions.json` | 48 | water→lightning remap (all 48); phantom skill-150 (14 ⊂ 48); golden-era doubt (2 ⊂ 48) |
| `item_definitions.json` | 41 | 33 S6 backports + 8 durability-3 potions |
| `ancient_sets.json` | 36 | every set: S6-transcribed ordering |
| `skills.json` | 17 | 16 S6 backports + 1 cometfall AoE-encoding doubt (095d) |
| `gates_warps.json` | 14 | 095d warp fee/level list reused verbatim from 0.75 |
| `classes.json` | 8 | every class: ability seed + fruit divisor + (MG/DL) warp fraction; 4 are S6 second tiers |
| `special_drops.json` | 7 | chances + band edges; box-encoding + Loch's Feather/Crest era doubt |
| `chaos_mixes.json` | 6 | crafting economics + counts + success splits |
| `game_config.json` | 3 | drop rates; option-roll chances; exp jitter + personal store (record + 2 nested sections) |
| `map_definitions.json` | 3 | Devias terrain tag; Arena pitch placement; Devil Square collapse |
| `exp_tables.json` | 1 | one 400-cap curve flattened onto every era |
| `box_drops.json` | 1 | Box of Luck fixed level 6 + 50%/10k-zen split |
| `spawns.json` | 0 | — |

## Value families to re-source

Each family is a distinct doubt with one owning extractor. W-SRC resolves them
independently; resolving one never blocks another.

### A. Character creation defaults — `classes.json` (extractor `classes.py`)
1. **Starting ability = 1** on all 8 classes. OpenMU initializer seed, not a scraped
   classic value. *W-SRC:* confirm the classic creation AG for each class.
2. **Fruit points divisor** (DW/DK 400, elves 700, MG/DL 500). Encodes OpenMU's
   `FruitCalculationStrategy`; not a plain scraped int. *W-SRC:* derive the authentic
   per-class fruit cap curve.
3. **Warp requirement fraction 2/3** on Magic Gladiator and Dark Lord. Re-expression of
   OpenMU's `LevelWarpRequirementReductionPercent = ceil(100/3) = 34`. *W-SRC:* verify
   the classic reduction and its rounding direction.
4. **S6 second-tier creation values** (Soul Master, Blade Knight, Muse Elf, Dark Lord)
   mirror their base tier — second tiers are absent from 075/095d. *W-SRC:* source
   authentic second-tier creation stats if/when the era is confirmed in scope.

### B. Item modeling + S6 backports — `item_definitions.json` (extractor `items.py`)
5. **Potion durability = 3** on 8 consumables (Apple, 3 healing, 3 mana, Antidote).
   OpenMU stack-size modeling; classic pre-S3 potions were single items. *W-SRC:*
   confirm classic stack behavior; likely re-model to durability 1.
6. **S6-dataset base items** shipped only as ancient-set pieces / mix results: Storm
   Crow (MG) armor, Adamantine (DL) armor, Red Wing (Summoner) armor with empty
   `classes`, S6 jewelry (rings of wind/poison/earth/fire/magic, pendants of
   ice/water/ability), 2nd wings, Cape of Lord, stat fruit, Blood Castle ticket, the
   0.97d-tagged-s6 jewel. *W-SRC:* confirm each item's authentic era and stats.
7. **Wing bonus percentages** — second wings dmg +32% / absorb 25% / +1% per level;
   first-wing Cape dmg +20% / absorb 10% / +2% per level; `max_item_level` clamped
   15→11. S6 `Wings.cs` values. *W-SRC:* source authentic classic wing curves.

### C. Monster combat modeling — `monster_definitions.json` (extractor `monsters.py`)
8. **Lightning resistance transcribed from OpenMU's water-resistance slot** (all 48
   reviewed monsters). OpenMU models no lightning column pre-S3; the water byte is
   re-homed to lightning. *W-SRC:* verify each resistance byte against classic
   Monster.txt.
9. **Phantom skill-150 attack modeled as `plain`** (14 monsters). OpenMU maps the
   attack to S6 skill 150, which has no pre-S3 existence. *W-SRC:* confirm the classic
   attack type per monster.
10. **Golden-era doubt** — Golden Titan (#53) and #54 look ~0.97+, later than the pre-S3
    baseline. *W-SRC:* confirm era eligibility.

### D. Skills — `skills.json` (extractor `skills-effects.py`)
11. **16 S6-backported skills** (Blade Knight / Soul Master / Muse Elf / Magic Gladiator
    / Dark Lord 0.97–1.0 content) with values from the S6 initializer, plus the
    **cometfall AoE-encoding doubt** (095d marks it a plain direct hit yet attaches AoE
    settings). Open sub-doubts noted inline: Soul Barrier party-vs-self targeting; Nova
    (40) + Nova Charge (58) split. *W-SRC:* source authentic classic Skill.txt values and
    targeting.

### E. Maps — `map_definitions.json` (extractor `maps.py`)
12. **Devias tagged 095d** for its 0.95 terrain sidecar though its record fields are pure
    0.75; **Arena soccer pitch** placed in the 0.75 era per OpenMU (feature historically
    ~0.97+); **Devil Square** collapsed from 4 OpenMU discriminator records into one map.
    *W-SRC:* confirm era placement for each.

### F. Warps — `gates_warps.json` (extractor `maps.py`)
13. **095d warp fee/level list reused verbatim from 0.75** (all 14 warps). OpenMU's
    initializer carries a TODO to update it; no authentic 0.95 table sourced. *W-SRC:*
    source the classic 0.95d warp fee/level table.

### G. Ancient sets — `ancient_sets.json` (extractor `options-sets.py`)
14. **Set-number ordering 1..36 transcribed from OpenMU's S6 initializer** (all 36 sets),
    pending verification against a classic `SetItemOption` client file. Kantata (set 9)
    additionally carries a fixed OpenMU data-bug note (excellent-damage-chance
    10.0 → 0.10). *W-SRC:* verify ordering + the Kantata correction against a classic
    source.

### H. Special drops — `special_drops.json` (extractor `drops.py`)
15. **EventItemBag banding** — the 1% chance and band edges are OpenMU 095d initializer
    values; **Loch's Feather / Crest of Monarch** 0.1% chance and level floor 82 are S6
    Icarus values; the **Box of Kundun+2-as-Box-of-Luck level-9 encoding** carries a
    Golden Titan era doubt. *W-SRC:* source authentic drop chances and level floors.

### I. Box drops — `box_drops.json` (extractor `drops.py`)
16. **Box of Luck** contents fixed at level 6 with a 50% / 10,000-zen item-or-money
    split. OpenMU defaults. *W-SRC:* verify the fixed level and split against classic
    behavior.

### J. Chaos crafting — `chaos_mixes.json` (extractor `chaos.py`)
17. **2nd-wings / Cape value-per-percent 4,000,000 / 40,000 + luck/excellent 20/20**;
    **Dinorant horn_count 3 & fee 250,000** (classic is 10 full-durability Horns / 500,000);
    **Devil's Square** 7-level fee band + 80/70 success split; **Blood Castle** 8-level
    fee band + flat 80% success; **Fruits** created-stat kind is a weighted services roll
    (`FRUIT_STAT_WEIGHTS`, OpenMU defaults). *W-SRC:* source authentic classic recipe
    economics and level breadths.

### K. Experience curve — `exp_tables.json` (extractor `constants-exp.py`)
18. **One 400-cap curve flattened onto every era.** OpenMU applies a single curve to
    0.75/0.95d whose historical caps were lower. *W-SRC:* source the authentic per-era
    caps and curves.

### L. Game config defaults — `game_config.json` (extractor `constants-exp.py`)
19. **Drop category rates** (money 0.5 / item 0.3 / jewel 0.001 / excellent 0.0001 from
    `GameConfigurationInitializerBase`; skill 50% from `DefaultDropGenerator`);
    **option-roll chances** + the 2-excellent cap + `max_dropped_option_level = 3`;
    **exp jitter 0.8–1.2** (OpenMU uniform-jitter mechanism, no classic corroboration);
    **personal_store** (~1.0-era feature). *W-SRC:* source authentic classic drop and
    option-roll policy.

## Resolution plan (W-SRC)

For each family above:
1. Locate an authentic classic source for the 0.75 / 0.95d baseline (client data file
   or documented GS config), independent of OpenMU.
2. If the value matches: delete the `review` string at the owning extractor and re-run it.
3. If it differs: correct the value at the extractor, re-run, and re-run
   `tools/extract/validate_refs.py` + `cargo test --test data_files` to confirm the
   dataset still parses and cross-resolves.
4. If the value is genuinely unsourceable and stays an accepted default: keep the flag
   and record the decision here — a shipped default with a standing flag is acceptable;
   a shipped default with **no** flag is not.

Debt is discharged when the last `review` string tied to an OpenMU default is either
removed (confirmed/corrected) or explicitly accepted-and-recorded here.
