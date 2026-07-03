#!/usr/bin/env python3
"""Extract the chaos machine's closed recipe catalog -> data/chaos_mixes.json (v2).

The v1 generic ingredient-matcher engine is dead: MixBehavior, MixCost,
MixSuccess, MixInput, ItemMatch, MixAmount, MixItemAction, ResultSelection,
ResultChances, ref-linking and both npc_price_divisors were OpenMU's
SimpleCraftingSettings evaluator transcribed field-for-field, and they became
Rust (services/crafting.rs). v2 emits ChaosMix: a provenance envelope
{name, source_version, review?, recipe} wrapping a kind-tagged ChaosRecipe
where each family carries exactly its own typed facts and economics.

Sources (verified against /tmp/openmu-ref); the numbers below are the same
authentic values the v1 extractor pulled, re-shaped to the v2 contract:
  Version075/ChaosMixes.cs        -> Chaos Weapon (identical in 095d => "075")
  Version095d/ChaosMixes.cs       -> 1st Wings, +10, +11, Dinorant
  GameLogic/.../DevilSquareTicketCrafting.cs (version-shared handler) -> DS ticket
  VersionSeasonSix/ChaosMixes.cs + BloodCastleTicketCrafting.cs (curated
    1.0-era backports, "s6" + review) -> 2nd Wings, Cape of Lord, Fruits, BC ticket

Contract notes (integrate phase MUST know):
  - Item-level WINDOWS emit {"min","max"} (the Interval canonical wire shared
    with box_drops item_level_range), NOT min_level/max_level. Per the R3 task
    brief's explicit override of the older R3-json-contract Window note.
  - feather/crest are ItemAtLevel {"item","level"} (an exact item+level, not a
    window) and keep those field names.
  - Every OpenMU-invented value (2nd-wings/cape 4M/40k value rates and 20/20
    luck/exc, Dinorant 3 horns/250k, fruit weights, ticket 80/70 & breadth)
    survives verbatim carrying a review string naming it an OpenMU default.
  - Killed mechanisms (bonus-roll formulas, downgrade, fruit stat weights, rate
    /fee evaluators, jewel identities the rules consume) live in
    services/crafting.rs, not here.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import coverage, item_ref, without_name, write_datafile, write_names

# Constants.MaximumItemLevel = 11 for both pre-S3 datasets; s6 caps of 15 are
# clamped to 11 (approved decision 2).
MAX_ITEM_LEVEL = 11

# Item identities (group, number), verified in the item initializers.
LOCHS_FEATHER = item_ref(13, 14)          # +0 = feather; +1 = Monarch's Crest
FRUITS = item_ref(13, 15)
CAPE_OF_LORD = item_ref(13, 30)
HORN_OF_UNIRIA = item_ref(13, 2)
HORN_OF_DINORANT = item_ref(13, 3)
JEWEL_OF_CREATION = item_ref(14, 22)
DEVILS_EYE = item_ref(14, 17)
DEVILS_KEY = item_ref(14, 18)
DEVILS_INVITATION = item_ref(14, 19)
SCROLL_OF_ARCHANGEL = item_ref(13, 16)
BLOOD_BONE = item_ref(13, 17)
INVISIBILITY_CLOAK = item_ref(13, 18)
CHAOS_DRAGON_AXE = item_ref(2, 6)
CHAOS_NATURE_BOW = item_ref(4, 6)
CHAOS_LIGHTNING_STAFF = item_ref(5, 7)
CHAOS_WEAPONS = [CHAOS_DRAGON_AXE, CHAOS_NATURE_BOW, CHAOS_LIGHTNING_STAFF]
FIRST_WINGS = [item_ref(12, n) for n in (0, 1, 2)]      # fairy / heaven / satan
SECOND_WINGS = [item_ref(12, n) for n in (3, 4, 5, 6)]  # spirit/soul/dragon/darkness


def window(mn, mx):
    """Inclusive item-level window on the Interval canonical wire {min,max}."""
    return {"min": mn, "max": mx}


def at_level(item, level):
    """An exact item at an exact level (ItemAtLevel)."""
    return {"item": item, "level": level}


def wing_economics():
    """WingEconomics shared verbatim by second_wings and cape_of_lord.

    fee 5M / 90% cap are authentic; the two value-per-percent rates
    (4M / 40k) and the 20/20 luck/exc chances are OpenMU-only (flagged on
    each record's review).
    """
    return {
        "fee_zen": 5_000_000,
        "max_success_percent": 90,
        "wing_value_zen_per_percent": 4_000_000,
        "excellent_value_zen_per_percent": 40_000,
        "luck_chance_percent": 20,
        "excellent_chance_percent": 20,
    }


def mix(name, source_version, recipe, review=None):
    rec = {"name": name, "source_version": source_version}
    if review:
        rec["review"] = review
    rec["recipe"] = recipe
    return rec


RECORDS = [
    mix("Chaos Weapon", "075", {
        "kind": "chaos_weapon",
        "sacrifice_levels": window(4, MAX_ITEM_LEVEL),
        "weapons": CHAOS_WEAPONS,
    }),

    mix("1st Level Wings", "095d", {
        "kind": "first_wings",
        "chaos_weapons": CHAOS_WEAPONS,
        "chaos_weapon_levels": window(4, MAX_ITEM_LEVEL),
        "extra_sacrifice_levels": window(4, MAX_ITEM_LEVEL),
        "wings": FIRST_WINGS,
    }),

    mix("2nd Level Wings", "s6", {
        "kind": "second_wings",
        "first_wings": FIRST_WINGS,
        "wing_levels": window(0, MAX_ITEM_LEVEL),
        "excellent_levels": window(4, MAX_ITEM_LEVEL),
        "feather": at_level(LOCHS_FEATHER, 0),
        "economics": wing_economics(),
        "wings": SECOND_WINGS,
    }, review=(
        "2nd wings are 1.0-era, data only in S6 (Summoner input/result "
        "removed, level caps clamped 15->11); wing_value_zen_per_percent "
        "4,000,000 and excellent_value_zen_per_percent 40,000 exist only as "
        "OpenMU divisor encodings, and luck/excellent 20/20 only in OpenMU's "
        "S6 initializer — all four are OpenMU defaults pending classic "
        "sourcing")),

    mix("Cape of Lord", "s6", {
        "kind": "cape_of_lord",
        "first_wings": FIRST_WINGS,
        "wing_levels": window(0, MAX_ITEM_LEVEL),
        "excellent_levels": window(4, MAX_ITEM_LEVEL),
        "crest": at_level(LOCHS_FEATHER, 1),
        "economics": wing_economics(),
        "cape": CAPE_OF_LORD,
    }, review=(
        "Cape of Lord is the 1.0-era Dark Lord wing but this crafting "
        "(Monarch's Crest = Loch's Feather +1 recipe) may postdate 1.0; "
        "value-per-percent 4,000,000/40,000 and luck/excellent 20/20 are "
        "OpenMU defaults pending classic sourcing")),

    mix("+10 Item Combination", "095d", {
        "kind": "item_upgrade",
        "target": "plus_ten",
        "bless": 1,
        "soul": 1,
        "base_success_percent": 50,
        "fee_zen": 2_000_000,
    }),

    mix("+11 Item Combination", "095d", {
        "kind": "item_upgrade",
        "target": "plus_eleven",
        "bless": 2,
        "soul": 2,
        "base_success_percent": 45,
        "fee_zen": 4_000_000,
    }),

    mix("Dinorant", "095d", {
        "kind": "dinorant",
        "horn": HORN_OF_UNIRIA,
        "horn_count": 3,
        "success_percent": 70,
        "fee_zen": 250_000,
        "dinorant": HORN_OF_DINORANT,
    }, review=(
        "horn_count 3 and fee 250,000 exist only in OpenMU's 095d dataset; "
        "every classic source (and OpenMU's own S6 data) documents 10 "
        "full-durability Horns of Uniria for 500,000 zen — OpenMU "
        "defaults pending catalog decision; 70% and the full-durability horn "
        "rule are authentic")),

    mix("Fruits", "s6", {
        "kind": "fruits",
        "catalyst": JEWEL_OF_CREATION,
        "success_percent": 90,
        "fee_zen": 3_000_000,
        "fruit": FRUITS,
    }, review=(
        "stat fruits are 1.0-era, data only in S6; the created fruit's stat "
        "kind is a weighted services roll (weights are OpenMU defaults, see "
        "FRUIT_STAT_WEIGHTS), not data")),

    mix("Devil's Square Ticket", "095d", {
        "kind": "devil_square_ticket",
        "eye": DEVILS_EYE,
        "key": DEVILS_KEY,
        "invitation": DEVILS_INVITATION,
        "fee_zen_by_level": [100_000, 200_000, 400_000, 700_000,
                             1_100_000, 1_600_000, 2_000_000],
        "success_percent_by_level": [80, 80, 80, 80, 70, 70, 70],
    }, review=(
        "fee band and 7-level breadth come from OpenMU's version-shared "
        "handler table (classic-documented fees; pre-S3 Devil's Square level "
        "count pending era verification); the 80/70 success split at level 5 "
        "is an OpenMU handler constant pending classic sourcing")),

    mix("Blood Castle Ticket", "s6", {
        "kind": "blood_castle_ticket",
        "scroll": SCROLL_OF_ARCHANGEL,
        "bone": BLOOD_BONE,
        "cloak": INVISIBILITY_CLOAK,
        "fee_zen_by_level": [50_000, 80_000, 150_000, 250_000,
                             400_000, 600_000, 850_000, 1_050_000],
        "success_percent_by_level": [80, 80, 80, 80, 80, 80, 80, 80],
    }, review=(
        "Blood Castle is 0.97/1.0-era; fee band is the documented classic "
        "recipe, 8-level breadth pending era verification; flat 80% success "
        "is an OpenMU handler constant pending classic sourcing")),
]


RECIPE_KINDS = {
    "chaos_weapon", "first_wings", "second_wings", "cape_of_lord",
    "item_upgrade", "dinorant", "fruits", "devil_square_ticket",
    "blood_castle_ticket",
}
UPGRADE_TARGETS = {"plus_ten", "plus_eleven"}


def is_item_ref(v):
    return isinstance(v, dict) and set(v) == {"group", "number"} \
        and isinstance(v["group"], int) and isinstance(v["number"], int)


def check_window(w, rid):
    assert set(w) == {"min", "max"}, rid
    assert 0 <= w["min"] <= w["max"] <= 15, rid


def check_zen_table(t, n, rid):
    assert len(t) == n, rid
    assert all(isinstance(z, int) and z >= 0 for z in t), rid


def check_percent_table(t, n, rid):
    assert len(t) == n, rid
    assert all(isinstance(p, int) and 0 <= p <= 100 for p in t), rid


def verify(records):
    names = set()
    for r in records:
        rid = r["name"]
        assert r["source_version"] in ("075", "095d", "s6"), rid
        assert r["source_version"] != "s6" or r.get("review"), rid
        assert rid not in names, rid
        names.add(rid)
        assert set(r) <= {"name", "source_version", "review", "recipe"}, rid
        rec = r["recipe"]
        kind = rec["kind"]
        assert kind in RECIPE_KINDS, rid

        if kind == "chaos_weapon":
            check_window(rec["sacrifice_levels"], rid)
            assert len(rec["weapons"]) == 3 and all(map(is_item_ref, rec["weapons"])), rid
        elif kind == "first_wings":
            assert len(rec["chaos_weapons"]) == 3, rid
            check_window(rec["chaos_weapon_levels"], rid)
            check_window(rec["extra_sacrifice_levels"], rid)
            assert len(rec["wings"]) == 3, rid
        elif kind == "second_wings":
            assert len(rec["first_wings"]) == 3, rid
            check_window(rec["wing_levels"], rid)
            check_window(rec["excellent_levels"], rid)
            assert is_item_ref(rec["feather"]["item"]) and 0 <= rec["feather"]["level"] <= 15, rid
            assert len(rec["wings"]) == 4, rid
            verify_wing_economics(rec["economics"], rid)
        elif kind == "cape_of_lord":
            assert len(rec["first_wings"]) == 3, rid
            check_window(rec["wing_levels"], rid)
            check_window(rec["excellent_levels"], rid)
            assert is_item_ref(rec["crest"]["item"]) and 0 <= rec["crest"]["level"] <= 15, rid
            assert is_item_ref(rec["cape"]), rid
            verify_wing_economics(rec["economics"], rid)
        elif kind == "item_upgrade":
            assert rec["target"] in UPGRADE_TARGETS, rid
            assert rec["bless"] >= 1 and rec["soul"] >= 1, rid
            assert 0 <= rec["base_success_percent"] <= 100, rid
            assert rec["fee_zen"] >= 0, rid
        elif kind == "dinorant":
            assert is_item_ref(rec["horn"]) and is_item_ref(rec["dinorant"]), rid
            assert rec["horn_count"] >= 1, rid
            assert 0 <= rec["success_percent"] <= 100 and rec["fee_zen"] >= 0, rid
        elif kind == "fruits":
            assert is_item_ref(rec["catalyst"]) and is_item_ref(rec["fruit"]), rid
            assert 0 <= rec["success_percent"] <= 100 and rec["fee_zen"] >= 0, rid
        elif kind == "devil_square_ticket":
            assert all(is_item_ref(rec[k]) for k in ("eye", "key", "invitation")), rid
            check_zen_table(rec["fee_zen_by_level"], 7, rid)
            check_percent_table(rec["success_percent_by_level"], 7, rid)
        elif kind == "blood_castle_ticket":
            assert all(is_item_ref(rec[k]) for k in ("scroll", "bone", "cloak")), rid
            check_zen_table(rec["fee_zen_by_level"], 8, rid)
            check_percent_table(rec["success_percent_by_level"], 8, rid)


def verify_wing_economics(e, rid):
    assert set(e) == {
        "fee_zen", "max_success_percent", "wing_value_zen_per_percent",
        "excellent_value_zen_per_percent", "luck_chance_percent",
        "excellent_chance_percent"}, rid
    assert e["fee_zen"] >= 0 and e["wing_value_zen_per_percent"] >= 0, rid
    assert e["excellent_value_zen_per_percent"] >= 0, rid
    for p in ("max_success_percent", "luck_chance_percent",
              "excellent_chance_percent"):
        assert 0 <= e[p] <= 100, rid


def name_key(record):
    """Sidecar identity for a mix: its recipe kind (plus the upgrade target,
    the only field distinguishing the two item_upgrade rows)."""
    recipe = record["recipe"]
    key = {"recipe": recipe["kind"], "name": record["name"]}
    if recipe["kind"] == "item_upgrade":
        key["target"] = recipe["target"]
    return key


def main():
    assert len(RECORDS) == 10, len(RECORDS)
    verify(RECORDS)
    # Display names -> host-owned sidecar keyed by recipe; the core file carries
    # only the recipe facts (pre-S3 wire has no mix id — see notes).
    write_names("chaos_mixes.json", {"records": [name_key(r) for r in RECORDS]})
    path = write_datafile("chaos_mixes.json", [without_name(r) for r in RECORDS])

    by_version = {}
    for r in RECORDS:
        by_version[r["source_version"]] = by_version.get(r["source_version"], 0) + 1
    reviews = {r["name"]: r["review"] for r in RECORDS if "review" in r}

    coverage("chaos_mixes", {
        "records": len(RECORDS),
        "by_source_version": by_version,
        "review_count": len(reviews),
        "reviews": reviews,
        "invented_values": {
            "second_wings_cape_value_rates": "wing_value_zen_per_percent "
                "4,000,000 and excellent_value_zen_per_percent 40,000 exist "
                "only as OpenMU divisor encodings (2nd Level Wings, Cape of "
                "Lord) — OpenMU defaults pending re-derivation from "
                "classic sources",
            "second_wings_cape_bonus_chances": "luck/excellent 20/20 come "
                "only from OpenMU's S6 initializer (2nd Level Wings, Cape of "
                "Lord) — OpenMU defaults pending classic sourcing",
            "dinorant_horns_and_fee": "horn_count 3 / fee 250,000 (095d "
                "dataset); classic and OpenMU S6 both document 10 "
                "full-durability horns / 500,000 zen — kept verbatim "
                "pending catalog decision",
            "devil_square_success_split": "80/70 success split at level 5 is "
                "an OpenMU handler constant pending classic sourcing",
            "blood_castle_flat_success": "flat 80% success is an OpenMU "
                "handler constant pending classic sourcing",
            "ticket_level_breadth": "Devil's Square 7-level and Blood Castle "
                "8-level breadth come from OpenMU's version-shared handler "
                "tables; pre-S3 level counts pending era verification",
            "fruit_stat_weights": "the created fruit's stat kind is a "
                "weighted services roll (FRUIT_STAT_WEIGHTS 30/25/20/20/5); "
                "weights are OpenMU code constants, live in services not data",
        },
        "gaps": {
            "item_upgrade_12_to_15": "S6 mixes #22/#23/#49/#50: item level "
                "cap is 11 (approved decision 2)",
            "illusion_temple_ticket": "S6 mix #37: Illusion Temple is Season 3",
            "potion_of_bless_soul": "S6 mixes #15/#16: castle siege potions (S3+)",
            "shield_potions": "S6 mixes #30/#31/#32: SD stat system is S3+",
            "life_stone": "S6 mix #17: castle siege (S3)",
            "fenrir_craftings": "S6 mixes #25/#26/#27/#28: Fenrir is S2 but "
                "trainable pets are excluded wholesale (decision 5)",
            "dark_horse_dark_raven": "S6 mixes #13/#14 (Pet Trainer): "
                "trainable pets excluded (decision 5)",
            "third_wings": "S6 mixes #38/#39: 3rd wings are Season 3",
            "level_380_option": "S6 mix #36: guardian/380 options are post-S3",
            "secromicon": "S6 mix #46: Season 6",
            "gemstone_refinery_refine_restore": "S6 mixes #33/#34/#35: Jewel "
                "of Harmony ecosystem (S4)",
            "cherry_blossom_mix": "S6 mix #41: seasonal event (S4+)",
            "first_wings_misery_result": "S6 adds Wings of Misery (12,41) to "
                "the 1st-wings results; Summoner (S3) — 095d result list "
                "used",
            "jewel_mixes_lahap": "Lahap jewel packing (10 S6 JewelMix "
                "records): not a chaos mix, excluded by decision (crafting.md)",
        },
        "notes": {
            "window_wire_shape": "item-level windows emit {min,max} (Interval "
                "canonical wire, shared with box_drops item_level_range) per "
                "the R3 task-brief override of the older Window note; "
                "feather/crest stay ItemAtLevel {item,level}",
            "no_number_no_id": "pre-S3 wire carries no recipe id; the server "
                "deduces the recipe from placed ingredients (services "
                "match_recipe). number and id slug both killed; name covers "
                "display",
            "killed_to_services": "MixBehavior/MixCost/MixSuccess/MixInput/"
                "ItemMatch/MixAmount/MixItemAction/ResultSelection/"
                "ResultChances/ref-linking and both npc_price_divisors became "
                "services/crafting.rs (rate & fee formulas, downgrade, bonus "
                "rolls, fruit stat weights, jewel identities)",
        },
    })

    import json
    with open(path, encoding="utf-8") as f:
        parsed = json.load(f)
    assert len(parsed["records"]) == 10, len(parsed["records"])
    print("%s\nrecords=%d by_source_version=%s" % (
        path, len(parsed["records"]), by_version))


if __name__ == "__main__":
    main()
