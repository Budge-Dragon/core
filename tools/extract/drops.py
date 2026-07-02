#!/usr/bin/env python3
"""Extract drop groups (spec section 13) into data/drop_groups.json.

Sources (OpenMU clone /tmp/openmu-ref):
  src/Persistence/Initialization/GameConfigurationInitializerBase.cs
      default groups: money 0.5, random item 0.3, jewel 0.001 (all versions
      -> "075"), excellent 0.0001 (gated on excellent options -> "095d").
  src/Persistence/Initialization/Version095d/Items/EventTicketItems.cs
      Devil's Eye/Key +1..+4, chance 0.01, monster-level bands built from
      drop levels [2, 36, 47, 60]; registered as defaults on every map.
  src/Persistence/Initialization/Version095d/InvasionMobsInitialization.cs
      monster-bound groups: Golden Budge Dragon (43) Box of Luck, Golden
      Titan (53) Box of Kundun +2 (item level 9), Red Dragon (44) jewels.
  src/Persistence/Initialization/VersionSeasonSix/Items/EventTicketItems.cs
      s6 backport: Blood Castle ticket ingredients Scroll of Archangel /
      Blood Bone +1..+8, bands from [2, 32, 45, 57, 68, 76, 84, 95].
  src/Persistence/Initialization/VersionSeasonSix/Maps/Icarus.cs
      s6 backport: Loch's Feather / Crest of Monarch (item level 1), chance
      0.001, min monster level 82, bound to the Icarus map only.

Not here by design: item-attached box drop tables (Box of Luck contents) are
the items extractor's `box_drops`; everything else found in the source but
not emitted is a named gap in the coverage file.

Drop groups reference no stats, so stat_map.json is not needed here.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import coverage, item_ref, write_datafile


def simple(slug, kind, chance, version):
    return {"id": slug, "kind": kind, "chance": chance, "source_version": version}


def item_list(slug, chance, items, item_level, monster, min_lvl, max_lvl, version,
              review=None):
    rec = {
        "id": slug,
        "kind": "item_list",
        "chance": chance,
        "items": [item_ref(g, n) for g, n in items],
        "item_level": item_level,  # source null == drops at +0
        "monster": monster,
        "min_monster_level": min_lvl,
        "max_monster_level": max_lvl,
        "source_version": version,
    }
    if review:
        rec["review"] = review
    return rec


def ticket_bands(slug_base, item, drop_levels, version, review_fn):
    """Mirror the event-ticket initializer loop: item level n (1-based) drops
    from monsters of level drop_levels[n-1] up to drop_levels[n]-1 (open top
    for the last band). Chance 0.01, registered on every map."""
    records = []
    for i, min_lvl in enumerate(drop_levels):
        level = i + 1
        max_lvl = drop_levels[i + 1] - 1 if i + 1 < len(drop_levels) else None
        records.append(item_list(
            "%s_plus_%d" % (slug_base, level), 0.01, [item], level,
            None, min_lvl, max_lvl, version, review_fn(level)))
    return records


records = []

# --- defaults (base config initializer, shared by every version) -----------
records.append(simple("default_money", "money", 0.5, "075"))
records.append(simple("default_random_item", "random_item", 0.3, "075"))
records.append(simple("default_jewels", "jewel", 0.001, "075"))
# only versions with excellent options get this group -> 095d
records.append(simple("default_excellent", "excellent", 0.0001, "095d"))

# --- 095d Devil Square ticket ingredients (default-registered on all maps) -
for base, number in (("devils_eye", 17), ("devils_key", 18)):
    records.extend(ticket_bands(base, (14, number), [2, 36, 47, 60], "095d",
                                lambda level: None))

# --- 095d invasion monsters (monster-bound, guaranteed chance 1.0) ---------
records.append(item_list(
    "golden_budge_dragon_box_of_luck", 1.0, [(14, 11)], 0, 43,
    None, None, "095d"))
records.append(item_list(
    "golden_titan_box_of_kundun_2", 1.0, [(14, 11)], 9, 53,
    None, None, "095d"))
records.append(item_list(
    "red_dragon_jewels", 1.0, [(14, 13), (14, 14), (12, 15)], 0, 44,
    None, None, "095d"))

# --- s6 backports: Blood Castle ticket ingredients -------------------------
def bc_review(level):
    note = ("Blood Castle ticket ingredient, approved 1.0-era backport; "
            "band values are s6 data")
    if level >= 7:
        note += "; gate %d is arguably later-era (7: S1+, 8: S3)" % level
    return note


records.extend(ticket_bands("scroll_of_archangel", (13, 16),
                            [2, 32, 45, 57, 68, 76, 84, 95], "s6", bc_review))
records.extend(ticket_bands("blood_bone", (13, 17),
                            [2, 32, 45, 57, 68, 76, 84, 95], "s6", bc_review))

# --- s6 backports: Icarus map groups (2nd wings / Cape of Lord inputs) -----
records.append(item_list(
    "icarus_lochs_feather", 0.001, [(13, 14)], 0, None, 82, None, "s6",
    "feeds the approved 2nd-wings mix (1.0-era); s6 Icarus map data, "
    "bound to the Icarus map only"))
records.append(item_list(
    "icarus_crest_of_monarch", 0.001, [(13, 14)], 1, None, 82, None, "s6",
    "Crest of Monarch (Loch's Feather +1) feeds Cape of Lord — Dark "
    "Lord-era content, era doubt; bound to the Icarus map only"))

out_path = write_datafile("drop_groups.json", records)

# --- coverage ---------------------------------------------------------------
by_version = {}
for rec in records:
    by_version[rec["source_version"]] = by_version.get(rec["source_version"], 0) + 1

coverage("drops", {
    "file": "data/drop_groups.json",
    "records": len(records),
    "by_source_version": by_version,
    "review_flagged": [r["id"] for r in records if "review" in r],
    "binding_notes": {
        "default_on_every_map": (
            ["default_money", "default_random_item", "default_excellent",
             "default_jewels"]
            + [r["id"] for r in records if r["id"].startswith(
                ("devils_", "scroll_of_archangel", "blood_bone"))]),
        "icarus_map_only": ["icarus_lochs_feather", "icarus_crest_of_monarch"],
        "monster_bound": {r["id"]: r["monster"] for r in records
                          if r.get("monster") is not None},
    },
    "gaps": [
        {"name": "box_item_drop_tables",
         "why": "box contents (Box of Luck etc.) are item-attached drop "
                "tables; owned by the items extractor's box_drops by design"},
        {"name": "legacy_quest_item_drop_groups",
         "why": "quest-bound drop groups (broken sword, scroll of emperor, "
                "...) ship with quest definitions; quests.json is deferred "
                "to a follow-up wave per spec section 9"},
        {"name": "blood_castle_event_reward_groups",
         "why": "in-event reward groups (jewel of chaos per gate) and the "
                "destructible saint statue's archangel-weapon drop need an "
                "event/minigame schema not in this wave; statue role is an "
                "excluded s6 event object"},
        {"name": "chaos_castle_reward_groups",
         "why": "winner jewel/ancient reward groups need the missing "
                "event/minigame schema; s6-era doubt on the event itself"},
        {"name": "s6_golden_army_boxes",
         "why": "s6 golden invasion monsters 78-83 drop Box of Kundun "
                "+1/+3/+4/+5; golden army is S3-era and the monsters are "
                "not in the baseline — Golden Dragon (79) is arguably "
                "1.0-era, left to the monsters-wave curation"},
        {"name": "devil_square_5_to_7_ticket_bands",
         "why": "s6 Devil's Eye/Key bands +5..+7 belong to Devil Square "
                "arenas beyond the 095d baseline (4 arenas)"},
        {"name": "dark_horse_raven_spirit_drops",
         "why": "trainable pets are excluded wholesale by the spec"},
        {"name": "symbol_of_kundun_drops",
         "why": "seven level-banded groups for Kalima entry; Kalima maps/"
                "items are in neither the baseline nor the backport list"},
        {"name": "land_of_trials_jewel_of_guardian",
         "why": "castle siege content, excluded wholesale by the spec"},
        {"name": "barracks_of_balgass_flame_of_condor",
         "why": "3rd-wings ingredient, post-S3"},
        {"name": "illusion_temple_ticket_drops",
         "why": "Illusion Temple is post-S3"},
        {"name": "golden_soldier_no_drop_group",
         "why": "observation: the 095d source binds no drop group to "
                "Golden Soldier (54); only 43/53/44 carry one"},
    ],
})

print("wrote %s (%d records)" % (out_path, len(records)))
