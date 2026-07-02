#!/usr/bin/env python3
"""Extract maps, spawn areas, gates/warps and terrain (spec sections 10-12).

Outputs:
  data/map_definitions.json    spec section 11
  data/spawn_areas.json        spec section 10
  data/gates_warps.json        spec section 12 (kind-tagged: exit_gate, enter_gate, warp)
  data/terrain/*.bin           spec section 11 sidecars (re-encoded from .att)
  data/_coverage/maps.json     counts, review list, named gaps

Baseline = the full 0.95d dataset (14 maps). source_version = "075" when the
record is defined by a Version075 initializer that 0.95d reuses unchanged,
"095d" otherwise. No s6 backports exist in this category (Blood Castle etc.
are named gaps).

Terrain: source .att = 3-byte header (00 ff ff) + 65536 cells (index = y*256+x).
Source flag bits Safezone=1, Character=2 (runtime occupancy, dropped),
Blocked=4, NoGround=8, Water=16 are re-encoded to our bits
safezone=1, blocked=2, no_ground=4, water=8. Bits above 0x1F (noise in a few
Icarus cells) are masked off and counted in the coverage report.
"""

import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import coverage, load_stat_map, map_ref, slugify, write_datafile, DATA_DIR

INIT = "/tmp/openmu-ref/src/Persistence/Initialization"
RESOURCES = os.path.join(INIT, "Resources")

# Direction enum: 0 = undefined (emitted as null). Gate direction bytes are the
# same raw values (source comment: "0 means 'Undefined' ... without adding 1").
DIRECTIONS = {
    1: "west", 2: "south_west", 3: "south", 4: "south_east",
    5: "east", 6: "north_east", 7: "north", 8: "north_west",
}

# Wave numbers from Version095d/Events/DevilSquareInitializer.cs constants.
WAVE_CONSTS = {
    "DevilSquareInitializer.FirstWaveNumber": 1,
    "DevilSquareInitializer.SecondWaveNumber": 2,
    "DevilSquareInitializer.ThirdWaveNumber": 3,
    "DevilSquareInitializer.BossWaveNumber": 10,
}

TRIGGER_KINDS = {  # SpawnTrigger enum -> spec kind; wave kinds carry "wave"
    "Automatic": "automatic",
    "Wandering": "wandering",
    "OnceAtEventStart": "once_at_event_start",
    "ManuallyForEvent": "manually_for_event",
    "AutomaticDuringWave": "automatic_during_wave",
    "OnceAtWaveStart": "once_at_wave_start",
}
WAVE_TRIGGERS = {"automatic_during_wave", "once_at_wave_start"}

# All maps get the default drop groups registered in
# GameConfigurationInitializerBase.AddItemDropGroups (registration order).
# default_excellent exists because the 0.95d baseline has excellent options.
DEFAULT_DROP_GROUPS = ["default_money", "default_random_item",
                       "default_excellent", "default_jewels"]

# The 14 maps of the 0.95d dataset (Version095d/GameMapsInitializer.cs).
# files = (source path, source_version of the spawns it contributes), in
# base -> derived order. att = the terrain resource the 0.95d dataset resolves
# (per TerrainVersionPrefix: Version075.Maps.BaseMapInitializer = "075_";
# Exile/Arena extend Initialization.BaseMapInitializer directly = no prefix;
# Version095d.Maps.Devias overrides the prefix away; 095d Lorencia/Noria
# inherit "075_" and therefore keep the 0.75 terrain).
MAPS = [
    {"number": 0, "disc": 0, "version": "075", "att": "075_Terrain1.att",
     "files": [("Version075/Maps/Lorencia.cs", "075"),
               ("Version095d/Maps/Lorencia.cs", "095d")]},
    {"number": 1, "disc": 0, "version": "075", "att": "075_Terrain2.att",
     "files": [("Version075/Maps/Dungeon.cs", "075")]},
    {"number": 2, "disc": 0, "version": "095d", "att": "Terrain3.att",
     "review": "0.75-era map re-derived by the 0.95d dataset with 0.95 terrain "
               "(only town map whose initializer overrides the 075_ terrain "
               "prefix); tagged 095d because the shipped record differs from "
               "0.75 by terrain only",
     "files": [("Version075/Maps/Devias.cs", "075"),
               ("Version095d/Maps/Devias.cs", "095d")]},
    {"number": 3, "disc": 0, "version": "075", "att": "075_Terrain4.att",
     "files": [("Version075/Maps/Noria.cs", "075"),
               ("Version095d/Maps/Noria.cs", "095d")]},
    {"number": 4, "disc": 0, "version": "075", "att": "075_Terrain5.att",
     "files": [("Version075/Maps/LostTower.cs", "075")]},
    {"number": 5, "disc": 0, "version": "075", "att": "Terrain6.att",
     "files": [("Version075/Maps/Exile.cs", "075")]},
    {"number": 6, "disc": 0, "version": "075", "att": "Terrain7.att",
     "files": [("Version075/Maps/Arena.cs", "075")]},
    {"number": 7, "disc": 0, "version": "075", "att": "075_Terrain8.att",
     "files": [("Version075/Maps/Atlans.cs", "075")]},
    {"number": 8, "disc": 0, "version": "095d", "att": "Terrain9.att",
     "files": [("Version095d/Maps/Tarkan.cs", "095d")]},
    {"number": 10, "disc": 0, "version": "095d", "att": "Terrain11.att",
     "files": [("Version095d/Maps/Icarus.cs", "095d")]},
    {"number": 9, "disc": 1, "version": "095d", "att": "Terrain10_1.att",
     "files": [("Version095d/Maps/DevilSquare1.cs", "095d")]},
    {"number": 9, "disc": 2, "version": "095d", "att": "Terrain10_2.att",
     "files": [("Version095d/Maps/DevilSquare2.cs", "095d")]},
    {"number": 9, "disc": 3, "version": "095d", "att": "Terrain10_3.att",
     "files": [("Version095d/Maps/DevilSquare3.cs", "095d")]},
    {"number": 9, "disc": 4, "version": "095d", "att": "Terrain10_4.att",
     "files": [("Version095d/Maps/DevilSquare4.cs", "095d")]},
]


def read(rel_path):
    with open(os.path.join(INIT, rel_path), encoding="utf-8-sig") as f:
        return f.read()


def rect(x1, y1, x2, y2):
    assert 0 <= x1 <= x2 <= 255 and 0 <= y1 <= y2 <= 255, (x1, y1, x2, y2)
    return {"x1": x1, "y1": y1, "x2": x2, "y2": y2}


# ---------------------------------------------------------------- map metadata

NAME_RE = re.compile(r'const string Name = "([^"]+)"|MapName => "([^"]+)"')
NUMBER_RE = re.compile(r"const byte Number = (\d+);|MapNumber => (\d+);")
SAFEZONE_RE = re.compile(r"SafezoneMapNumber => ([\w.]+)\.Number;")
REQUIREMENT_RE = re.compile(r"this\.CreateRequirement\(Stats\.(\w+), (\d+)\)")
POWERUP_RE = re.compile(r"this\.AddCharacterPowerUp\(Stats\.(\w+), ([\d.]+)")


def class_name_numbers():
    """Bare map class name -> map number, for resolving `X.Number` references."""
    numbers = {}
    for entry in MAPS:
        for rel, _version in entry["files"]:
            match = NUMBER_RE.search(read(rel))
            if match is None:  # 095d town subclasses inherit the 0.75 number
                numbers.setdefault(os.path.basename(rel)[:-3], entry["number"])
                continue
            value = int(match.group(1) or match.group(2))
            numbers[os.path.basename(rel)[:-3]] = value
            assert value == entry["number"], (rel, value, entry["number"])
    return numbers


def parse_map_name(entry):
    for rel, _version in entry["files"]:
        match = NAME_RE.search(read(rel))
        if match:
            return match.group(1) or match.group(2)
    raise AssertionError("no map name in " + str(entry["files"]))


def parse_battle_zone(text):
    if "BattleZoneDefinition" not in text:
        return None
    battle_type = re.search(r"battleZone\.Type = BattleType\.(\w+);", text).group(1)
    points = {}
    for side in ("Left", "Right"):
        x = re.search(rf"battleZone\.{side}TeamSpawnPointX = (\d+);", text).group(1)
        y = re.search(rf"battleZone\.{side}TeamSpawnPointY = (\d+);", text).group(1)
        points[side] = {"x": int(x), "y": int(y)}
    rects = {}
    for field in ("Ground", "LeftGoal", "RightGoal"):
        m = re.search(
            rf"battleZone\.{field} = this\.CreateRectangle\(\d+, (\d+), (\d+), (\d+), (\d+)\);",
            text)
        x1, y1, x2, y2 = (int(g) for g in m.groups())
        rects[field] = rect(x1, y1, x2, y2)
    return {
        "battle_type": battle_type.lower(),
        "ground": rects["Ground"],
        "left_goal": rects["LeftGoal"],
        "right_goal": rects["RightGoal"],
        "left_spawn": points["Left"],
        "right_spawn": points["Right"],
    }


# ---------------------------------------------------------------- spawn areas

SPAWN_RE = re.compile(r"this\.CreateMonsterSpawn\(([^;]*?)\)\s*;", re.S)
NPC_REF_RE = re.compile(r"this\.NpcDictionary\[(\d+)\]")
CONST_RE = re.compile(r"const (?:byte|short|int) (\w+) = (\d+);")
# DevilSquare3/4 spawn an S6 monster when available, else a 0.95d-era one:
# `if (this.NpcDictionary.TryGetValue(180|294, ...)) { ... } else { ... }`.
# Monsters 180 (Shriker, Kalima) and 294 (Axe Warrior) are not in the 0.95d
# monster set, so the baseline always takes the else branch.
TRY_GET_RE = re.compile(
    r"if \(this\.NpcDictionary\.TryGetValue\((\d+), [^)]*\)\)\s*"
    r"\{(?:[^{}]*)\}\s*else\s*\{([^{}]*)\}", re.S)


def parse_spawns(rel_path, version, map_reference):
    text = read(rel_path)
    for monster_number in TRY_GET_RE.findall(text):
        assert monster_number[0] in ("180", "294"), (rel_path, monster_number)
    text = TRY_GET_RE.sub(lambda m: m.group(2), text)
    text = re.sub(r"//[^\n]*", "", text)  # strip comments (incl. a commented-out
    # Atlans spawn and the trailing `// <monster name>` annotations)
    consts = {name: int(value) for name, value in CONST_RE.findall(text)}
    spawns = []
    for call in SPAWN_RE.findall(text):
        parts = [p.strip() for p in call.split(",")]
        monster = int(NPC_REF_RE.fullmatch(parts[1]).group(1))
        nums, direction, trigger, wave = [], None, "Automatic", None
        for token in parts[2:]:
            if token.startswith("Direction."):
                direction = token[len("Direction."):]
            elif token.startswith("SpawnTrigger."):
                trigger = token[len("SpawnTrigger."):]
            elif token in WAVE_CONSTS:
                wave = WAVE_CONSTS[token]
            elif token.isdigit() or token in consts:
                value = int(token) if token.isdigit() else consts[token]
                if trigger != "Automatic" and len(nums) >= 4:
                    wave = value  # waveNumber comes positionally after the trigger
                else:
                    nums.append(value)
            else:
                raise AssertionError(f"unresolved token {token!r} in {rel_path}")
        if len(nums) == 2:  # point overload: (x, y)
            x, y = nums
            area, quantity = rect(x, y, x, y), 1
        elif len(nums) in (4, 5):  # area overload: (x1, x2, y1, y2[, quantity])
            x1, x2, y1, y2 = nums[:4]
            area = rect(x1, y1, x2, y2)
            quantity = nums[4] if len(nums) == 5 else 1
        else:
            raise AssertionError(f"odd spawn arity {nums} in {rel_path}")
        kind = TRIGGER_KINDS[trigger]
        trigger_obj = {"kind": kind}
        if kind in WAVE_TRIGGERS:
            assert wave is not None, (rel_path, call)
            trigger_obj["wave"] = wave
        else:
            assert wave is None, (rel_path, call)
        spawns.append({
            "map": map_reference,
            "monster": monster,
            "area": area,
            "quantity": quantity,
            "direction": DIRECTIONS.get(direction_value(direction)),
            "trigger": trigger_obj,
            "source_version": version,
        })
    return spawns


def direction_value(name):
    if name is None or name == "Undefined":
        return 0
    return {v: k for k, v in DIRECTIONS.items()}[slugify(name)]


# ---------------------------------------------------------------- gates/warps

EXIT_RE = re.compile(
    r"targetGates\.Add\((\d+), this\.CreateExitGate\(maps\[(?:new\((\d+), (\d+)\)|(\d+))\],"
    r" (\d+), (\d+), (\d+), (\d+), (\d+)(, true)?\)\);")
ENTER_RE = re.compile(
    r"maps\[(?:new\((\d+), (\d+)\)|(\d+))\]\.EnterGates\.Add\(this\.CreateEnterGate\("
    r"(\d+), targetGates\[(\d+)\], (\d+), (\d+), (\d+), (\d+), (\d+)\)\);")
WARP_RE = re.compile(
    r'this\.CreateWarpInfo\((\d+), "([^"]+)", (\d+), (\d+), gates\[(\d+)\]\)')


def parse_gates(rel_path):
    """Returns (exit_gates, enter_gates, warps) without source_version tags."""
    text = read(rel_path)
    exits, enters, warps = [], [], []
    for m in EXIT_RE.finditer(text):
        number, dn, dd, plain, x1, y1, x2, y2, direction, spawn_flag = m.groups()
        map_number, disc = (int(dn), int(dd)) if plain is None else (int(plain), 0)
        exits.append({
            "kind": "exit_gate",
            "number": int(number),
            "map": map_ref(map_number, disc),
            "area": rect(int(x1), int(y1), int(x2), int(y2)),
            "direction": DIRECTIONS.get(int(direction)),
            "is_spawn_gate": spawn_flag is not None,
        })
    for m in ENTER_RE.finditer(text):
        dn, dd, plain, number, target, x1, y1, x2, y2, min_level = m.groups()
        map_number, disc = (int(dn), int(dd)) if plain is None else (int(plain), 0)
        enters.append({
            "kind": "enter_gate",
            "number": int(number),
            "map": map_ref(map_number, disc),
            "area": rect(int(x1), int(y1), int(x2), int(y2)),
            "target_gate": int(target),
            "min_level": int(min_level),
        })
    for m in WARP_RE.finditer(text):
        index, name, cost, min_level, gate = m.groups()
        warps.append({
            "kind": "warp",
            "index": int(index),
            "name": name,
            "cost_zen": int(cost),
            "min_level": int(min_level),
            "target_gate": int(gate),
        })
    return exits, enters, warps


def tag_gate_versions(records_095d, records_075, key_field):
    """095d is the baseline; anything identical in 0.75 is tagged 075."""
    from_075 = {r[key_field]: r for r in records_075}
    for record in records_095d:
        older = from_075.get(record[key_field])
        record["source_version"] = "075" if older == record else "095d"
    missing = set(from_075) - {r[key_field] for r in records_095d}
    assert not missing, f"0.75 gates dropped by 0.95d: {missing}"
    return records_095d


# ------------------------------------------------------------------- terrain

ATT_HEADER = b"\x00\xff\xff"
CELLS = 256 * 256


def reencode_terrain(att_name, out_name):
    """Returns (out_path_rel_to_data, count_of_cells_with_unknown_bits)."""
    with open(os.path.join(RESOURCES, att_name), "rb") as f:
        blob = f.read()
    assert len(blob) == 3 + CELLS and blob[:3] == ATT_HEADER, att_name
    unknown_bits = 0
    out = bytearray(CELLS)
    for i, value in enumerate(blob[3:]):
        if value & ~0x1F:
            unknown_bits += 1
        # safezone=1 stays bit 0; blocked 4->2, no_ground 8->4, water 16->8;
        # character (2, runtime occupancy) and unknown high bits are dropped.
        out[i] = (value & 0x01) | ((value & 0x1C) >> 1)
    out_path = os.path.join(DATA_DIR, "terrain", out_name)
    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    with open(out_path, "wb") as f:
        f.write(bytes(out))
    return "terrain/" + out_name, unknown_bits


# ---------------------------------------------------------------------- main

def main():
    stat_map = load_stat_map()
    class_numbers = class_name_numbers()

    # Gates first: the safezone default rule needs spawn-gate presence per map.
    exits_095d, enters_095d, warps_095d = parse_gates("Version095d/Gates.cs")
    exits_075, enters_075, warps_075 = parse_gates("Version075/Gates.cs")
    exits = tag_gate_versions(exits_095d, exits_075, "number")
    enters = tag_gate_versions(enters_095d, enters_075, "number")
    warps = tag_gate_versions(warps_095d, warps_075, "index")
    assert len(warps) == 14, len(warps)
    assert all(w["source_version"] == "075" for w in warps), \
        "0.95d warp list expected to be the verbatim 0.75 copy"
    exit_numbers = {g["number"] for g in exits}
    for gate in enters:
        assert gate["target_gate"] in exit_numbers, gate
    for warp in warps:
        assert warp["target_gate"] in exit_numbers, warp
    spawn_gate_maps = {g["map"]["number"] for g in exits if g["is_spawn_gate"]}

    map_records, spawn_records = [], []
    terrain_notes = {}
    for entry in MAPS:
        name = parse_map_name(entry)
        map_id = slugify(name)
        reference = map_ref(entry["number"], entry["disc"])
        combined = "".join(read(rel) for rel, _v in entry["files"])

        safezone_override = SAFEZONE_RE.search(combined)
        if safezone_override:
            target_class = safezone_override.group(1).split(".")[-1]
            safezone_number = class_numbers[target_class]
        elif entry["number"] in spawn_gate_maps:
            safezone_number = entry["number"]
        else:
            safezone_number = 0  # Lorencia, per BaseMapInitializer default

        requirements = [{"stat": stat_map[stat], "value": int(value)}
                        for stat, value in REQUIREMENT_RE.findall(combined)]
        power_ups = [{"stat": stat_map[stat], "value": float(value), "aggregate": "add_raw"}
                     for stat, value in POWERUP_RE.findall(combined)]
        if "AddUnderwaterMovementPowerUp()" in combined:
            power_ups.append({"stat": stat_map["IsUnderwater"], "value": 1.0,
                              "aggregate": "add_raw"})

        terrain_path, unknown = reencode_terrain(
            entry["att"], f"{entry['version']}_{map_id}.bin")
        if unknown:
            terrain_notes[map_id] = unknown

        record = {
            "number": entry["number"],
            "discriminator": entry["disc"],
            "id": map_id,
            "name": name,
            "source_version": entry["version"],
        }
        if "review" in entry:
            record["review"] = entry["review"]
        record.update({
            "terrain": terrain_path,
            "exp_multiplier": 1.0,  # ExpMultiplier = 1 for every shipped map
            "safezone_map": map_ref(safezone_number, 0),
            "requirements": requirements,
            "character_power_ups": power_ups,
            "drop_groups": list(DEFAULT_DROP_GROUPS),
            "battle_zone": parse_battle_zone(combined),
        })
        map_records.append(record)

        for rel, version in entry["files"]:
            spawn_records.extend(parse_spawns(rel, version, reference))

    assert len(map_records) == 14
    map_keys = {(m["number"], m["discriminator"]) for m in map_records}
    for spawn in spawn_records:
        assert (spawn["map"]["number"], spawn["map"]["discriminator"]) in map_keys
    for gate in exits + enters:
        assert (gate["map"]["number"], gate["map"]["discriminator"]) in map_keys
    wave_spawns = [s for s in spawn_records if s["trigger"]["kind"] in WAVE_TRIGGERS]
    assert {s["trigger"]["wave"] for s in wave_spawns} == {1, 2, 3, 10}

    gate_records = exits + enters + warps
    write_datafile("map_definitions.json", map_records)
    write_datafile("spawn_areas.json", spawn_records)
    write_datafile("gates_warps.json", gate_records)

    def by_version(records):
        counts = {}
        for record in records:
            counts[record["source_version"]] = counts.get(record["source_version"], 0) + 1
        return counts

    reviews = [{"file": "map_definitions.json", "id": r["id"], "review": r["review"]}
               for r in map_records if "review" in r]
    coverage_path = coverage("maps", {
        "files": {
            "map_definitions.json": len(map_records),
            "spawn_areas.json": len(spawn_records),
            "gates_warps.json": len(gate_records),
            "terrain_binaries": len(MAPS),
        },
        "counts_by_source_version": {
            "map_definitions.json": by_version(map_records),
            "spawn_areas.json": by_version(spawn_records),
            "gates_warps.json": {
                "exit_gate": by_version(exits),
                "enter_gate": by_version(enters),
                "warp": by_version(warps),
            },
        },
        "review": reviews,
        "gaps": [
            "devil square mini-game event config (enter/game/exit durations, entry "
            "level ranges per square 1-4, rewards, ticket item 14/19, max 10 players; "
            "Version095d/Events/DevilSquareInitializer.cs) - no mini-game schema in "
            "the spec this wave; the wave-tagged spawns, maps and entrance exit "
            "gates 58-61 ARE extracted",
            "authentic 0.95d warp fees/levels unknown - OpenMU ships the 0.75 warp "
            "list verbatim with a 'todo: update for 0.95d'; adopted as-is, all 14 "
            "entries tagged 075",
            "1.0-era Blood Castle maps/spawns exist only in the S6 dataset and are "
            "outside the approved 14-map scope of this wave - no map backports",
            "invasion mobs (43/44/53/54) have monster definitions but no static "
            "spawn areas - invasions spawn dynamically at runtime (not spawn data)",
        ],
        "notes": [
            "facts file 4_*.md claims Lorencia/Devias/Noria all get 0.95 terrain; "
            "the source shows only 095d Devias overrides the 075_ terrain prefix - "
            "Lorencia/Noria keep 0.75 terrain in the 0.95d dataset and stay tagged 075",
            "facts file counts (32/36 exit, 23 enter gates) differ from source: "
            f"parsed {len(exits)} exit ({sum(1 for g in exits if g['source_version'] == '075')} "
            f"already in 0.75) and {len(enters)} enter gates",
            "0.75 Exile/Arena use the unprefixed Terrain6/7.att resources (no "
            "075_ variant exists); re-encoded as 075_exile.bin/075_arena.bin",
            "default_excellent drop group reference on every map reflects the 0.95d "
            "baseline (excellent options do not exist in the pure 0.75 dataset)",
            "terrain cells with undefined high bits (masked off): "
            + (str(terrain_notes) if terrain_notes else "none"),
            "map records tagged 075 are byte-identical between the 0.75 and 0.95d "
            "datasets (including terrain); spawn additions by 095d subclasses are "
            "separate 095d-tagged spawn records",
            "DevilSquare3/4 conditionally spawn S6 monsters 180 (Shriker) / 294 "
            "(Axe Warrior) when present; neither exists in 0.95d, so the shipped "
            "else-branch spawns (34 Cursed Wizard / 69 Alquamos) were extracted "
            "and the S6-branch calls dropped (2 calls)",
            "075 spawn count 1563 vs facts-file 1564: the extra call in the facts "
            "count is a commented-out (and syntactically broken) Atlans line",
        ],
    })
    print("wrote", coverage_path)
    print("maps:", by_version(map_records))
    print("spawns:", by_version(spawn_records))
    print("gates_warps:", by_version(gate_records))


if __name__ == "__main__":
    main()
