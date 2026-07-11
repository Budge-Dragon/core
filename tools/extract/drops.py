#!/usr/bin/env python3
"""Extract special drops + openable-box tables (v2 drops domain) into
data/special_drops.json and data/box_drops.json.

OpenMU's `DropItemGroup` model dies. v2 keeps only what MU actually has:
per-fact special-drop records keyed by the game's own identities (monster
number, map number, item ref) and box contents keyed by the box item.

The global per-kill roll policy (money 0.5 / item 0.3 / jewel 0.001 /
excellent 0.0001) and the option-roll policy are NOT here: they live in the
`drops` / `option_roll` sections of game_config.json, emitted by the
constants-exp extractor. This extractor owns only the banded/bound/box facts.

Sources (OpenMU clone reference/openmu):
  src/Persistence/Initialization/Version095d/Items/EventTicketItems.cs
      Devil's Eye 14/17 / Devil's Key 14/18, chance 0.01, monster-level
      bands built from drop levels [2, 36, 47, 60] -> item level 1..4.
  src/Persistence/Initialization/VersionSeasonSix/Items/EventTicketItems.cs
      s6 backport: Blood Castle ingredients Scroll of Archangel 13/16 /
      Blood Bone 13/17, chance 0.01, bands [2, 32, 45, 57, 68, 76, 84, 95]
      -> item level 1..8.
  src/Persistence/Initialization/Version095d/InvasionMobsInitialization.cs
      monster-bound (guaranteed) loot: Golden Budge Dragon (43) -> Box of
      Luck 14/11 +0; Golden Titan (53) -> Box of Kundun+2 = 14/11 +9; Red
      Dragon (44) -> Jewel of Bless/Soul/Chaos (14/13, 14/14, 12/15).
  src/Persistence/Initialization/VersionSeasonSix/Maps/Icarus.cs (map 10)
      s6 backport: Loch's Feather 13/14 +0 and Crest of Monarch 13/14 +1,
      chance 0.001, minimum monster level 82, bound to the Icarus map only.
  src/Persistence/Initialization/Version095d/Items/BoxOfLuck.cs
      Box of Luck (14/11) +0: RandomItem group, chance 0.5, fixed content
      level 6, 10,000-zen fallback; contents = 17 weapons/jewels plus armor
      sets 0/2/4/5/6/7/8/10/11/12 (all five pieces exist in 095d).

Chance encoding: OpenMU fraction x 10000 -> ChancePer10000 bare int
(0.01 -> 100, 0.001 -> 10, 0.5 -> 5000). Guaranteed monster-bound drops
carry NO chance field (guaranteed-by-rule; v2 deleted the chance:1.0
convention). No slugs, no group ids, no group indirection anywhere.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import coverage, item_ref, write_datafile


# --- review strings (every OpenMU-invented value is flagged) ----------------

DEVIL_REVIEW = (
    "banding concept is classic EventItemBag behavior; the 1% chance and band "
    "edges are OpenMU 095d initializer values pending classic verification"
)
BLOOD_CASTLE_REVIEW = (
    "Blood Castle ticket ingredient, approved 1.0-era backport; chance and "
    "band edges are s6 data; gates 7-8 arguably later-era (7: S1+, 8: S3)"
)
GOLDEN_TITAN_REVIEW = (
    "Box of Kundun+2 encoded as Box of Luck at item level 9 - authentic client "
    "encoding; Golden Titan era doubt (~0.97+)"
)
FEATHER_REVIEW = (
    "Loch's Feather feeds the approved 2nd-wings mix (1.0-era); the 0.1% chance "
    "and level floor 82 are OpenMU S6 Icarus values pending classic sources"
)
CREST_REVIEW = (
    "Crest of Monarch (Loch's Feather +1) feeds Cape of Lord - Dark Lord-era "
    "content, era doubt; the 0.1% chance and level floor 82 are OpenMU S6 "
    "Icarus values pending classic sources"
)
BOX_OF_LUCK_REVIEW = (
    "095d Box of Luck: OpenMU gives contents fixed level 6 and the "
    "50%/10,000-zen split - verify fixed level 6 against classic behavior"
)


# --- special_drops.json builders --------------------------------------------

def level_banded(item, chance_per_10000, drop_levels, version, review):
    """SpecialDrop::LevelBanded. Mirrors the event-ticket initializer loop:
    item level n (1-based) drops from monsters of level drop_levels[n-1] and
    up until the next band's threshold; the last band is open-ended. The v2
    band table is ascending {min_monster_level -> item_level} thresholds, not
    v1's per-plus-level records with min/max monster-level pairs."""
    bands = [{"min_monster_level": lvl, "item_level": i + 1}
             for i, lvl in enumerate(drop_levels)]
    rec = {
        "kind": "level_banded",
        "item": item_ref(*item),
        "chance_per_10000": chance_per_10000,
        "bands": bands,
        "source_version": version,
    }
    if review:
        rec["review"] = review
    return rec


def monster_bound(monster, items, item_level, version, review=None):
    """SpecialDrop::MonsterBound. Guaranteed loot on every kill of `monster`;
    a single-entry `items` is a fixed drop, multiple entries a uniform pick.
    No chance field: monster-bound drops are guaranteed by rule."""
    rec = {
        "kind": "monster_bound",
        "monster": monster,
        "items": [item_ref(g, n) for g, n in items],
        "item_level": item_level,
        "source_version": version,
    }
    if review:
        rec["review"] = review
    return rec


def map_bound(map_number, min_monster_level, item, item_level,
              chance_per_10000, version, review):
    """SpecialDrop::MapBound. World drop from monsters at or above a level
    floor, only on the named map."""
    rec = {
        "kind": "map_bound",
        "map": map_number,
        "min_monster_level": min_monster_level,
        "item": item_ref(*item),
        "item_level": item_level,
        "chance_per_10000": chance_per_10000,
        "source_version": version,
    }
    if review:
        rec["review"] = review
    return rec


special_records = [
    # 095d Devil Square ticket ingredients (bands 2/36/47/60).
    level_banded((14, 17), 100, [2, 36, 47, 60], "095d", DEVIL_REVIEW),
    level_banded((14, 18), 100, [2, 36, 47, 60], "095d", DEVIL_REVIEW),
    # s6 backport: Blood Castle ticket ingredients (8 bands).
    level_banded((13, 16), 100, [2, 32, 45, 57, 68, 76, 84, 95], "s6",
                 BLOOD_CASTLE_REVIEW),
    level_banded((13, 17), 100, [2, 32, 45, 57, 68, 76, 84, 95], "s6",
                 BLOOD_CASTLE_REVIEW),
    # 095d golden/red-dragon invasion bosses (guaranteed loot).
    monster_bound(43, [(14, 11)], 0, "095d"),                # Box of Luck +0
    monster_bound(53, [(14, 11)], 9, "095d", GOLDEN_TITAN_REVIEW),  # +9
    monster_bound(44, [(14, 13), (14, 14), (12, 15)], 0, "095d"),   # jewels
    # s6 backport: Icarus wing materials (map 10, floor 82).
    map_bound(10, 82, (13, 14), 0, 10, "s6", FEATHER_REVIEW),
    map_bound(10, 82, (13, 14), 1, 10, "s6", CREST_REVIEW),
]


# --- box_drops.json builders ------------------------------------------------

def armor_set(number):
    """The five armor pieces of one set: helm(7) armor(8) pants(9)
    gloves(10) boots(11). All exist for every referenced set in 095d, so the
    source `TryAddDropItem` existence filter admits all five."""
    return [(7, number), (8, number), (9, number), (10, number), (11, number)]


# 095d Box of Luck (14/11) +0 contents, in source order.
box_of_luck_items = [
    (0, 3), (0, 5), (0, 9), (0, 10), (0, 13),   # swords
    (4, 4), (4, 5), (4, 9), (4, 11), (4, 12),   # bows / crossbows
    (5, 0), (5, 2), (5, 3), (5, 4),             # staffs
    (12, 15), (14, 13), (14, 14),               # Chaos / Bless / Soul jewels
]
for _set in (0, 2, 4, 5, 6, 7, 8, 10, 11, 12):
    box_of_luck_items.extend(armor_set(_set))

box_records = [{
    "box_item": item_ref(14, 11),
    "box_level": 0,
    "item_roll_per_10000": 5000,
    "items": [item_ref(g, n) for g, n in box_of_luck_items],
    "item_level_range": {"min": 6, "max": 6},
    "money_fallback": 10000,
    "source_version": "095d",
    "review": BOX_OF_LUCK_REVIEW,
}]


# --- emit --------------------------------------------------------------------

special_path = write_datafile("special_drops.json", special_records)
box_path = write_datafile("box_drops.json", box_records)


# --- coverage ----------------------------------------------------------------

def by_version(records):
    out = {}
    for rec in records:
        out[rec["source_version"]] = out.get(rec["source_version"], 0) + 1
    return out


def special_id(rec):
    kind = rec["kind"]
    if kind == "level_banded":
        it = rec["item"]
        return "level_banded %d/%d" % (it["group"], it["number"])
    if kind == "monster_bound":
        return "monster_bound %d" % rec["monster"]
    if kind == "map_bound":
        it = rec["item"]
        return "map_bound map %d %d/%d+%d" % (
            rec["map"], it["group"], it["number"], rec["item_level"])
    return kind


combined = by_version(special_records)
for k, v in by_version(box_records).items():
    combined[k] = combined.get(k, 0) + v

coverage("drops", {
    "files": ["data/special_drops.json", "data/box_drops.json"],
    "records": len(special_records) + len(box_records),
    "by_source_version": combined,
    "special_drops": {
        "file": "data/special_drops.json",
        "records": len(special_records),
        "by_source_version": by_version(special_records),
        "review_flagged": [special_id(r) for r in special_records
                           if "review" in r],
        "monster_bound": {special_id(r): r["monster"] for r in special_records
                          if r["kind"] == "monster_bound"},
        "map_bound_map": {special_id(r): r["map"] for r in special_records
                          if r["kind"] == "map_bound"},
    },
    "box_drops": {
        "file": "data/box_drops.json",
        "records": len(box_records),
        "by_source_version": by_version(box_records),
        "review_flagged": ["box 14/11+%d" % r["box_level"] for r in box_records
                           if "review" in r],
        "content_item_refs": len(box_records[0]["items"]),
    },
    "notes": [
        {"name": "default_drop_policy_moved",
         "why": "the per-kill defaults (money 0.5, item 0.3, jewel 0.001, "
                "excellent 0.0001, skill 0.5) and the option-roll policy are "
                "the DropConfig/OptionRollPolicy sections of game_config.json, "
                "emitted by the constants-exp extractor - not a drops file"},
        {"name": "box_content_refs_atlas_checked",
         "why": "all 67 Box of Luck content ItemRefs (17 weapons/jewels + 10 "
                "armor sets x 5 pieces) must resolve in item_definitions.json "
                "at Atlas load; every referenced 095d armor set has all five "
                "pieces so the source TryAddDropItem filter admits them all"},
    ],
    "gaps": [
        {"name": "box_kundun_tiers_and_seasonal_boxes",
         "why": "s6 BoxOfLuck.cs defines Box of Kundun +1..+5 (14/11 box "
                "levels 8-12) and a seasonal box zoo (Pink/Red/Blue Chocolate, "
                "Ribbon, Christmas, Pumpkin, Cherry Blossom, GM Present). The "
                "Kundun tiers are ItemType=Excellent - the v2 BoxDrop shape "
                "(item-roll-else-money over a normal-item pool) cannot encode "
                "an excellent-only box without relabeling excellent as normal, "
                "so they are excluded; seasonal boxes are post-S3 event "
                "content outside the 095d baseline. Golden Titan's 14/11 +9 "
                "drop (special_drops) therefore has no box-level-9 contents "
                "record in this baseline - an authentic 095d gap (095d "
                "BoxOfLuck defines only box level 0). INTEGRATE decision if a "
                "resolved box tier is required."},
        {"name": "legacy_quest_item_drop_groups",
         "why": "chance-based monster-bound quest items (Broken Sword, Scroll "
                "of Emperor, ...) ship with quest definitions; quests are a "
                "named v2 scope boundary (no quests.json), and SpecialDrop "
                "extends with what quests need when that extraction lands"},
        {"name": "blood_castle_and_chaos_castle_event_rewards",
         "why": "in-event reward groups (per-gate jewels, saint-statue "
                "archangel-weapon drop, winner ancient/jewel rewards) need an "
                "event/minigame schema not in this wave"},
        {"name": "s6_golden_army_boxes",
         "why": "s6 golden invasion monsters 78-83 drop Box of Kundun "
                "+1/+3/+4/+5; golden army is S3-era and the monsters are not "
                "in the baseline monster set"},
        {"name": "devil_square_5_to_7_and_extended_ticket_bands",
         "why": "s6 Devil's Eye/Key bands +5..+7 (drop levels 70/80/90) and "
                "the s6 Blood Castle Old Scroll / Illusion Sorcerer Covenant "
                "banded tickets belong to arenas/events beyond the 095d "
                "baseline"},
        {"name": "symbol_of_kundun_kalima_drops",
         "why": "seven level-banded groups for Kalima entry; Kalima maps and "
                "items are in neither the baseline nor the backport list"},
        {"name": "land_of_trials_barracks_illusion_temple_drops",
         "why": "Jewel of Guardian (castle-siege), Flame of Condor "
                "(3rd-wings, post-S3), and Illusion Temple tickets (post-S3) "
                "are excluded wholesale by the spec"},
        {"name": "trainable_pet_drops",
         "why": "Dark Horse / Dark Raven / Spirit drops - trainable pets are "
                "excluded wholesale by the spec"},
        {"name": "golden_soldier_no_drop_group",
         "why": "observation: 095d InvasionMobsInitialization binds no drop "
                "group to Golden Soldier (54); only 43/53/44 carry one"},
    ],
})

print("wrote %s (%d records)" % (special_path, len(special_records)))
print("wrote %s (%d records)" % (box_path, len(box_records)))
print("special by source_version:", by_version(special_records))
print("box by source_version:", by_version(box_records))
print("box content item refs:", len(box_records[0]["items"]))
