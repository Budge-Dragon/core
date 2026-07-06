#!/usr/bin/env python3
"""Extract maps, gates/warps and terrain (v2 maps_gates domain).

Outputs:
  data/map_definitions.json    11 records (maps 0-10; Devil Square collapsed
                               to one map-9 record)
  data/gates_warps.json        71 records, kind-tagged:
                               spawn_gate | target_gate | enter_gate | warp
  data/terrain/0.bin..10.bin   11 sidecars, keyed by map number (re-encoded
                               from OpenMU .att; the four byte-identical Devil
                               Square blobs collapse to terrain/9.bin)
  data/_coverage/maps.json     counts, review list, named gaps

Baseline = the 0.95d dataset. source_version = "075" when the shipped v2
record is defined by a Version075 initializer that 0.95d reuses unchanged in
every field the v2 record carries, "095d" otherwise.

Provenance is flattened onto each record (source_version + optional review).

Gate.txt's three-valued flag becomes three record kinds: OpenMU's ExitGate
with isSpawnGate=true is flag 0 -> spawn_gate; ExitGate with the default
false is flag 2 -> target_gate; EnterGate is flag 1 -> enter_gate. Warps are
the Move.txt list.

Terrain: source .att = 3-byte header (00 ff ff) + 65536 cells (index =
y*256+x). Source flag bits Safezone=1, Character=2 (runtime occupancy,
dropped), Blocked=4, NoGround=8, Water=16 are re-encoded to our bits
safezone=1, blocked=2, no_ground=4, water=8. Bits above 0x1F (noise in a few
Icarus cells) are masked off and counted in the coverage report. Core never
reads a filesystem: no record carries a terrain path; the host loads
terrain/<mapnumber>.bin by convention.
"""

import glob
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import coverage, without_name, write_datafile, write_names, DATA_DIR

INIT = "/tmp/openmu-ref/src/Persistence/Initialization"
RESOURCES = os.path.join(INIT, "Resources")

# Gate direction byte: 0 = undefined (field omitted); 1-8 are the 8 compass
# points. Source comment: "0 means 'Undefined' ... without adding 1".
DIRECTIONS = {
    1: "west", 2: "south_west", 3: "south", 4: "south_east",
    5: "east", 6: "north_east", 7: "north", 8: "north_west",
}

# The 11 client maps of the 0.95d dataset, one entry per Devil Square square
# so each square's terrain resolves; the four map-9 squares collapse to a
# single record and a single terrain sidecar (their blobs are byte-identical).
# att = the terrain resource the 0.95d dataset resolves (per
# TerrainVersionPrefix: Version075.Maps.BaseMapInitializer = "075_"; Exile and
# Arena extend Initialization.BaseMapInitializer directly = no prefix;
# Version095d.Maps.Devias overrides the prefix away; 095d Lorencia/Noria
# inherit "075_" and keep the 0.75 terrain).
MAPS = [
    {"number": 0, "version": "075", "att": "075_Terrain1.att",
     "files": [("Version075/Maps/Lorencia.cs", "075"),
               ("Version095d/Maps/Lorencia.cs", "095d")]},
    {"number": 1, "version": "075", "att": "075_Terrain2.att",
     "files": [("Version075/Maps/Dungeon.cs", "075")]},
    {"number": 2, "version": "095d", "att": "Terrain3.att",
     "review": "0.75-era town re-derived by the 0.95d dataset with 0.95 "
               "terrain (only town map whose initializer overrides the 075_ "
               "terrain prefix); tagged 095d because the shipped map carries "
               "the 0.95d terrain sidecar (terrain/2.bin = Terrain3.att) — the "
               "record fields (number, name, environment) are the 0.75 map",
     "files": [("Version075/Maps/Devias.cs", "075"),
               ("Version095d/Maps/Devias.cs", "095d")]},
    {"number": 3, "version": "075", "att": "075_Terrain4.att",
     "files": [("Version075/Maps/Noria.cs", "075"),
               ("Version095d/Maps/Noria.cs", "095d")]},
    {"number": 4, "version": "075", "att": "075_Terrain5.att",
     "files": [("Version075/Maps/LostTower.cs", "075")]},
    {"number": 5, "version": "075", "att": "Terrain6.att",
     "files": [("Version075/Maps/Exile.cs", "075")]},
    {"number": 6, "version": "075", "att": "Terrain7.att",
     "review": "pitch coordinates are measurable from the Arena terrain; "
               "placing battle soccer in the 0.75 era follows OpenMU's dataset "
               "— the feature historically appears ~0.97+",
     "files": [("Version075/Maps/Arena.cs", "075")]},
    {"number": 7, "version": "075", "att": "075_Terrain8.att",
     "files": [("Version075/Maps/Atlans.cs", "075")]},
    {"number": 8, "version": "095d", "att": "Terrain9.att",
     "files": [("Version095d/Maps/Tarkan.cs", "095d")]},
    # respawn_map override: Icarus.cs:44 SafezoneMapNumber => LostTower.Number
    # (4). Icarus owns no town spawn gate, so its default would be Lorencia (0);
    # the override sends a sky death down to the ground of Lost Tower.
    {"number": 10, "version": "095d", "att": "Terrain11.att",
     "respawn_map": 4,
     "files": [("Version095d/Maps/Icarus.cs", "095d")]},
    # respawn_map override: DevilSquare1.cs:45 SafezoneMapNumber => Noria.Number
    # (3). The arena owns its own gate 58, so its default would be self (9); the
    # override sends a Devil-Square death out to the event's host town, Noria.
    {"number": 9, "version": "095d", "att": "Terrain10_1.att",
     "name": "Devil Square",
     "respawn_map": 3,
     "review": "collapsed from OpenMU's four discriminator records (one client "
               "map; the four squares are event brackets, owned by W-DS); the "
               "four OpenMU terrain blobs were verified byte-identical before "
               "collapsing",
     "files": [("Version095d/Maps/DevilSquare1.cs", "095d"),
               ("Version095d/Maps/DevilSquare2.cs", "095d"),
               ("Version095d/Maps/DevilSquare3.cs", "095d"),
               ("Version095d/Maps/DevilSquare4.cs", "095d")]},
]

# OpenMU reuses the 0.75 warp fee/level table verbatim as the 0.95d list (its
# initializer carries a todo to update it); no authentic 0.95 table is sourced.
WARP_REVIEW = (
    "OpenMU ships the 0.75 warp fee/level list verbatim as the 0.95d list (its "
    "initializer carries a todo to update it); no authentic 0.95 fee/level "
    "table has been sourced")


def read(rel_path):
    with open(os.path.join(INIT, rel_path), encoding="utf-8-sig") as f:
        return f.read()


def rect(x1, y1, x2, y2):
    assert 0 <= x1 <= x2 <= 255 and 0 <= y1 <= y2 <= 255, (x1, y1, x2, y2)
    return {"x1": x1, "y1": y1, "x2": x2, "y2": y2}


# ---------------------------------------------------------------- map metadata

NAME_RE = re.compile(r'const string Name = "([^"]+)"|MapName => "([^"]+)"')
CANFLY_RE = re.compile(r"CreateRequirement\(Stats\.CanFly")


def parse_map_name(entry):
    if "name" in entry:  # collapsed client name (Devil Square 1..4 -> one map)
        return entry["name"]
    for rel, _version in entry["files"]:
        match = NAME_RE.search(read(rel))
        if match:
            return match.group(1) or match.group(2)
    raise AssertionError("no map name in " + str(entry["files"]))


def parse_environment(combined):
    """Traversal medium, derived from the OpenMU map initializer.

    Atlans registers an underwater-movement power-up; Icarus carries a CanFly
    entry requirement (a sky map); every other map is ordinary ground.
    """
    if "AddUnderwaterMovementPowerUp()" in combined:
        return "underwater"
    if CANFLY_RE.search(combined):
        return "sky"
    return "ground"


def parse_soccer_pitch(text):
    """Arena battle-soccer pitch; None on every other map. BattleType is
    dropped (a pitch is soccer by construction)."""
    if "BattleZoneDefinition" not in text:
        return None
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
        "ground": rects["Ground"],
        "left_goal": rects["LeftGoal"],
        "right_goal": rects["RightGoal"],
        "left_spawn": points["Left"],
        "right_spawn": points["Right"],
    }


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
    """Parse one Gates.cs into v1-shaped gate dicts (used for version tagging;
    reshaped to the v2 kinds after tagging).

    Returns (exits, enters, warps). map is the bare client map number (the
    OpenMU discriminator is discarded — Devil Square gates 58-61 all resolve to
    map 9). is_spawn_gate distinguishes Gate.txt flag 0 (spawn) from flag 2
    (target).
    """
    text = read(rel_path)
    exits, enters, warps = [], [], []
    for m in EXIT_RE.finditer(text):
        number, dn, _dd, plain, x1, y1, x2, y2, direction, spawn_flag = m.groups()
        map_number = int(dn) if plain is None else int(plain)
        exits.append({
            "number": int(number),
            "map": map_number,
            "area": rect(int(x1), int(y1), int(x2), int(y2)),
            "direction": DIRECTIONS.get(int(direction)),
            "is_spawn_gate": spawn_flag is not None,
        })
    for m in ENTER_RE.finditer(text):
        dn, _dd, plain, number, target, x1, y1, x2, y2, min_level = m.groups()
        map_number = int(dn) if plain is None else int(plain)
        enters.append({
            "number": int(number),
            "map": map_number,
            "area": rect(int(x1), int(y1), int(x2), int(y2)),
            "target_gate": int(target),
            "min_level": int(min_level),
        })
    for m in WARP_RE.finditer(text):
        index, name, cost, min_level, gate = m.groups()
        warps.append({
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


def emit_exit(gate):
    kind = "spawn_gate" if gate["is_spawn_gate"] else "target_gate"
    record = {
        "kind": kind,
        "number": gate["number"],
        "map": gate["map"],
        "area": gate["area"],
    }
    if gate["direction"] is not None:  # byte 0 -> unspecified, field omitted
        record["direction"] = gate["direction"]
    record["source_version"] = gate["source_version"]
    if gate.get("review"):  # curated gates carry a provenance note; parsed ones do not
        record["review"] = gate["review"]
    return record


def emit_enter(gate):
    record = {
        "kind": "enter_gate",
        "number": gate["number"],
        "map": gate["map"],
        "area": gate["area"],
        "target_gate": gate["target_gate"],
    }
    if gate["min_level"] != 0:  # Gate.txt level-0 sentinel -> typed absence
        record["min_level"] = gate["min_level"]
    record["source_version"] = gate["source_version"]
    return record


def emit_warp(warp):
    return {
        "kind": "warp",
        "index": warp["index"],
        "name": warp["name"],
        "cost_zen": warp["cost_zen"],
        "min_level": warp["min_level"],
        "target_gate": warp["target_gate"],
        "source_version": warp["source_version"],
        "review": WARP_REVIEW,
    }


# ------------------------------------------------------------------- terrain

ATT_HEADER = b"\x00\xff\xff"
CELLS = 256 * 256


def reencode_terrain(att_name, map_number):
    """Re-encode one .att into terrain/<map_number>.bin. Returns the count of
    cells carrying undefined high bits (masked off)."""
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
    out_path = os.path.join(DATA_DIR, "terrain", f"{map_number}.bin")
    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    with open(out_path, "wb") as f:
        f.write(bytes(out))
    return unknown_bits


def clear_terrain():
    """Drop stale sidecars (v1 named them <version>_<slug>.bin) so the
    directory holds exactly the 11 number-keyed v2 files."""
    for path in glob.glob(os.path.join(DATA_DIR, "terrain", "*.bin")):
        os.remove(path)


# ---------------------------------------------------------------------- main

def main():
    # Gates first, tagged on the v1-shaped dicts, then reshaped to v2 kinds.
    exits_095d, enters_095d, warps_095d = parse_gates("Version095d/Gates.cs")
    exits_075, enters_075, warps_075 = parse_gates("Version075/Gates.cs")
    exits = tag_gate_versions(exits_095d, exits_075, "number")
    enters = tag_gate_versions(enters_095d, enters_075, "number")
    warps = tag_gate_versions(warps_095d, warps_075, "index")
    assert len(warps) == 14, len(warps)

    # Curated backport: Tarkan's town spawn gate lives only in
    # VersionSeasonSix/Gates.cs:198 (CreateExitGate(maps[8], 187,63,203,69, 0,
    # true)); the 0.95d Gates.cs omits it, which exiled Tarkan deaths to
    # Lorencia. Adopt gate 57 as an s6 backport (rect verified fully walkable on
    # terrain/8.bin) so map 8 owns a spawn gate and its respawn resolves to
    # itself. Added as a literal rather than parsing all of SeasonSix Gates.cs,
    # which would pull in dozens of unrelated later-era gates.
    exits.append({
        "number": 57,
        "map": 8,
        "area": rect(187, 63, 203, 69),
        "direction": None,  # SeasonSix byte 0 -> unspecified facing
        "is_spawn_gate": True,
        "source_version": "s6",
        "review": "Tarkan's town spawn gate is defined only in "
                  "VersionSeasonSix/Gates.cs; the 0.95d Gates.cs omits it. "
                  "Adopted as an s6 backport so Tarkan deaths respawn in Tarkan; "
                  "rect (187,63)-(203,69) verified fully walkable on terrain/8.bin",
    })
    assert all(w["source_version"] == "075" for w in warps), \
        "0.95d warp list expected to be the verbatim 0.75 copy"

    exit_numbers = {g["number"] for g in exits}
    for gate in enters:
        assert gate["target_gate"] in exit_numbers, gate  # never targets enter
    for warp in warps:
        assert warp["target_gate"] in exit_numbers, warp

    # Maps that own a spawn gate — a death respawns on the map it died on when
    # the map is in this set, else it defaults to Lorencia (0). Re-derives
    # BaseMapInitializer.cs:91 from our parsed gate data (the curated Tarkan gate
    # 57 puts map 8 in the set). Explicit per-map overrides beat the default.
    spawn_owner = {g["map"] for g in exits if g["is_spawn_gate"]}

    clear_terrain()
    map_records = []
    terrain_notes = {}
    for entry in MAPS:
        number = entry["number"]
        combined = "".join(read(rel) for rel, _v in entry["files"])
        default_respawn = number if number in spawn_owner else 0
        record = {
            "number": number,
            "name": parse_map_name(entry),
            "environment": parse_environment(combined),
            "respawn_map": entry.get("respawn_map", default_respawn),
        }
        pitch = parse_soccer_pitch(combined)
        if pitch is not None:
            record["soccer_pitch"] = pitch
        record["source_version"] = entry["version"]
        if "review" in entry:
            record["review"] = entry["review"]
        map_records.append(record)

        unknown = reencode_terrain(entry["att"], number)
        if unknown:
            terrain_notes[number] = unknown

    assert len(map_records) == 11, len(map_records)
    map_numbers = {m["number"] for m in map_records}
    assert len(map_numbers) == 11, "duplicate map number"

    gate_records = ([emit_exit(g) for g in exits]
                    + [emit_enter(g) for g in enters]
                    + [emit_warp(w) for w in warps])
    for record in gate_records:
        assert record["map"] in map_numbers if "map" in record else True, record
    assert len(gate_records) == 71, len(gate_records)

    # Display names -> host-owned sidecars; the core files carry only
    # identities and rules. Map names key by number; warp names by list index
    # (only warp gate records carry a name).
    write_names("map_definitions.json", {"records": [
        {"number": r["number"], "name": r["name"]} for r in map_records]})
    write_names("gates_warps.json", {"records": [
        {"index": r["index"], "name": r["name"]}
        for r in gate_records if r["kind"] == "warp"]})
    write_datafile("map_definitions.json", [without_name(r) for r in map_records])
    write_datafile("gates_warps.json", [without_name(r) for r in gate_records])

    # ---- coverage ----
    def by_version(records):
        counts = {}
        for record in records:
            v = record["source_version"]
            counts[v] = counts.get(v, 0) + 1
        return counts

    spawn_gates = [r for r in gate_records if r["kind"] == "spawn_gate"]
    target_gates = [r for r in gate_records if r["kind"] == "target_gate"]
    enter_gates = [r for r in gate_records if r["kind"] == "enter_gate"]
    warp_records = [r for r in gate_records if r["kind"] == "warp"]

    reviews = [{"file": "map_definitions.json", "number": r["number"],
                "review": r["review"]}
               for r in map_records if "review" in r]
    reviews.append({"file": "gates_warps.json", "kind": "warp",
                    "records": len(warp_records), "review": WARP_REVIEW})

    coverage_path = coverage("maps", {
        "files": {
            "map_definitions.json": len(map_records),
            "gates_warps.json": len(gate_records),
            "terrain_binaries": len(map_records),
        },
        "counts_by_source_version": {
            "map_definitions.json": by_version(map_records),
            "gates_warps.json": {
                "spawn_gate": by_version(spawn_gates),
                "target_gate": by_version(target_gates),
                "enter_gate": by_version(enter_gates),
                "warp": by_version(warp_records),
            },
        },
        "review": reviews,
        "gaps": [
            "Devil Square mini-game event config (per-square entry level "
            "brackets, waves, rewards, timers, in-event death, ticket item, "
            "max 10 players; Version095d/Events/DevilSquareInitializer.cs) is "
            "W-DS scope, not maps data; the single collapsed map-9 record and "
            "its four arrival gates (spawn gates 58-61) ARE extracted",
            "authentic 0.95d warp fees/levels unknown - OpenMU ships the 0.75 "
            "warp list verbatim with a 'todo: update for 0.95d'; adopted as-is, "
            "all 14 warp records tagged 075 with a review note",
            "S6/1.0-era maps (Blood Castle, Kalima, ...) are outside the "
            "approved 11-map 0.95d scope of this wave - no map backports",
        ],
        "notes": [
            "Devil Square: OpenMU's four discriminator records (map 9 disc "
            "1-4, names 'Devil Square 1'..'4') collapse to ONE map-9 record "
            "named 'Devil Square'; the four Terrain10_{1..4}.att blobs are "
            "byte-identical (verified md5) and collapse to terrain/9.bin",
            "Devias (map 2) tagged 095d: its record fields are the 0.75 map, "
            "but the shipped terrain sidecar (terrain/2.bin) is the 0.95d "
            "Terrain3.att (only town whose initializer overrides the 075_ "
            "prefix); the v1 record-level terrain field is gone in v2, so the "
            "095d tag now reflects the sidecar, not a record-field difference",
            "environment derived from the initializer: Atlans (7) = underwater "
            "(AddUnderwaterMovementPowerUp), Icarus (10) = sky (CanFly entry "
            "requirement), all other maps = ground; OpenMU's IsUnderwater stat "
            "and CanFly stat-requirement mechanisms are dropped (become W-CMB / "
            "services rules on the typed environment)",
            "Arena soccer_pitch present on map 6 only; BattleType.Soccer tag "
            "dropped (a pitch is soccer by construction); era placement flagged "
            "as a review note on the record",
            "gate map references are bare client map numbers (u8); the OpenMU "
            "MapRef discriminator is gone (Devil Square gates 58-61 -> map 9)",
            "enter gates 3 and 20 carry Gate.txt's level-0 no-requirement "
            "sentinel -> min_level omitted (typed absence, never Level(0))",
            "every spawn-gate direction byte is 0 -> direction omitted; target "
            "gates carry real facing bytes 1-8",
            "terrain cells with undefined high bits (masked off), by map "
            "number: " + (str(terrain_notes) if terrain_notes else "none"),
        ],
    })

    print("wrote", coverage_path)
    print("map_definitions:", by_version(map_records), "total", len(map_records))
    print("gates_warps by kind: spawn_gate", len(spawn_gates),
          "target_gate", len(target_gates), "enter_gate", len(enter_gates),
          "warp", len(warp_records), "total", len(gate_records))
    print("gates_warps by source_version:", by_version(gate_records))


if __name__ == "__main__":
    main()
