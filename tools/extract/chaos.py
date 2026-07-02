#!/usr/bin/env python3
"""Extract chaos machine mixes (spec section 14) -> data/chaos_mixes.json.

Sources (verified against /tmp/openmu-ref):
  Version075/ChaosMixes.cs   -> #1 chaos weapon (identical in 095d => "075")
  Version095d/ChaosMixes.cs  -> #2 DS ticket, #3 +10, #4 +11, #5 dinorant, #11 1st wings
  VersionSeasonSix/ChaosMixes.cs (curated 1.0-era backports, "s6" + review)
                             -> #6 fruits, #7 2nd wings, #8 BC ticket, #24 cape of lord

The initializers are procedural C# building tiny object graphs; the recipes
are transcribed literally below (with the source's computed expressions kept
as expressions) instead of parsing C#. Mapping notes:

  - MaximumAmount == 0 in OpenMU means "no upper bound" -> amount.max = null.
    The spec's chaos-weapon example shows max 1 for the random-item input;
    the source has no maximum. Kept the source value, flagged in coverage.
  - MaximumSuccessPercent == 0 means "no explicit cap"; the engine clamps at
    100 -> max_percent = 100.
  - Settings-level NpcPriceDivisor == 0 / per-input divisor == 0 -> null.
  - MixResult: Disappear -> "disappear", StaysAsIs -> "stays",
    ChaosWeaponAndFirstWingsDowngradedRandom -> "downgrade_chaos_weapon".
  - Item level bounds apply to every requirement (RequiredItemMatches checks
    them regardless of PossibleItems), so "specific_items" matches carry
    min_level/max_level when the constraint is real (the matched items can
    have levels). For level-0-only ingredients (jewels, horns) the 0..0
    default is vacuous and omitted, matching the spec example.
  - s6 item-level caps of 15 are clamped to 11 (approved decision 2; both
    pre-S3 datasets use Constants.MaximumItemLevel = 11).
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import coverage, item_ref, write_datafile

MAX_ITEM_LEVEL = 11

# Item identities (group, number), verified in the item initializers.
JEWEL_OF_CHAOS = item_ref(12, 15)
JEWEL_OF_BLESS = item_ref(14, 13)
JEWEL_OF_SOUL = item_ref(14, 14)
JEWEL_OF_CREATION = item_ref(14, 22)
HORN_OF_UNIRIA = item_ref(13, 2)
HORN_OF_DINORANT = item_ref(13, 3)
LOCHS_FEATHER = item_ref(13, 14)  # +1 = Monarch's Crest
FRUITS = item_ref(13, 15)
CAPE_OF_LORD = item_ref(13, 30)
CHAOS_DRAGON_AXE = item_ref(2, 6)
CHAOS_NATURE_BOW = item_ref(4, 6)
CHAOS_LIGHTNING_STAFF = item_ref(5, 7)
CHAOS_WEAPONS = [CHAOS_DRAGON_AXE, CHAOS_NATURE_BOW, CHAOS_LIGHTNING_STAFF]
FIRST_WINGS = [item_ref(12, n) for n in (0, 1, 2)]   # fairy / heaven / satan
SECOND_WINGS = [item_ref(12, n) for n in (3, 4, 5, 6)]  # spirit/soul/dragon/darkness


def specific(items, min_level=None, max_level=None, required_option_types=None):
    m = {"kind": "specific_items", "items": items}
    if min_level is not None:
        m["min_level"] = min_level
        m["max_level"] = max_level
    if required_option_types:
        m["required_option_types"] = required_option_types
    return m


def any_item(min_level, max_level, required_option_types):
    return {"kind": "any_item", "min_level": min_level, "max_level": max_level,
            "required_option_types": required_option_types}


def inp(match, mn, mx, on_success="disappear", on_fail="disappear",
        npc_price_divisor=None, add_percent_per_extra=0, ref=None):
    return {"match": match, "amount": {"min": mn, "max": mx},
            "on_success": on_success, "on_fail": on_fail,
            "npc_price_divisor": npc_price_divisor,
            "add_percent_per_extra": add_percent_per_extra, "ref": ref}


def create(item, level_range=(0, 0), durability=None):
    return {"kind": "create", "item": item,
            "level_range": list(level_range), "durability": durability}


def modify(ref, add_level):
    return {"kind": "modify", "ref": ref, "add_level": add_level}


def mix(number, slug, name, source_version, behavior, review=None,
        flat_zen=0, zen_per_success_percent=0,
        base_percent=0, max_percent=100, npc_price_divisor=None,
        luck_bonus_percent=0, inputs=(), results=(),
        result_selection="any", multiple_allowed=False,
        luck_percent=0, skill_percent=0, excellent_percent=0):
    rec = {"number": number, "id": slug, "name": name,
           "source_version": source_version}
    if review:
        rec["review"] = review
    rec.update({
        "behavior": behavior,
        "cost": {"flat_zen": flat_zen,
                 "zen_per_success_percent": zen_per_success_percent},
        "success": {"base_percent": base_percent, "max_percent": max_percent,
                    "npc_price_divisor": npc_price_divisor,
                    "luck_bonus_percent": luck_bonus_percent},
        "inputs": list(inputs),
        "results": list(results),
        "result_selection": result_selection,
        "multiple_allowed": multiple_allowed,
        "result_chances": {"luck_percent": luck_percent,
                           "skill_percent": skill_percent,
                           "excellent_percent": excellent_percent},
    })
    return rec


def jewel(item, mn, mx):
    """Plain jewel/ingredient input: disappears either way, no level bounds."""
    return inp(specific([item]), mn, mx)


def item_upgrade(number, target_level):
    """095d ItemLevelUpgradeCrafting(craftingNumber, targetLevel)."""
    return mix(
        number, "item_upgrade_%d" % target_level,
        "+%d Item Combination" % target_level, "095d", "simple",
        flat_zen=2_000_000 * (target_level - 9),
        base_percent=50 if target_level == 10 else 45,
        luck_bonus_percent=25,
        inputs=[
            inp(any_item(target_level - 1, target_level - 1, []), 1, 1,
                on_success="stays", ref=1),
            jewel(JEWEL_OF_CHAOS, 1, 1),
            jewel(JEWEL_OF_BLESS, target_level - 9, target_level - 9),
            jewel(JEWEL_OF_SOUL, target_level - 9, target_level - 9),
        ],
        results=[modify(1, 1)])


def chaos_weapon_mix():
    """Version075 ChaosWeaponCrafting (identical in 095d)."""
    return mix(
        1, "chaos_weapon", "Chaos Weapon", "075",
        "chaos_weapon_and_first_wings",
        zen_per_success_percent=10_000, npc_price_divisor=20_000,
        inputs=[
            inp(any_item(4, MAX_ITEM_LEVEL, ["option"]), 1, None,
                on_fail="downgrade_chaos_weapon"),
            jewel(JEWEL_OF_CHAOS, 1, None),
            jewel(JEWEL_OF_BLESS, 0, None),
            jewel(JEWEL_OF_SOUL, 0, None),
        ],
        results=[create(w, (0, 4)) for w in CHAOS_WEAPONS])


def first_wings_mix():
    """Version095d FirstWingsCrafting."""
    return mix(
        11, "first_wings", "1st Level Wings", "095d",
        "chaos_weapon_and_first_wings",
        zen_per_success_percent=10_000, npc_price_divisor=20_000,
        inputs=[
            inp(specific(CHAOS_WEAPONS, 4, MAX_ITEM_LEVEL, ["option"]), 1, 1,
                on_fail="downgrade_chaos_weapon"),
            inp(any_item(4, MAX_ITEM_LEVEL, ["option"]), 0, None),
            jewel(JEWEL_OF_CHAOS, 1, None),
            jewel(JEWEL_OF_BLESS, 0, None),
            jewel(JEWEL_OF_SOUL, 0, None),
        ],
        results=[create(w) for w in FIRST_WINGS])


def dinorant_mix():
    """Version095d DinorantCrafting (3 horns / 250k; the S6 10/500k is not used)."""
    return mix(
        5, "dinorant", "Dinorant", "095d", "dinorant",
        flat_zen=250_000, base_percent=70, skill_percent=100,
        inputs=[
            jewel(JEWEL_OF_CHAOS, 1, 1),
            jewel(HORN_OF_UNIRIA, 3, 3),
        ],
        results=[create(HORN_OF_DINORANT)])


def devil_square_ticket_mix():
    """Version095d DevilSquareTicketCrafting: handler-only, no settings."""
    return mix(2, "devil_square_ticket", "Devil's Square Ticket", "095d",
               "ticket_devil_square")


def blood_castle_ticket_mix():
    """VersionSeasonSix BloodCastleTicketCrafting: handler-only backport."""
    return mix(8, "blood_castle_ticket", "Blood Castle Ticket", "s6",
               "ticket_blood_castle",
               review="Blood Castle is 0.97/1.0-era; recipe, prices and "
                      "success rates live in the ticket rule (S6 handler "
                      "values, see coverage rules)")


def second_wings_mix():
    """VersionSeasonSix SecondWingsCrafting, curated."""
    return mix(
        7, "second_wings", "2nd Level Wings", "s6", "second_wings",
        review="2nd wings are 1.0-era, data only in S6: Summoner wings "
               "removed (Wing of Misery 12/41 input, Wings of Despair 12/42 "
               "result), level caps clamped 15->11; luck/exc result chances "
               "(20/20, max 1 exc) are S6 values",
        flat_zen=5_000_000, max_percent=90,
        luck_percent=20, excellent_percent=20,
        inputs=[
            # first wing: explicit 0..15 bounds in source, clamped
            inp(specific(FIRST_WINGS, 0, MAX_ITEM_LEVEL), 1, 1,
                npc_price_divisor=4_000_000),
            inp(any_item(4, MAX_ITEM_LEVEL, ["excellent"]), 0, None,
                npc_price_divisor=40_000),
            jewel(JEWEL_OF_CHAOS, 1, 1),
            # feather at +0 only (a +1 feather is the Monarch's Crest)
            inp(specific([LOCHS_FEATHER], 0, 0), 1, 1),
        ],
        results=[create(w) for w in SECOND_WINGS])


def fruits_mix():
    """VersionSeasonSix FruitCrafting."""
    return mix(
        6, "fruits", "Fruits", "s6", "simple",
        review="stat fruits are 1.0-era, data only in S6; created fruit "
               "level (stat kind) is weighted-random 0-4 in the crafting "
               "rule, not data",
        flat_zen=3_000_000, base_percent=90,
        inputs=[
            jewel(JEWEL_OF_CHAOS, 1, 1),
            jewel(JEWEL_OF_CREATION, 1, 1),
        ],
        results=[create(FRUITS)])


def cape_of_lord_mix():
    """VersionSeasonSix CapeCrafting, curated to the DL cape only."""
    return mix(
        24, "cape_of_lord", "Cape of Lord", "s6", "second_wings",
        review="era-check: Cape of Lord is the 1.0-era Dark Lord wing, but "
               "this crafting (#24, Monarch's Crest = Loch's Feather+1 "
               "recipe) may postdate 1.0; Cape of Fighter (12/49, Rage "
               "Fighter) result and Wing of Misery (12/41) input removed, "
               "level caps clamped 15->11",
        flat_zen=5_000_000, max_percent=90,
        luck_percent=20, excellent_percent=20,
        inputs=[
            inp(specific(FIRST_WINGS, 0, MAX_ITEM_LEVEL), 1, 1,
                npc_price_divisor=4_000_000),
            inp(any_item(4, MAX_ITEM_LEVEL, ["excellent"]), 0, None,
                npc_price_divisor=40_000),
            jewel(JEWEL_OF_CHAOS, 1, 1),
            inp(specific([LOCHS_FEATHER], 1, 1), 1, 1),  # Monarch's Crest
        ],
        results=[create(CAPE_OF_LORD)])


RECORDS = sorted([
    chaos_weapon_mix(),
    devil_square_ticket_mix(),
    item_upgrade(3, 10),
    item_upgrade(4, 11),
    dinorant_mix(),
    fruits_mix(),
    second_wings_mix(),
    blood_castle_ticket_mix(),
    first_wings_mix(),
    cape_of_lord_mix(),
], key=lambda r: r["number"])

BEHAVIORS = {"simple", "chaos_weapon_and_first_wings", "second_wings",
             "dinorant", "ticket_devil_square", "ticket_blood_castle"}
MIX_RESULTS = {"disappear", "stays", "downgrade_chaos_weapon"}
OPTION_TYPES = {"option", "luck", "excellent", "ancient_option",
                "ancient_bonus", "wing"}


def verify(records):
    numbers, slugs = set(), set()
    for r in records:
        assert r["source_version"] in ("075", "095d", "s6"), r["id"]
        assert r["source_version"] != "s6" or r.get("review"), r["id"]
        assert r["behavior"] in BEHAVIORS, r["id"]
        assert r["result_selection"] in ("any", "all"), r["id"]
        assert r["number"] not in numbers and r["id"] not in slugs, r["id"]
        numbers.add(r["number"])
        slugs.add(r["id"])
        for i in r["inputs"]:
            m = i["match"]
            assert m["kind"] in ("specific_items", "any_item"), r["id"]
            assert m["kind"] != "specific_items" or m["items"], r["id"]
            assert m["kind"] != "any_item" or (
                "min_level" in m and "max_level" in m
                and "required_option_types" in m), r["id"]
            for t in m.get("required_option_types", []):
                assert t in OPTION_TYPES, r["id"]
            assert i["amount"]["min"] >= 0, r["id"]
            assert i["on_success"] in MIX_RESULTS, r["id"]
            assert i["on_fail"] in MIX_RESULTS, r["id"]
        refs = {i["ref"] for i in r["inputs"] if i["ref"] is not None}
        for res in r["results"]:
            assert res["kind"] in ("create", "modify"), r["id"]
            if res["kind"] == "modify":
                assert res["ref"] in refs, r["id"]
        for pct in r["result_chances"].values():
            assert isinstance(pct, int) and 0 <= pct <= 100, r["id"]


def main():
    verify(RECORDS)
    path = write_datafile("chaos_mixes.json", RECORDS)
    by_version = {}
    for r in RECORDS:
        by_version[r["source_version"]] = by_version.get(r["source_version"], 0) + 1
    reviews = {r["id"]: r["review"] for r in RECORDS if "review" in r}
    coverage("chaos_mixes", {
        "records": len(RECORDS),
        "by_source_version": by_version,
        "review_count": len(reviews),
        "reviews": reviews,
        "rules": {
            "ticket_devil_square": "handler recipe (not data): 1 Devil's Eye "
                "(14,17) + 1 Devil's Key (14,18) of EQUAL item level + 1 Jewel of "
                "Chaos -> Devil's Invitation (14,19) at the input level, durability "
                "1; success 80% for level<5 else 70%; zen by level 1-7 = 100k/200k/"
                "400k/700k/1.1m/1.6m/2m",
            "ticket_blood_castle": "handler recipe (not data): 1 Scroll of "
                "Archangel (13,16) + 1 Blood Bone (13,17) of EQUAL item level + 1 "
                "Jewel of Chaos -> Invisibility Cloak (13,18) at the input level, "
                "durability 1; success 80% flat; zen by level 1-8 = 50k/80k/150k/"
                "250k/400k/600k/850k/1.05m",
            "chaos_weapon_and_first_wings_options": "result option/luck/skill are "
                "formulas of the final success rate, not result_chances data: roll "
                "i in 0..2, item option level 3-i with chance rate/5 + 4*(i+1) "
                "percent; luck with rate/5 + 4; skill with rate/5 + 6",
            "second_wings_option": "wing item-option roll (20%/10%/4% for levels "
                "1/2/3) is in the second_wings rule; luck/excellent come from "
                "result_chances",
            "downgrade_chaos_weapon": "on-fail semantics: level -> random "
                "0..level-1, 50% skill loss (if not excellent), 50% item option "
                "-1 level (removed at 1), durability rescaled",
            "fruits_level": "created Fruits level (= fruit stat kind) is "
                "weighted-random 0-4 with weights 30/25/20/20/5",
            "success_npc_price_divisor": "settings-level divisor REPLACES the "
                "additive success path: rate = sum(npc old-buying prices of all "
                "inputs) / divisor; per-input divisor ADDS sum(prices)/divisor "
                "percent; final zen = flat_zen + zen_per_success_percent * rate",
            "max_percent_default": "OpenMU MaximumSuccessPercent 0 = uncapped; "
                "engine clamps at 100 -> emitted as max_percent 100",
        },
        "notes": {
            "chaos_weapon_amount": "spec section 14 example shows amount max 1 "
                "for the chaos-weapon random item; source has no maximum "
                "(MaximumAmount 0 = unbounded) -> emitted max null",
            "specific_items_levels": "specific_items matches carry min_level/"
                "max_level when the constraint is real (first wings weapon 4..11, "
                "Loch's Feather 0..0 vs Monarch's Crest 1..1); omitted for "
                "level-0-only ingredients where 0..0 is vacuous",
            "second_wings_max_exc_options": "S6 sets max 1 excellent option on "
                "2nd-wings/cape results; result_chances has no such field -> "
                "cap lives in the second_wings rule",
            "item_upgrade_success": "095d +10/+11 use flat 50%/45% + luck 25; the "
                "S6 penalty fields (exc/ancient/380/socket) are S6-only data, not "
                "backported",
        },
        "gaps": {
            "item_upgrade_12_to_15": "S6 mixes #22/#23/#49/#50: item level cap "
                "is 11 (approved decision 2)",
            "illusion_temple_ticket": "S6 mix #37: Illusion Temple is Season 3",
            "potion_of_bless_soul": "S6 mixes #15/#16: castle siege potions (S3+)",
            "shield_potions": "S6 mixes #30/#31/#32: SD stat system is S3+ "
                "(classic PvP instead)",
            "life_stone": "S6 mix #17: castle siege (S3)",
            "fenrir_craftings": "S6 mixes #25/#26/#27/#28: Fenrir is S2 but "
                "trainable pets are excluded wholesale (decision 5)",
            "dark_horse_dark_raven": "S6 mixes #13/#14 (Pet Trainer): trainable "
                "pets excluded (decision 5)",
            "third_wings": "S6 mixes #38/#39: 3rd wings are Season 3",
            "level_380_option": "S6 mix #36: guardian/380 options are post-S3",
            "secromicon": "S6 mix #46: Season 6",
            "gemstone_refinery_refine_restore": "S6 mixes #33/#34/#35 (Elphis/"
                "Osbourne/Jerridon): Jewel of Harmony ecosystem (S4)",
            "cherry_blossom_mix": "S6 mix #41: seasonal event (S4+)",
            "first_wings_misery_result": "S6 adds Wings of Misery (12,41) to the "
                "1st-wings results; Summoner (S3) - 095d result list used",
            "jewel_mixes_lahap": "Lahap jewel packing (10 S6 JewelMix records): "
                "pending decision 5, era-questionable packed-item ids",
        },
    })
    print(path)


if __name__ == "__main__":
    main()
