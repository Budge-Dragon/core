"""Cross-reference validator for /data/*.json (throwaway tooling).

Loads every data file, builds identity indexes, and checks that all
cross-references resolve per docs/specs/2026-07-02-data-schemas.md:

- envelope shape {"schema_version": 1, "records": [...]} on every file
- "source_version" in {"075", "095d", "s6"} on every record
- stat slugs (any {"stat": ...}, plus formula target/input, bonus_stat)
  -> stats.json
- possible_options -> item_options.json; possible_set_groups -> item_sets.json
- bonus_table slugs -> item_level_bonus_tables.json
- item {group, number} refs (sets/drops/chaos/box_drops) -> item_definitions.json
- skill numbers (item skill, monster attack_skill) -> skills.json
- monster numbers (spawns, drop groups, summon skills) -> monster_definitions.json
- map {number, discriminator} refs (spawns, gates, class home maps, safezones)
  -> map_definitions.json
- enter_gate/warp target_gate -> exit gate numbers
- effect slugs (skill effect, item consume_effect) -> magic_effects.json
- drop group slugs (maps, monsters) -> drop_groups.json
- class slugs (items, skills, evolution) -> character_classes.json (not "global")
- terrain sidecars referenced by maps exist on disk

Failures are grouped per (owning category, reference kind) with counts and up
to 3 examples. Exit code 1 if anything is broken.
"""

import json
import os
import sys
from collections import defaultdict

from common import DATA_DIR

VALID_SOURCE_VERSIONS = {"075", "095d", "s6"}

# data file -> owning extractor category (matches data/_coverage/*.json)
CATEGORY_BY_FILE = {
    "stats.json": "stats",
    "character_classes.json": "classes",
    "item_definitions.json": "items",
    "item_level_bonus_tables.json": "items",
    "item_options.json": "options_sets",
    "item_sets.json": "options_sets",
    "skills.json": "skills-effects",
    "magic_effects.json": "skills-effects",
    "monster_definitions.json": "monsters",
    "spawn_areas.json": "monsters",
    "map_definitions.json": "maps",
    "gates_warps.json": "maps",
    "drop_groups.json": "drops",
    "chaos_mixes.json": "chaos_mixes",
    "exp_tables.json": "constants_exp",
    "game_constants.json": "constants_exp",
}

failures = defaultdict(list)  # (category, kind) -> [example, ...]


def fail(fname, kind, example):
    failures[(CATEGORY_BY_FILE.get(fname, fname), kind)].append(example)


def rec_label(fname, rec):
    """Short human handle for a record, for failure examples."""
    if "id" in rec:
        rid = rec["id"]
        if isinstance(rid, dict):  # item {group, number}
            return f"{fname} item {rid.get('group')}/{rid.get('number')}"
        return f"{fname} {rid}"
    if "number" in rec:
        return f"{fname} #{rec['number']}"
    if "index" in rec:
        return f"{fname} warp {rec['index']}"
    return fname


# ---------------------------------------------------------------- load files

datasets = {}
for fname in sorted(os.listdir(DATA_DIR)):
    if not fname.endswith(".json"):
        continue
    path = os.path.join(DATA_DIR, fname)
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    if set(data.keys()) != {"schema_version", "records"} or data.get("schema_version") != 1:
        fail(fname, "bad envelope (want exactly schema_version=1 + records)",
             f"{fname} keys={sorted(data.keys())}")
    records = data.get("records", [])
    if not isinstance(records, list):
        fail(fname, "records is not a list", fname)
        records = []
    datasets[fname] = records

for fname in CATEGORY_BY_FILE:
    if fname not in datasets:
        fail(fname, "expected data file missing", os.path.join(DATA_DIR, fname))

# -------------------------------------------------------------- build indexes


def records_of(fname):
    return datasets.get(fname, [])


stat_ids = {r.get("id") for r in records_of("stats.json")}
option_ids = {r.get("id") for r in records_of("item_options.json")}
set_ids = {r.get("id") for r in records_of("item_sets.json")}
bonus_table_ids = {r.get("id") for r in records_of("item_level_bonus_tables.json")}
effect_ids = {r.get("id") for r in records_of("magic_effects.json")}
drop_group_ids = {r.get("id") for r in records_of("drop_groups.json")}
class_ids = {r.get("id") for r in records_of("character_classes.json")} - {"global"}
item_ids = {(r["id"]["group"], r["id"]["number"])
            for r in records_of("item_definitions.json") if isinstance(r.get("id"), dict)}
skill_numbers = {r.get("number") for r in records_of("skills.json")}
monster_numbers = {r.get("number") for r in records_of("monster_definitions.json")}
map_ids = {(r.get("number"), r.get("discriminator")) for r in records_of("map_definitions.json")}
exit_gate_numbers = {r.get("number") for r in records_of("gates_warps.json")
                     if r.get("kind") == "exit_gate"}

# ------------------------------------------------------------ generic checks


def check_source_versions():
    for fname, records in datasets.items():
        for rec in records:
            sv = rec.get("source_version")
            if sv not in VALID_SOURCE_VERSIONS:
                fail(fname, "missing/invalid source_version",
                     f"{rec_label(fname, rec)} source_version={sv!r}")


def walk_stat_refs(fname, node, label):
    """Any {"stat": "<slug>"} anywhere is a reference into stats.json."""
    if isinstance(node, dict):
        stat = node.get("stat")
        if isinstance(stat, str) and stat not in stat_ids:
            fail(fname, "unknown stat slug", f"{label}: stat {stat!r}")
        for value in node.values():
            walk_stat_refs(fname, value, label)
    elif isinstance(node, list):
        for value in node:
            walk_stat_refs(fname, value, label)


def check_stat_refs():
    for fname, records in datasets.items():
        for rec in records:
            walk_stat_refs(fname, rec, rec_label(fname, rec))
    # slugs living outside a "stat" key
    for rec in records_of("character_classes.json"):
        label = rec_label("character_classes.json", rec)
        for formula in rec.get("stat_formulas", []):
            for key in ("target", "input"):
                slug = formula.get(key)
                if isinstance(slug, str) and slug not in stat_ids:
                    fail("character_classes.json", "unknown stat slug",
                         f"{label}: formula {key} {slug!r}")
    for rec in records_of("item_sets.json"):
        label = rec_label("item_sets.json", rec)
        for piece in rec.get("pieces", []):
            slug = piece.get("bonus_stat")
            if slug is not None and slug not in stat_ids:
                fail("item_sets.json", "unknown stat slug", f"{label}: bonus_stat {slug!r}")


def walk_bonus_tables(fname, node, label):
    if isinstance(node, dict):
        table = node.get("bonus_table")
        if isinstance(table, str) and table not in bonus_table_ids:
            fail(fname, "unknown bonus_table slug", f"{label}: {table!r}")
        for value in node.values():
            walk_bonus_tables(fname, value, label)
    elif isinstance(node, list):
        for value in node:
            walk_bonus_tables(fname, value, label)


def check_bonus_tables():
    for fname, records in datasets.items():
        for rec in records:
            walk_bonus_tables(fname, rec, rec_label(fname, rec))


def check_item_ref(fname, ref, label, context):
    if not (isinstance(ref, dict) and (ref.get("group"), ref.get("number")) in item_ids):
        fail(fname, "unresolved item {group,number} ref", f"{label}: {context} {ref}")


def check_map_ref(fname, ref, label, context):
    if not (isinstance(ref, dict) and (ref.get("number"), ref.get("discriminator")) in map_ids):
        fail(fname, "unresolved map ref", f"{label}: {context} {ref}")


# ------------------------------------------------------------ per-file checks


def check_item_definitions():
    fname = "item_definitions.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        for slug in rec.get("possible_options", []):
            if slug not in option_ids:
                fail(fname, "unknown option slug in possible_options", f"{label}: {slug!r}")
        for slug in rec.get("possible_set_groups", []):
            if slug not in set_ids:
                fail(fname, "unknown set slug in possible_set_groups", f"{label}: {slug!r}")
        skill = rec.get("skill")
        if skill is not None and skill.get("skill") not in skill_numbers:
            fail(fname, "unknown skill number", f"{label}: skill {skill.get('skill')}")
        effect = rec.get("consume_effect")
        if effect is not None and effect not in effect_ids:
            fail(fname, "unknown effect slug", f"{label}: consume_effect {effect!r}")
        for slug in rec.get("classes", []):
            if slug not in class_ids:
                fail(fname, "unknown class slug", f"{label}: class {slug!r}")
        for box in rec.get("box_drops", []):
            for ref in box.get("items", []):
                check_item_ref(fname, ref, label, "box_drops item")


def check_item_sets():
    fname = "item_sets.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        for piece in rec.get("pieces", []):
            check_item_ref(fname, piece.get("item"), label, "piece")


def check_skills():
    fname = "skills.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        behavior = rec.get("behavior", {})
        if behavior.get("kind") == "summon" and behavior.get("monster") not in monster_numbers:
            fail(fname, "unknown monster number in summon",
                 f"{label}: monster {behavior.get('monster')}")
        effect = rec.get("effect")
        if effect is not None and effect not in effect_ids:
            fail(fname, "unknown effect slug", f"{label}: effect {effect!r}")
        for slug in rec.get("classes", []):
            if slug not in class_ids:
                fail(fname, "unknown class slug", f"{label}: class {slug!r}")


def check_monsters():
    fname = "monster_definitions.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        skill = rec.get("attack_skill")
        if skill is not None and skill not in skill_numbers:
            fail(fname, "unknown skill number", f"{label}: attack_skill {skill}")
        for slug in rec.get("drop_groups", []):
            if slug not in drop_group_ids:
                fail(fname, "unknown drop group slug", f"{label}: {slug!r}")


def check_spawn_areas():
    fname = "spawn_areas.json"
    for i, rec in enumerate(records_of(fname)):
        label = f"{fname}[{i}]"
        if rec.get("monster") not in monster_numbers:
            fail(fname, "unknown monster number in spawn", f"{label}: monster {rec.get('monster')}")
        check_map_ref(fname, rec.get("map"), label, "map")


def check_maps():
    fname = "map_definitions.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        for slug in rec.get("drop_groups", []):
            if slug not in drop_group_ids:
                fail(fname, "unknown drop group slug", f"{label}: {slug!r}")
        safezone = rec.get("safezone_map")
        if safezone is not None:
            check_map_ref(fname, safezone, label, "safezone_map")
        terrain = rec.get("terrain")
        if not terrain or not os.path.isfile(os.path.join(DATA_DIR, terrain)):
            fail(fname, "terrain file missing on disk", f"{label}: {terrain!r}")


def check_gates_warps():
    fname = "gates_warps.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        kind = rec.get("kind")
        if kind in ("exit_gate", "enter_gate"):
            check_map_ref(fname, rec.get("map"), f"{kind} {label}", "map")
        if kind in ("enter_gate", "warp") and rec.get("target_gate") not in exit_gate_numbers:
            fail(fname, "target_gate does not resolve to an exit gate",
                 f"{kind} {label}: target_gate {rec.get('target_gate')}")


def check_classes():
    fname = "character_classes.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        home = rec.get("home_map")
        if home is not None:
            check_map_ref(fname, home, label, "home_map")
        evolution = rec.get("evolution")
        if evolution is not None and evolution.get("class") not in class_ids:
            fail(fname, "unknown class slug", f"{label}: evolution {evolution.get('class')!r}")


def check_drop_groups():
    fname = "drop_groups.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        for ref in rec.get("items", []):
            check_item_ref(fname, ref, label, "item")
        monster = rec.get("monster")
        if monster is not None and monster not in monster_numbers:
            fail(fname, "unknown monster number", f"{label}: monster {monster}")


def check_chaos_mixes():
    fname = "chaos_mixes.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        for inp in rec.get("inputs", []):
            match = inp.get("match", {})
            if match.get("kind") == "specific_items":
                for ref in match.get("items", []):
                    check_item_ref(fname, ref, label, "input item")
        for result in rec.get("results", []):
            if result.get("kind") == "create":
                check_item_ref(fname, result.get("item"), label, "result item")


# --------------------------------------------------------------------- main

check_source_versions()
check_stat_refs()
check_bonus_tables()
check_item_definitions()
check_item_sets()
check_skills()
check_monsters()
check_spawn_areas()
check_maps()
check_gates_warps()
check_classes()
check_drop_groups()
check_chaos_mixes()

total_records = sum(len(records) for records in datasets.values())
print(f"checked {len(datasets)} files, {total_records} records")

if not failures:
    print("PASSED: all cross-references resolve")
    sys.exit(0)

print(f"FAILED: {len(failures)} broken reference kind(s)")
for (category, kind), examples in sorted(failures.items()):
    shown = "; ".join(examples[:3])
    print(f"  [{category}] {kind}: {len(examples)} occurrence(s), e.g. {shown}")
sys.exit(1)
