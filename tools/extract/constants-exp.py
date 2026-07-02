#!/usr/bin/env python3
"""Extract game_constants.json + exp_tables.json (spec sections 15 and 16).

Sources (all values verified against /tmp/openmu-ref):
  GameConfigurationInitializerBase.cs   scalar game config values + global
                                        base attribute values + exp formula
  InitializerBase.cs                    MaximumOptionLevel (4)
  GameLogic/DefaultDropGenerator.cs     BaseMoneyDrop, DropLevelMaxGap,
                                        SkillDropChancePercent
  GameLogic/AttackableExtensions.cs     min hit chance 0.03, overrate 0.3
  DataModel/InventoryConstants.cs       storage sizes
  MovementSpeedConstants.cs             running gear / wing / iced speeds

Approved overrides (not OpenMU values):
  tick_duration_ms = 100                ours, decision 4
  max_inventory_money / max_vault_money = 2_000_000_000
                                        classic zen cap, decision 7 (OpenMU
                                        uses int.MaxValue)

The 402-entry exp table is computed here with exact integer math from the
two-piece formula (GameConfigurationInitializerBase.CalculateNeededExperience,
table shape from GameContext.CreateExpTable: MaximumLevel + 2 entries).

All values are identical across Version075/095d/S6 (shared base initializer,
no per-version overrides) -> source_version "075" on both records.
"""

import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import coverage, load_stat_map, write_datafile

OPENMU = "/tmp/openmu-ref/src"


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


def expect(actual, wanted, what):
    if actual != wanted:
        sys.exit("%s: expected %r, source has %r" % (what, wanted, actual))
    return actual


def total_exp(level):
    """Exact integer two-piece formula (CalculateNeededExperience)."""
    if level == 0:
        return 0
    base = 10 * (level + 8) * (level - 1) * (level - 1)
    if level < 256:
        return base
    return base + 1000 * (level - 247) * (level - 256) * (level - 256)


def main():
    cfg = parse_game_config(read("Persistence/Initialization/GameConfigurationInitializerBase.cs"))
    glob = parse_global_base_values(read("Persistence/Initialization/GameConfigurationInitializerBase.cs"))
    drop = parse_consts(read("GameLogic/DefaultDropGenerator.cs"))
    inv = parse_consts(read("DataModel/InventoryConstants.cs"))
    move = parse_consts(read("Persistence/Initialization/MovementSpeedConstants.cs"))
    attackable = read("GameLogic/AttackableExtensions.cs")
    init_base = read("Persistence/Initialization/InitializerBase.cs")
    stat_map = load_stat_map()

    # The global base attribute values feed stats.json; make sure the stats
    # agent shipped them before we mirror their values here.
    for prop in ("MoneyAmountRate", "RandomExperienceMinMultiplier",
                 "RandomExperienceMaxMultiplier"):
        if prop not in stat_map:
            sys.exit("stat_map.json is missing %s (run stats.py first)" % prop)

    # -- game_constants.json (spec section 16), field order as in the spec --
    constants = {
        "source_version": "075",
        "tick_duration_ms": 100,  # ours (decision 4), not an OpenMU value
        "recovery_interval_ms": expect(int(cfg["RecoveryInterval"]), 3000, "RecoveryInterval"),
        "info_range": expect(int(cfg["InfoRange"]), 12, "InfoRange"),
        "item_drop_duration_ms": 1000 * int(grab(
            r"TimeSpan\.FromSeconds\((\d+)\)", cfg["ItemDropDuration"], "ItemDropDuration")),
        "max_item_option_level_drop": expect(int(cfg["MaximumItemOptionLevelDrop"]), 3,
                                             "MaximumItemOptionLevelDrop"),
        "excellent_drop_level_delta": expect(int(cfg["ExcellentItemDropLevelDelta"]), 25,
                                             "ExcellentItemDropLevelDelta"),
        "max_option_level": expect(int(grab(
            r"protected virtual int MaximumOptionLevel => (\d+);", init_base,
            "InitializerBase.cs")), 4, "MaximumOptionLevel"),
        "should_drop_money": expect(cfg["ShouldDropMoney"], "true", "ShouldDropMoney") == "true",
        "money_amount_rate": expect(glob["MoneyAmountRate"], 1.0, "MoneyAmountRate"),
        "base_money_drop": expect(drop["BaseMoneyDrop"], 7, "BaseMoneyDrop"),
        "drop_level_max_gap": expect(drop["DropLevelMaxGap"], 12, "DropLevelMaxGap"),
        "skill_drop_chance": expect(drop["SkillDropChancePercent"], 50,
                                    "SkillDropChancePercent") / 100.0,
        # decision 7: classic zen cap, replaces OpenMU's int.MaxValue
        "max_inventory_money": (expect(cfg["MaximumInventoryMoney"], "int.MaxValue",
                                       "MaximumInventoryMoney") and 2_000_000_000),
        "max_vault_money": (expect(cfg["MaximumVaultMoney"], "int.MaxValue",
                                   "MaximumVaultMoney") and 2_000_000_000),
        "clamp_money_on_pickup": expect(cfg["ClampMoneyOnPickup"], "false",
                                        "ClampMoneyOnPickup") == "true",
        "maximum_party_size": expect(int(cfg["MaximumPartySize"]), 5, "MaximumPartySize"),
        "max_characters_per_account": expect(int(cfg["MaximumCharactersPerAccount"]), 5,
                                             "MaximumCharactersPerAccount"),
        "character_name_regex": expect(cfg["CharacterNameRegex"].strip('"'),
                                       "^[a-zA-Z0-9]{3,10}$", "CharacterNameRegex"),
        "prevent_experience_overflow": expect(cfg["PreventExperienceOverflow"], "false",
                                              "PreventExperienceOverflow") == "true",
        "area_skill_hits_player": expect(cfg["AreaSkillHitsPlayer"], "false",
                                         "AreaSkillHitsPlayer") == "true",
        "random_exp_multiplier_range": [
            expect(glob["RandomExperienceMinMultiplier"], 0.8, "RandomExperienceMinMultiplier"),
            expect(glob["RandomExperienceMaxMultiplier"], 1.2, "RandomExperienceMaxMultiplier"),
        ],
        "damage_per_one_item_durability": expect(int(cfg["DamagePerOneItemDurability"]),
                                                 2000, "DamagePerOneItemDurability"),
        "damage_per_one_pet_durability": expect(int(cfg["DamagePerOnePetDurability"]),
                                                100000, "DamagePerOnePetDurability"),
        "hits_per_one_item_durability": expect(int(cfg["HitsPerOneItemDurability"]),
                                               10000, "HitsPerOneItemDurability"),
        "minimum_hit_chance": float(grab(r"float hitChance = ([0-9.]+)f;", attackable,
                                         "AttackableExtensions.cs")),
        "overrate_damage_factor": float(grab(r"dmg = \(int\)\(dmg \* ([0-9.]+)\);",
                                             attackable, "AttackableExtensions.cs")),
        "inventory": {
            "equipped_slots": expect(inv["LastEquippableItemSlotIndex"]
                                     - inv["FirstEquippableItemSlotIndex"] + 1, 12,
                                     "equippable slots"),
            "main_rows": expect(inv["InventoryRows"], 8, "InventoryRows"),
            "main_columns": expect(inv["RowSize"], 8, "RowSize"),
            "store_slots": expect(inv["StoreRows"] * inv["RowSize"], 32, "store slots"),
            "vault_slots": expect(inv["WarehouseRows"] * inv["RowSize"], 120, "vault slots"),
            "temp_storage_slots": expect(inv["TemporaryStorageRows"] * inv["RowSize"], 32,
                                         "temp storage slots"),
        },
        "movement": {
            "running_gear_min_level": expect(move["RunningGearMinimumLevel"], 5,
                                             "RunningGearMinimumLevel"),
            "running_gear_speed": float(expect(move["RunningGearMovementSpeed"], 15.0,
                                               "RunningGearMovementSpeed")),
            "wing_speed": float(expect(move["DefaultWingMovementSpeed"], 15.0,
                                       "DefaultWingMovementSpeed")),
            "fast_wing_speed": float(expect(move["FastWingMovementSpeed"], 16.0,
                                            "FastWingMovementSpeed")),
            "iced_speed_factor": float(expect(move["IcedMovementSpeedFactor"], 0.5,
                                              "IcedMovementSpeedFactor")),
        },
    }
    expect(float(cfg["MinimumHitChance"]) if "MinimumHitChance" in cfg else
           constants["minimum_hit_chance"], 0.03, "minimum hit chance")
    expect(constants["overrate_damage_factor"], 0.3, "overrate damage factor")
    expect(drop["DefaultMaxItemOptionLevelDrop"],
           constants["max_item_option_level_drop"], "drop generator option-level cross-check")

    # -- exp_tables.json (spec section 15) --
    max_level = expect(int(cfg["MaximumLevel"]), 400, "MaximumLevel")
    table = [total_exp(level) for level in range(max_level + 2)]
    expect(len(table), 402, "exp table length")
    expect(table[:3], [0, 0, 100], "exp table head")
    expect(table[10], 14580, "exp table level 10 (facts sample)")
    if any(b < a for a, b in zip(table, table[1:])):
        sys.exit("exp table is not monotonic")
    if table[-1] >= 2**64:
        sys.exit("exp table overflows u64")
    exp_record = {
        "source_version": "075",
        "max_level": max_level,
        "formula": "10*(level+8)*(level-1)^2 [+ 1000*(level-247)*(level-256)^2 for level>=256]",
        "total_exp_by_level": table,
    }

    constants_path = write_datafile("game_constants.json", [constants])
    exp_path = write_datafile("exp_tables.json", [exp_record])

    gaps = {
        "max_letters": "letters/inbox are host-owned social features (50); excluded per spec section 16",
        "letter_send_price": "letters/inbox are host-owned social features (1000 zen); excluded per spec section 16",
        "max_password_length": "account/password constants excluded wholesale (spec exclusions)",
        "experience_rate": "global exp multiplier (1.0) is modeled as the stats.json stat 'experience_rate', not a constant; spec section 16 shape omits it",
        "movement_speed_factor": "global base value (1.0) is modeled as the stats.json stat 'movement_speed_factor'; spec section 16 shape omits it",
        "basic_mount_speed": "Uniria/Dinorant mount speed (15.0) rides in item_definitions power-ups, not in the constants shape",
        "cold_speed_factor": "ColdMovementSpeedFactor (0.33) is wired only by S6 cold-effect initializers; skills agent owns it if the ice-arrow backport needs it",
        "horse_fenrir_speeds": "17/19 belong to trainable pets, excluded wholesale",
        "master_experience": "master level/exp formula, MaximumMasterLevel 200, MinimumMonsterLevelForMasterExperience 95: post-S3",
        "maximum_alliance_size": "guild alliances, S6-only global value (5); decision 5 open",
        "per_kill_exp_formula": "(lvl+25)*lvl/3 with gap scaling, >=65 bonus, x1.25: formula shape -> Rust rule (decision 3)",
        "min_damage_floor": "max(1, attackerLevel/10): formula shape -> Rust rule (decision 3)",
        "dinorant_damage_factor": "x1.3 skill-less attack multiplier: formula shape -> Rust rule (decision 3)",
        "double_wield_factor": "halve-then-double wield rule: formula shape -> Rust rule (decision 3)",
        "classic_duel_damage_factor": "0.6 duel damage factor: formula shape -> Rust rule (decision 3)",
        "level_dependent_damage": "dead data even in OpenMU (populated and read nowhere); excluded per spec",
    }
    notes = [
        "spec section 15 sample shows total_exp_by_level[3] = 396; the formula "
        "(and OpenMU's CalculateNeededExperience) gives 440 - the computed table "
        "follows the formula, the spec sample value looks like a typo",
        "facts doc sample 'level 255 = 1697560640' disagrees with the formula "
        "(169677080); levels 2 and 10 samples match, formula followed",
        "max_inventory_money/max_vault_money overridden to 2000000000 (decision 7); "
        "OpenMU source has int.MaxValue",
        "tick_duration_ms 100 is a mu-core decision (4), not extracted from OpenMU",
    ]
    cov = {
        "files": [constants_path, exp_path],
        "records": 2,
        "by_source_version": {"075": 2},
        "review_count": 0,
        "reviews": {},
        "gaps": gaps,
        "notes": notes,
    }
    cov_path = coverage("constants_exp", cov)

    print("wrote %s (1 record)" % constants_path)
    print("wrote %s (1 record, %d exp entries, max %d)" % (exp_path, len(table), table[-1]))
    print("wrote %s (%d gaps, 0 reviews)" % (cov_path, len(gaps)))


if __name__ == "__main__":
    main()
