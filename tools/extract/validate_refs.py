"""Cross-reference validator for the v2 /data/*.json set (throwaway tooling).

Loads every v2 data file, builds numeric-identity indexes, and checks that all
cross-references resolve per the locked v2 JSON contract:

- envelope shape {"records": [...]} on every file
- "source_version" in {"075", "095d", "s6"} on every top-level record
- item {group, number} refs (ancient set pieces, chaos recipes, special/box
  drops, game_config jewel_drops) -> item_definitions.json
- monster numbers (spawns, skill summons, monster-bound drops) ->
  monster_definitions.json
- skill numbers (monster attack, item skill/teaches) -> skills.json
- transformation-ring skins (6 monster numbers) -> monster_definitions.json
- map numbers (gates, spawns, map-bound drops, class home_map) ->
  map_definitions.json
- gate targets (enter_gate/warp target_gate) -> a spawn_gate|target_gate number
- class enum values (evolution.into, item/skill class lists) -> the class roster

The v2 identity model is purely numeric: the v1 slug vocabulary (stat / option /
set / bonus_table / effect / drop_group / class slugs and the "global"
pseudo-record) is gone, so those checks are removed.

Failures are grouped per (owning category, reference kind) with counts and up to
3 examples. Exit code 1 if anything is broken.
"""

import json
import os
import sys
from collections import defaultdict

from common import DATA_DIR

VALID_SOURCE_VERSIONS = {"075", "095d", "s6"}

# data file -> owning extractor category (matches data/_coverage/*.json)
CATEGORY_BY_FILE = {
    "classes.json": "classes",
    "item_definitions.json": "items",
    "monster_definitions.json": "monsters",
    "spawns.json": "monsters",
    "skills.json": "skills-effects",
    "map_definitions.json": "maps",
    "gates_warps.json": "maps",
    "ancient_sets.json": "options_sets",
    "special_drops.json": "drops",
    "box_drops.json": "drops",
    "chaos_mixes.json": "chaos_mixes",
    "exp_tables.json": "constants_exp",
    "game_config.json": "constants_exp",
}

failures = defaultdict(list)  # (category, kind) -> [example, ...]


def fail(fname, kind, example):
    failures[(CATEGORY_BY_FILE.get(fname, fname), kind)].append(example)


def rec_label(fname, rec):
    """Short human handle for a record, for failure examples."""
    ident = rec.get("id")
    if isinstance(ident, dict):  # item {group, number}
        return f"{fname} item {ident.get('group')}/{ident.get('number')}"
    if "number" in rec:
        return f"{fname} #{rec['number']}"
    if "set_number" in rec:
        return f"{fname} set {rec['set_number']}"
    if "index" in rec:
        return f"{fname} warp {rec['index']}"
    if "name" in rec:
        return f"{fname} {rec['name']!r}"
    return fname


# ---------------------------------------------------------------- load files

datasets = {}
for fname in sorted(os.listdir(DATA_DIR)):
    if not fname.endswith(".json"):
        continue
    path = os.path.join(DATA_DIR, fname)
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    if set(data.keys()) != {"records"}:
        fail(fname, "bad envelope (want exactly a records list)",
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


item_refs = {(r["id"]["group"], r["id"]["number"])
             for r in records_of("item_definitions.json")
             if isinstance(r.get("id"), dict)}
skill_numbers = {r.get("number") for r in records_of("skills.json")}
monster_numbers = {r.get("number") for r in records_of("monster_definitions.json")}
map_numbers = {r.get("number") for r in records_of("map_definitions.json")}
class_names = {r.get("class") for r in records_of("classes.json")}

gates = records_of("gates_warps.json")
spawn_or_target_gate_numbers = {r.get("number") for r in gates
                                if r.get("kind") in ("spawn_gate", "target_gate")}


# ------------------------------------------------------------ helper checks


def check_item(fname, ref, label, context):
    if not (isinstance(ref, dict)
            and (ref.get("group"), ref.get("number")) in item_refs):
        fail(fname, "unresolved item {group,number} ref", f"{label}: {context} {ref}")


def check_items(fname, refs, label, context):
    for ref in refs or []:
        check_item(fname, ref, label, context)


def check_monster(fname, number, label, context):
    if number not in monster_numbers:
        fail(fname, "unresolved monster number", f"{label}: {context} {number}")


def check_skill(fname, number, label, context):
    if number not in skill_numbers:
        fail(fname, "unresolved skill number", f"{label}: {context} {number}")


def check_map(fname, number, label, context):
    if number not in map_numbers:
        fail(fname, "unresolved map number", f"{label}: {context} {number}")


def check_classes_list(fname, names, label, context):
    for name in names or []:
        if name not in class_names:
            fail(fname, "unknown class name", f"{label}: {context} {name!r}")


# ------------------------------------------------------------ generic checks


def check_source_versions():
    for fname, records in datasets.items():
        for rec in records:
            sv = rec.get("source_version")
            if sv not in VALID_SOURCE_VERSIONS:
                fail(fname, "missing/invalid source_version",
                     f"{rec_label(fname, rec)} source_version={sv!r}")


# ------------------------------------------------------------ per-file checks


def check_classes():
    fname = "classes.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        check_map(fname, rec.get("home_map"), label, "home_map")
        evolution = rec.get("evolution", {})
        if evolution.get("kind") == "evolves":
            check_classes_list(fname, [evolution.get("into")], label, "evolution.into")


def check_item_definitions():
    fname = "item_definitions.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        kind = rec.get("kind")
        # weapon/bow/crossbow/staff/shield/pet carry an optional attack skill.
        skill = rec.get("skill")
        if kind in ("weapon", "bow", "crossbow", "staff", "shield", "pet") and skill is not None:
            check_skill(fname, skill, label, "skill")
        # orb / skill_scroll teach a skill.
        if kind in ("orb", "skill_scroll"):
            check_skill(fname, rec.get("teaches"), label, "teaches")
        # transformation rings carry six monster skins.
        if kind == "transformation_ring":
            for skin in rec.get("skins", []):
                check_monster(fname, skin, label, "skin")
        check_classes_list(fname, rec.get("classes"), label, "class")


def check_monsters():
    fname = "monster_definitions.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        attack = rec.get("role", {}).get("attack")
        if isinstance(attack, dict) and attack.get("kind") == "skill":
            check_skill(fname, attack.get("skill"), label, "attack skill")


def check_spawns():
    fname = "spawns.json"
    for i, rec in enumerate(records_of(fname)):
        label = f"{fname}[{i}]"
        check_map(fname, rec.get("map"), label, "map")
        check_monster(fname, rec.get("monster"), label, "monster")


def check_skills():
    fname = "skills.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        shape = rec.get("shape", {})
        if shape.get("kind") == "summon":
            check_monster(fname, shape.get("monster"), label, "summon")
        check_classes_list(fname, rec.get("classes"), label, "class")


def check_gates_warps():
    fname = "gates_warps.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        kind = rec.get("kind")
        if kind in ("spawn_gate", "target_gate", "enter_gate"):
            check_map(fname, rec.get("map"), f"{kind} {label}", "map")
        if kind in ("enter_gate", "warp"):
            target = rec.get("target_gate")
            if target not in spawn_or_target_gate_numbers:
                fail(fname, "target_gate does not resolve to a spawn/target gate",
                     f"{kind} {label}: target_gate {target}")


def check_ancient_sets():
    fname = "ancient_sets.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        for piece in rec.get("pieces", []):
            check_item(fname, piece.get("item"), label, "piece")


def check_special_drops():
    fname = "special_drops.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        kind = rec.get("kind")
        if kind == "level_banded":
            check_item(fname, rec.get("item"), label, "item")
        elif kind == "monster_bound":
            check_monster(fname, rec.get("monster"), label, "monster")
            check_items(fname, rec.get("items"), label, "item")
        elif kind == "map_bound":
            check_map(fname, rec.get("map"), label, "map")
            check_item(fname, rec.get("item"), label, "item")
        else:
            fail(fname, "unknown special-drop kind", f"{label}: kind {kind!r}")


def check_box_drops():
    fname = "box_drops.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        check_item(fname, rec.get("box_item"), label, "box_item")
        check_items(fname, rec.get("items"), label, "item")


# Chaos recipe -> the item refs it names (mirrors atlas::recipe_item_refs).
def chaos_recipe_items(recipe):
    kind = recipe.get("kind")
    if kind == "chaos_weapon":
        return list(recipe.get("weapons", []))
    if kind == "first_wings":
        return list(recipe.get("chaos_weapons", [])) + list(recipe.get("wings", []))
    if kind == "second_wings":
        feather = recipe.get("feather", {})
        return (list(recipe.get("first_wings", []))
                + [feather.get("item")]
                + list(recipe.get("wings", [])))
    if kind == "cape_of_lord":
        crest = recipe.get("crest", {})
        return (list(recipe.get("first_wings", []))
                + [crest.get("item"), recipe.get("cape")])
    if kind == "item_upgrade":
        return []
    if kind == "dinorant":
        return [recipe.get("horn"), recipe.get("dinorant")]
    if kind == "fruits":
        return [recipe.get("catalyst"), recipe.get("fruit")]
    if kind == "devil_square_ticket":
        return [recipe.get("eye"), recipe.get("key"), recipe.get("invitation")]
    if kind == "blood_castle_ticket":
        return [recipe.get("scroll"), recipe.get("bone"), recipe.get("cloak")]
    return None  # unknown kind


def check_chaos_mixes():
    fname = "chaos_mixes.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        recipe = rec.get("recipe", {})
        items = chaos_recipe_items(recipe)
        if items is None:
            fail(fname, "unknown chaos recipe kind", f"{label}: kind {recipe.get('kind')!r}")
            continue
        check_items(fname, items, label, "recipe item")


def check_game_config():
    fname = "game_config.json"
    for rec in records_of(fname):
        label = rec_label(fname, rec)
        check_items(fname, rec.get("drops", {}).get("jewel_drops"), label, "jewel_drop")


# --------------------------------------------------------------------- main

check_source_versions()
check_classes()
check_item_definitions()
check_monsters()
check_spawns()
check_skills()
check_gates_warps()
check_ancient_sets()
check_special_drops()
check_box_drops()
check_chaos_mixes()
check_game_config()

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
