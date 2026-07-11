#!/usr/bin/env python3
"""Extract exp_tables.json + game_config.json (v2 schema, section constants_exp).

Sources (all numbers verified against reference/openmu):
  Persistence/Initialization/GameConfigurationInitializerBase.cs
      MaximumLevel, ItemDropDuration, MaximumItemOptionLevelDrop,
      MaximumPartySize, RandomExperience{Min,Max}Multiplier, the four default
      DropItemGroup chances (money/item/excellent/jewel), zen-cap sentinels,
      the item-option AddChance default.
  GameLogic/DefaultDropGenerator.cs        SkillDropChancePercent.
  DataModel/InventoryConstants.cs          storage-grid geometry (rows x cols).
  Items/ExcellentOptions.cs                extra-excellent-slot AddChance.
  Version075/Items/Jewelery.cs             luck AddChance.

v2 shape (locked by the R2 Rust serde types; see R3-json-contract.md and the
constants_exp / drops / options design sections):
  exp_tables.json  -> ExpTable: max_level + dense total_exp_by_level
      (index = level-1, levels 1..=max_level; the v1 `formula` string field and
       the two padding cells are gone).
  game_config.json -> GameConfig: one FULL record. Two owned top-level
      durations (tick_duration_ms ours, item_drop_duration_ms authentic) plus
      typed per-domain sub-structs: `drops` (DropConfig, drops-owned shape),
      `option_roll` (OptionRollPolicy, options-owned shape), progression,
      zen_caps, inventory. The nested `drops` / `option_roll` sections carry
      ONLY a `review` string (no source_version).

Approved non-OpenMU values (decisions, carried in `notes`/`review`, not silently
relabeled authentic):
  tick_duration_ms = 100          ours (decision 4); OpenMU has no tick base.
  zen_caps = 2_000_000_000        classic cap (decision 7); OpenMU int.MaxValue.

OpenMU-invented values survive verbatim but every one carries a `review` string
naming it an OpenMU default pending an authentic source: the four drop category
rates + skill roll (drops section review), the three drop-time option-roll
rates + the 2-excellent cap (option_roll section review), and the exp jitter
range + personal-store grid (record review). The exp curve's flat 400 cap over
the 0.75/0.95d eras carries its own review on the exp record.

The two chaos-machine chances this section once carried
(second_wing_bonus_roll_per_10000, dinorant_option_roll_per_10000) are retired
(W-CRAFT): both were consumer-less and shadowed their authoritative homes —
the chaos_mixes WingEconomics record's luck/excellent chances and the dinorant
family facts in the craft service. One number, one home.

All base-initializer values are identical across Version075/095d/S6 (shared
base initializer, no per-version override) -> source_version "075" on both.
"""

import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import coverage, item_ref, write_datafile, OPENMU_ROOT

OPENMU = OPENMU_ROOT + "/src"

GAME_CFG = "Persistence/Initialization/GameConfigurationInitializerBase.cs"
DROP_GEN = "GameLogic/DefaultDropGenerator.cs"
INVENTORY = "DataModel/InventoryConstants.cs"
EXC_OPTS = "Persistence/Initialization/Items/ExcellentOptions.cs"
LUCK_OPTS = "Persistence/Initialization/Version075/Items/Jewelery.cs"


def read(rel):
    return open(os.path.join(OPENMU, rel), encoding="utf-8").read()


def grab(pattern, text, where):
    m = re.search(pattern, text)
    if not m:
        sys.exit("pattern not found in %s: %s" % (where, pattern))
    return m.group(1)


def parse_game_config(text):
    """this.GameConfiguration.<Prop> = <expr>;  -> {Prop: raw_expr}"""
    return dict(re.findall(r"this\.GameConfiguration\.(\w+) = (.+?);", text))


def parse_global_base_values(text):
    """ConstValueAttribute>(<value>f, Stats.<Prop>...) -> {Prop: float}"""
    pairs = re.findall(
        r"ConstValueAttribute>\(([0-9.]+)f?,\s*Stats\.(\w+)\.GetPersistent", text)
    return {prop: float(value) for value, prop in pairs}


def parse_consts(text):
    """C# const/static readonly numeric fields -> {Name: number}"""
    out = {}
    for name, value in re.findall(
            r"(?:const|static readonly)\s+\w+\s+(\w+)\s*=\s*([0-9.]+)f?\s*;", text):
        out[name] = float(value) if "." in value else int(value)
    return out


def parse_drop_group_chances(text):
    """<name>DropItemGroup.Chance = <fraction>;  -> {name: float}"""
    return {name: float(v)
            for name, v in re.findall(r"(\w+?)DropItemGroup\.Chance = ([0-9.]+);", text)}


def expect(actual, wanted, what):
    if actual != wanted:
        sys.exit("%s: expected %r, source has %r" % (what, wanted, actual))
    return actual


def per_10000(fraction, what):
    """Convert an OpenMU fraction to a ChancePer10000 numerator, exactly."""
    scaled = fraction * 10_000
    numerator = round(scaled)
    if abs(scaled - numerator) > 1e-9:
        sys.exit("%s: %r does not convert exactly to a per-10000 numerator" % (what, fraction))
    if not 0 <= numerator <= 10_000:
        sys.exit("%s: per-10000 numerator %d out of range" % (what, numerator))
    return numerator


def expect_literal(needle, text, where):
    """The source fraction must literally appear in its cited file."""
    if needle not in text:
        sys.exit("%s: expected literal %r in source" % (where, needle))


def total_exp(level):
    """Exact integer two-piece Webzen curve (CalculateNeededExperience).

    total(level) = 10*(level+8)*(level-1)^2  for level < 256,
    plus          1000*(level-247)*(level-256)^2  for level >= 256.
    total(1) = 0.
    """
    base = 10 * (level + 8) * (level - 1) * (level - 1)
    if level < 256:
        return base
    return base + 1000 * (level - 247) * (level - 256) * (level - 256)


def build_exp_table(max_level):
    """Dense total-exp curve, index = level-1, levels 1..=max_level."""
    table = [total_exp(level) for level in range(1, max_level + 1)]
    expect(len(table), max_level, "exp table length")
    expect(table[:10], [0, 100, 440, 1080, 2080, 3500, 5400, 7840, 10880, 14580],
           "exp table head (levels 1..10)")
    expect(table[-1], 3_822_148_080, "exp table tail (exp to hold level 400)")
    if any(b < a for a, b in zip(table, table[1:])):
        sys.exit("exp table is not monotonic")
    if table[-1] >= 2 ** 64:
        sys.exit("exp table overflows u64")
    return table


def build_drops_section(cfg_text, drop):
    """DropConfig — drops-owned shape; kill-scoped category rates + jewel roster.

    Per-drop option caps and the option/luck/excellent rolls are NOT here: they
    have one home, the option_roll section. Nested section carries review only.
    """
    chances = parse_drop_group_chances(cfg_text)
    money = per_10000(chances["money"], "money DropItemGroup.Chance")
    item = per_10000(chances["randomItem"], "randomItem DropItemGroup.Chance")
    excellent = per_10000(chances["excellentItem"], "excellentItem DropItemGroup.Chance")
    jewel = per_10000(chances["jewels"], "jewels DropItemGroup.Chance")
    skill = per_10000(drop["SkillDropChancePercent"] / 100.0, "SkillDropChancePercent")

    expect(money, 5000, "money_roll_per_10000")
    expect(item, 3000, "item_roll_per_10000")
    expect(jewel, 10, "jewel_roll_per_10000")
    expect(excellent, 1, "excellent_roll_per_10000")
    expect(skill, 5000, "skill_roll_per_10000")
    # DropConfig parse proves the four category numerators sum <= 10,000.
    expect(money + item + jewel + excellent <= 10_000, True, "category sum <= 10000")

    return {
        "money_roll_per_10000": money,
        "item_roll_per_10000": item,
        "jewel_roll_per_10000": jewel,
        "excellent_roll_per_10000": excellent,
        "skill_roll_per_10000": skill,
        # Jewel of Bless 14/13, Jewel of Soul 14/14, Jewel of Life 12/15.
        # 075 roster: no Jewel of Chaos.
        "jewel_drops": [item_ref(14, 13), item_ref(14, 14), item_ref(12, 15)],
        "review": (
            "all rates are OpenMU defaults (categories 0.5/0.3/0.001/0.0001 from "
            "GameConfigurationInitializerBase; skill 50% from DefaultDropGenerator) "
            "- authentic model is per-monster ItemRate/MoneyRate (Monster.txt) x "
            "global ItemDropRate/ZenDropRate (CommonServer.cfg); replace when "
            "classic sources land"),
    }


def build_option_roll_section(cfg_text, cfg):
    """OptionRollPolicy — options-owned shape; the drop-time option rolls +
    both caps. Chaos-machine chances do NOT live here (retired, W-CRAFT):
    their homes are the chaos_mixes WingEconomics record and the dinorant
    family facts in the craft service.

    Chances are per-item AddChance defaults; each source fraction is verified
    present in its cited file, then converted exactly. Nested section carries
    review only.
    """
    item_option = per_10000(0.25, "item option AddChance")   # GameConfigInit:115
    luck = per_10000(0.25, "luck AddChance")                 # 075 Jewelery:125
    extra_excellent = per_10000(0.001, "excellent AddChance")  # ExcellentOptions:66

    expect_literal("AddChance = 0.25f", cfg_text, "item option AddChance in GameConfigInit")
    expect_literal("AddChance = 0.25f", read(LUCK_OPTS), "luck AddChance in 075 Jewelery")
    expect_literal("AddChance = 0.001f", read(EXC_OPTS), "excellent AddChance in ExcellentOptions")

    expect(item_option, 2500, "item_option_roll_per_10000")
    expect(luck, 2500, "luck_roll_per_10000")
    expect(extra_excellent, 10, "extra_excellent_option_roll_per_10000")

    # max_dropped_option_level: authentic (cross-checked against the config
    # scalar); knob framing killed, cap kept as OptionLevel policy.
    max_option_level = expect(int(cfg["MaximumItemOptionLevelDrop"]), 3,
                              "MaximumItemOptionLevelDrop")

    return {
        "item_option_roll_per_10000": item_option,
        "luck_roll_per_10000": luck,
        "extra_excellent_option_roll_per_10000": extra_excellent,
        # OpenMU excellent-option MaximumOptionsPerItem = 2.
        "max_excellent_options_per_drop": 2,
        "max_dropped_option_level": max_option_level,
        "review": (
            "the three drop-time chances and the 2-excellent cap are OpenMU "
            "initializer defaults pending authentic classic sources; "
            "max_dropped_option_level 3 is commonly held classic knowledge "
            "stated here as policy"),
    }


def build_game_config(cfg, cfg_text, drop, inv, glob):
    item_drop_ms = 1000 * int(grab(
        r"TimeSpan\.FromSeconds\((\d+)\)", cfg["ItemDropDuration"], "ItemDropDuration"))
    expect(item_drop_ms, 60000, "item_drop_duration_ms")

    exp_min = int(round(expect(glob["RandomExperienceMinMultiplier"], 0.8,
                               "RandomExperienceMinMultiplier") * 100))
    exp_max = int(round(expect(glob["RandomExperienceMaxMultiplier"], 1.2,
                               "RandomExperienceMaxMultiplier") * 100))
    expect((exp_min, exp_max), (80, 120), "exp_jitter_percent")

    # Zen caps: OpenMU stores int.MaxValue; decision 7 restores the classic
    # 2,000,000,000 cap. Verify the sentinel, then override.
    expect(cfg["MaximumInventoryMoney"], "int.MaxValue", "MaximumInventoryMoney")
    expect(cfg["MaximumVaultMoney"], "int.MaxValue", "MaximumVaultMoney")
    zen_cap = 2_000_000_000

    row_size = expect(inv["RowSize"], 8, "RowSize")
    grid = lambda rows, cols: {"rows": rows, "columns": cols}

    return {
        "source_version": "075",
        "tick_duration_ms": 100,  # ours (decision 4), not an OpenMU value
        "item_drop_duration_ms": item_drop_ms,
        "drops": build_drops_section(cfg_text, drop),
        "option_roll": build_option_roll_section(cfg_text, cfg),
        "progression": {
            "max_party_size": expect(int(cfg["MaximumPartySize"]), 5, "MaximumPartySize"),
            "exp_jitter_percent": {"min": exp_min, "max": exp_max},
        },
        "zen_caps": {"inventory": zen_cap, "vault": zen_cap},
        "inventory": {
            "main": grid(expect(inv["InventoryRows"], 8, "InventoryRows"), row_size),
            "vault": grid(expect(inv["WarehouseRows"], 15, "WarehouseRows"), row_size),
            "personal_store": grid(expect(inv["StoreRows"], 4, "StoreRows"), row_size),
            # OpenMU's unified TemporaryStorage splits into the two distinct
            # client windows that share 4x8: trade and chaos_machine.
            "trade": grid(expect(inv["TemporaryStorageRows"], 4, "TemporaryStorageRows"), row_size),
            "chaos_machine": grid(inv["TemporaryStorageRows"], row_size),
        },
        "review": (
            "exp_jitter_percent: OpenMU uniform-jitter mechanism and 0.8-1.2 "
            "values, no classic corroboration - pending classic GS verification; "
            "personal_store: ~1.0-era feature, curated backport per decision 1"),
    }


def verify_written(path, expected_records):
    with open(path, encoding="utf-8") as f:
        doc = json.load(f)
    count = len(doc.get("records", []))
    if count != expected_records:
        sys.exit("%s: %d records, expected %d" % (path, count, expected_records))
    return count


def main():
    cfg_text = read(GAME_CFG)
    cfg = parse_game_config(cfg_text)
    glob = parse_global_base_values(cfg_text)
    drop = parse_consts(read(DROP_GEN))
    inv = parse_consts(read(INVENTORY))

    max_level = expect(int(cfg["MaximumLevel"]), 400, "MaximumLevel")
    table = build_exp_table(max_level)
    exp_record = {
        "source_version": "075",
        "max_level": max_level,
        "total_exp_by_level": table,
        "review": (
            "curve and 400 cap applied to every era per decision 6 - OpenMU "
            "flattens one curve onto 0.75/0.95d whose historical caps were lower; "
            "accepted as shipped data"),
    }

    game_config = build_game_config(cfg, cfg_text, drop, inv, glob)

    exp_path = write_datafile("exp_tables.json", [exp_record])
    cfg_path = write_datafile("game_config.json", [game_config])

    exp_count = verify_written(exp_path, 1)
    cfg_count = verify_written(cfg_path, 1)

    gaps = {
        "exp_tables.formula": "v1 formula string killed; curve content is the "
        "ExpTable doc comment, table stays authoritative",
        "recovery_interval_ms": "-> services vitals::REGEN_PULSE_INTERVAL_MS (3000 ms), review-flagged",
        "info_range": "-> services world::VIEW_RANGE_TILES (15); OpenMU's 12 rejected",
        "base_money_drop": "-> drops services BASE_MONEY_DROP=7, review-flagged (not data)",
        "drop_level_max_gap": "-> drops services DROP_POOL_LEVEL_GAP=12, review-flagged (not data)",
        "excellent_drop_level_delta": "-> items services EXCELLENT_DROP_LEVEL_BONUS=25 (not data)",
        "max_option_level": "jewel ceiling +16 -> item-options domain (not this file)",
        "minimum_hit_chance/overrate_damage_factor": "-> services combat consts",
        "min_damage/double_wield/duel/dinorant factors": "-> services combat consts (W-CMB)",
        "per_kill_exp_x1.25": "-> services progression::KILL_EXP_FLAT_BONUS_PERCENT=125",
        "money_amount_rate/should_drop_money/clamp_money_on_pickup": "deleted (fixed classic rules / no-op knobs)",
        "prevent_experience_overflow/area_skill_hits_player": "deleted (fixed classic rules / typed combat context)",
        "character_name_regex/max_characters_per_account": "deleted (host parse-boundary / no Account aggregate in core)",
        "durability economy (2000/100000/10000)": "deleted (OpenMU accumulator wear model; W-CMB sources fresh)",
        "movement struct (speeds, running-gear level)": "deleted; iced 0.5 -> skills_effects ICED_SPEED_REDUCTION_PERCENT=50",
        "iced_speed_factor": "-> skills_effects domain (not this file)",
    }
    reviews = {
        "exp_tables[075]": exp_record["review"],
        "game_config[075].review": game_config["review"],
        "game_config[075].drops.review": game_config["drops"]["review"],
        "game_config[075].option_roll.review": game_config["option_roll"]["review"],
    }
    notes = [
        "exp table trimmed 402 -> 400 entries (index = level-1, levels 1..=400); "
        "OpenMU's two padding cells (index-0 and the +1 read guard) have no v2 reader",
        "v1 spec sample total_exp_by_level level 3 = 396 was a typo; authentic value 440 (formula-correct)",
        "tick_duration_ms 100 is a mu-core decision (4), not an OpenMU value",
        "zen_caps 2000000000 override decision 7; OpenMU source has int.MaxValue",
        "nested drops/option_roll sections carry review only, no source_version "
        "(provenance is the enclosing record's source_version)",
        "option_roll shape is options-owned, drops shape is drops-owned; this "
        "extractor owns the game_config.json envelope/record around them",
        "second_wing_bonus_roll_per_10000 and dinorant_option_roll_per_10000 "
        "retired (W-CRAFT): both were consumer-less chaos-machine chances "
        "shadowing their authoritative homes — the chaos_mixes WingEconomics "
        "record's luck/excellent chances and the dinorant family facts in the "
        "craft service; one number, one home",
    ]
    cov = {
        "files": [exp_path, cfg_path],
        "records": exp_count + cfg_count,
        "by_source_version": {"075": exp_count + cfg_count},
        "review_count": exp_count + cfg_count,  # both records carry a review
        "reviews": reviews,
        "gaps": gaps,
        "notes": notes,
    }
    cov_path = coverage("constants_exp", cov)

    print("wrote %s (%d record, %d exp entries, max %d)" % (
        exp_path, exp_count, len(table), table[-1]))
    print("wrote %s (%d record: full GameConfig)" % (cfg_path, cfg_count))
    print("wrote %s (%d records total, by_source_version=%s, %d reviews, %d gaps)" % (
        cov_path, cov["records"], cov["by_source_version"], cov["review_count"], len(gaps)))


if __name__ == "__main__":
    main()
