#!/usr/bin/env python3
"""Extract the classic Monster.txt roster and world population (v2 schema).

Outputs:
  data/monster_definitions.json   MonsterDefinition records (role kind-tagged)
  data/spawns.json                Spawn records (placement + schedule kind-tagged)
  data/_coverage/monsters.json    counts, review list, named gaps

Sources (approved): Version075 map files' CreateMonsters + NpcInitialization
+ the Bali summon (#150) in SkillsInitializer; Version095d map additions
(Tarkan, Icarus), invasion mobs (#43/#44/#53/#54) and NPCs (#235-237).
Spawns come from every map file's CreateMonsterSpawns; the Devil Square wave
rows leave this file entirely (scope boundary -> W-DS).

v2 shape:
  * The v1 stat slug-list dies. Combat columns become the typed MonsterCombat /
    MobBehavior structs on the fighting role variants; resistances become raw
    0..255 bytes in a total PerElement (all 7 keys ice/poison/lightning/fire/
    earth/wind/water). OpenMU's `water_resistance` slot is the pre-S3 lightning
    column (review-tagged); earth/wind/water serialize as explicit 0.
  * attack is kind-tagged {plain | skill{skill}}. The 14 records OpenMU points
    at skill 150 (an S6 phantom pre-S3) ship as plain with a review note; no
    phantom skill number ships anywhere.
  * Spawn placement is kind-tagged {fixed | spot | area}; schedule is
    {permanent | wandering}. Map is a bare MapNumber. Devil Square wave rows
    are omitted (their residue lives in the monsters_spawns v2 section).

Killed with the mechanism they belonged to: NumberOfMaximumItemDrops (drops
service const), monster-bound drop groups (drops domain, keyed by number),
poison_damage_multiplier (skills_effects poison-tick rule), the opaque
Attribute byte, MapRef.discriminator on spawns. None of them appear here.
"""

import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import coverage, without_name, write_datafile, write_names

OPENMU = "/tmp/openmu-ref/src/Persistence/Initialization"
SKILL_NUMBER_CS = os.path.join(OPENMU, "Skills/SkillNumber.cs")

# --- monster definition sources (oldest first: 075 wins version-tag ties) ----
MONSTER_FILES = {
    "075": [
        "Version075/Maps/Lorencia.cs",
        "Version075/Maps/Dungeon.cs",
        "Version075/Maps/Devias.cs",
        "Version075/Maps/Noria.cs",
        "Version075/Maps/LostTower.cs",
        "Version075/Maps/Exile.cs",
        "Version075/Maps/Arena.cs",
        "Version075/Maps/Atlans.cs",
        "Version075/NpcInitialization.cs",
        "Version075/SkillsInitializer.cs",  # summon monster Bali #150
    ],
    "095d": [
        "Version095d/Maps/Tarkan.cs",
        "Version095d/Maps/Icarus.cs",
        "Version095d/Maps/DevilSquare1.cs",
        "Version095d/Maps/DevilSquare2.cs",
        "Version095d/Maps/DevilSquare3.cs",
        "Version095d/Maps/DevilSquare4.cs",
        "Version095d/NpcInitialization.cs",
        "Version095d/InvasionMobsInitialization.cs",
        "Version095d/SkillsInitializer.cs",  # Bali again, identical -> deduped
    ],
}

# --- spawn sources: (relative path, source_version, map number) --------------
# Map numbers mirror the maps domain's roster. Devil Square (map 9) files
# contribute only wave rows, which are dropped; no emitted spawn carries map 9,
# so a bare MapNumber (no discriminator) is total for this file.
SPAWN_FILES = [
    ("Version075/Maps/Lorencia.cs", "075", 0),
    ("Version095d/Maps/Lorencia.cs", "095d", 0),
    ("Version075/Maps/Dungeon.cs", "075", 1),
    ("Version075/Maps/Devias.cs", "075", 2),
    ("Version095d/Maps/Devias.cs", "095d", 2),
    ("Version075/Maps/Noria.cs", "075", 3),
    ("Version095d/Maps/Noria.cs", "095d", 3),
    ("Version075/Maps/LostTower.cs", "075", 4),
    ("Version075/Maps/Exile.cs", "075", 5),
    ("Version075/Maps/Arena.cs", "075", 6),
    ("Version075/Maps/Atlans.cs", "075", 7),
    ("Version095d/Maps/Tarkan.cs", "095d", 8),
    ("Version095d/Maps/Icarus.cs", "095d", 10),
    ("Version095d/Maps/DevilSquare1.cs", "095d", 9),
    ("Version095d/Maps/DevilSquare2.cs", "095d", 9),
    ("Version095d/Maps/DevilSquare3.cs", "095d", 9),
    ("Version095d/Maps/DevilSquare4.cs", "095d", 9),
]

# --- role vocabulary ---------------------------------------------------------
NPC_WINDOW = {  # LegacyQuest -> quest; the one vault concept is `vault`.
    "Merchant": "merchant",
    "VaultStorage": "vault",
    "ChaosMachine": "chaos_machine",
    "GuildMaster": "guild_master",
    "DevilSquare": "devil_square",
    "LegacyQuest": "quest",
}

TRAP_TARGETING = {  # RandomAttackInRange... is deleted (unused pre-S3).
    "AttackSingleWhenPressedTrapIntelligence": "single_when_pressed",
    "AttackAreaWhenPressedTrapIntelligence": "area_when_pressed",
    "AttackAreaTargetInDirectionTrapIntelligence": "directional",
}

# --- stat dictionary -> typed columns ----------------------------------------
COMBAT_MAP = {
    "Level": "level",
    "MaximumHealth": "hp",
    "MinimumPhysBaseDmg": "min_phys_damage",
    "MaximumPhysBaseDmg": "max_phys_damage",
    "DefenseBase": "defense",
    "AttackRatePvm": "attack_rate",
    "DefenseRatePvm": "defense_rate",
}
# Every fighting record carries these six; only traps omit DefenseBase (a fixed
# Monster.txt zero, per the section's honesty argument), so `defense` defaults 0.
COMBAT_REQUIRED = ("level", "hp", "min_phys_damage", "max_phys_damage",
                   "attack_rate", "defense_rate")

RESIST_MAP = {  # WaterResistance is the pre-S3 lightning slot (review-tagged).
    "IceResistance": "ice",
    "PoisonResistance": "poison",
    "FireResistance": "fire",
    "WaterResistance": "lightning",
}
ELEMENTS = ("ice", "poison", "lightning", "fire", "earth", "wind", "water")
# WindResistance: sole occurrence (Goblin) is 0 -> the schema-wide explicit
# wind:0; asserted 0 so no authentic byte is lost. PoisonDamageMultiplier:
# moved to the skills_effects poison-tick rule (the poison-attack fact rides on
# attack skill 1). Both are dropped from records here.
DROPPED_STATS = {"WindResistance", "PoisonDamageMultiplier"}

PHANTOM_SKILL = 150  # SkillNumber.MonsterSkill: resolves only via an S6 backport
PHANTOM_REVIEW = ("OpenMU maps this attack to skill 150, which resolves only "
                  "via an S6 skill backport — a phantom pre-S3; modeled as "
                  "plain, verify attack-type against an authentic Monster.txt.")

# Records whose era inside the 095d dataset is doubtful.
ERA_REVIEW = {
    53: "Golden Titan ships in the upstream 095d dataset, but this "
        "golden-invasion tier is commonly dated ~0.97+; kept as 095d per "
        "dataset policy.",
    54: "Golden Soldier ships in the upstream 095d dataset, but this "
        "golden-invasion tier is commonly dated ~0.97+; kept as 095d per "
        "dataset policy.",
}

DIRECTIONS = {  # C# Direction enum member -> snake_case (Undefined = no facing)
    "West": "west", "SouthWest": "south_west", "South": "south",
    "SouthEast": "south_east", "East": "east", "NorthEast": "north_east",
    "North": "north", "NorthWest": "north_west",
}


def read(rel):
    with open(os.path.join(OPENMU, rel), encoding="utf-8-sig") as f:
        return f.read()


def load_skill_numbers():
    numbers = {}
    with open(SKILL_NUMBER_CS, encoding="utf-8") as f:
        for line in f:
            m = re.match(r"\s*(\w+) = (\d+),", line)
            if m:
                numbers[m.group(1)] = int(m.group(2))
    if not numbers:
        raise SystemExit(f"no skill numbers parsed from {SKILL_NUMBER_CS}")
    return numbers


# ------------------------------------------------------------------- monsters

def blocks(text):
    """Yield (var_name, segment) per CreateNew<MonsterDefinition>() block."""
    starts = list(re.finditer(
        r"var (\w+) = this\.Context\.CreateNew<MonsterDefinition>\(\);", text))
    for i, m in enumerate(starts):
        end = starts[i + 1].start() if i + 1 < len(starts) else len(text)
        yield m.group(1), text[m.end():end]


def grab(seg, var, prop, pattern):
    m = re.search(rf"\b{var}\.{prop} = {pattern};", seg)
    return m.group(1) if m else None


def grab_int(seg, var, prop, default=0):
    v = grab(seg, var, prop, r"(\d+)")
    return int(v) if v is not None else default


def delay_ms(seg, var, prop):
    v = grab(seg, var, prop, r"new TimeSpan\((\d+) \* TimeSpan\.TicksPerMillisecond\)")
    if v is not None:
        return int(v)
    v = grab(seg, var, prop, r"new TimeSpan\((\d+) \* TimeSpan\.TicksPerSecond\)")
    if v is not None:
        return int(v) * 1000
    return 0


def combat_int(raw):
    raw = raw.strip().rstrip("f")
    if not re.fullmatch(r"\d+", raw):
        raise SystemExit(f"non-integer combat value {raw!r}")
    return int(raw)


def resistance_byte(raw):
    """'9f / 255' -> 9 (the stored 0..255 byte, recovered from OpenMU's n/255)."""
    m = re.fullmatch(r"(\d+(?:\.\d+)?)f?\s*/\s*(\d+)", raw.strip())
    if not m:
        raise SystemExit(f"resistance not an n/255 fraction: {raw!r}")
    numerator, denominator = float(m.group(1)), int(m.group(2))
    if denominator != 255:
        raise SystemExit(f"resistance denominator {denominator} != 255")
    return round(numerator / denominator * 255)


def parse_stats(seg):
    """Return (combat dict, resistance-byte dict, water_slot_used)."""
    m = re.search(r"new Dictionary<AttributeDefinition, float>\s*\{(.*?)\};",
                  seg, re.DOTALL)
    combat, resist, water_slot = {}, {}, False
    if not m:
        return combat, resist, water_slot
    for name, raw in re.findall(r"\{\s*Stats\.(\w+),\s*(.+?)\s*\},", m.group(1)):
        if name in COMBAT_MAP:
            combat[COMBAT_MAP[name]] = combat_int(raw)
        elif name in RESIST_MAP:
            resist[RESIST_MAP[name]] = resistance_byte(raw)
            if name == "WaterResistance":
                water_slot = True
        elif name == "WindResistance":
            if resistance_byte(raw) != 0:
                raise SystemExit(f"unexpected non-zero WindResistance {raw!r}")
        elif name == "PoisonDamageMultiplier":
            continue
        else:
            raise SystemExit(f"unmapped monster stat Stats.{name}")
    return combat, resist, water_slot


def build_combat(seg, var):
    combat, resist, water_slot = parse_stats(seg)
    for field in COMBAT_REQUIRED:
        if field not in combat:
            raise SystemExit(f"{var}: fighting record missing combat.{field}")
    if combat["level"] < 1:
        raise SystemExit(f"{var}: level {combat['level']} < 1")
    columns = {
        "level": combat["level"],
        "hp": combat["hp"],
        "min_phys_damage": combat["min_phys_damage"],
        "max_phys_damage": combat["max_phys_damage"],
        "defense": combat.get("defense", 0),
        "attack_rate": combat["attack_rate"],
        "defense_rate": combat["defense_rate"],
    }
    resistances = {el: resist.get(el, 0) for el in ELEMENTS}
    return columns, resistances, water_slot


def build_behavior(seg, var):
    return {
        "move_range": grab_int(seg, var, "MoveRange"),
        "attack_range": grab_int(seg, var, "AttackRange"),
        "view_range": grab_int(seg, var, "ViewRange"),
        "move_delay_ms": delay_ms(seg, var, "MoveDelay"),
        "attack_delay_ms": delay_ms(seg, var, "AttackDelay"),
        "respawn_ms": delay_ms(seg, var, "RespawnDelay"),
    }


def parse_attack(seg, var, skill_numbers):
    """Return (attack dict, phantom_flag)."""
    name = grab(seg, var, "AttackSkill", r".*?SkillNumber\.(\w+)\)")
    if name is None:
        return {"kind": "plain"}, False
    number = skill_numbers[name]
    if number == PHANTOM_SKILL:
        return {"kind": "plain"}, True
    return {"kind": "skill", "skill": number}, False


def parse_object_kind(seg, var):
    kind = grab(seg, var, "ObjectKind", r"NpcObjectKind\.(\w+)")
    return kind if kind is not None else "Monster"


def parse_monster(seg, var, version, skill_numbers):
    number = grab_int(seg, var, "Number", default=None)
    name = grab(seg, var, "Designation", r'"(.*?)"')
    if number is None or not name:
        raise SystemExit(f"block {var} without Number/Designation")

    kind = parse_object_kind(seg, var)
    reviews = []

    if kind in ("Monster", "Guard", "Trap"):
        combat, resistances, water_slot = build_combat(seg, var)
        behavior = build_behavior(seg, var)
        if kind == "Guard":
            if grab(seg, var, "AttackSkill", r".+") is not None:
                raise SystemExit(f"guard {number} carries an AttackSkill")
            role = {"kind": "guard", "combat": combat,
                    "resistances": resistances, "behavior": behavior}
        else:
            attack, phantom = parse_attack(seg, var, skill_numbers)
            if phantom:
                reviews.append(PHANTOM_REVIEW)
            if kind == "Monster":
                role = {"kind": "monster", "combat": combat,
                        "resistances": resistances, "behavior": behavior,
                        "attack": attack}
            else:
                intel = grab(seg, var, "IntelligenceTypeName",
                             r"typeof\((\w+)\)\.FullName")
                if intel not in TRAP_TARGETING:
                    raise SystemExit(f"trap {number} unmapped AI {intel}")
                role = {"kind": "trap", "targeting": TRAP_TARGETING[intel],
                        "combat": combat, "resistances": resistances,
                        "behavior": behavior, "attack": attack}
        if water_slot:
            reviews.append(
                f"lightning byte extracted from OpenMU's water_resistance slot "
                f"({resistances['lightning']}/255); OpenMU models no lightning "
                f"column pre-S3 — verify against classic Monster.txt.")
    elif kind == "PassiveNpc":
        window = grab(seg, var, "NpcWindow", r"NpcWindow\.(\w+)")
        if window is not None and window not in NPC_WINDOW:
            raise SystemExit(f"npc {number} unmapped NpcWindow.{window}")
        role = {"kind": "npc"}
        if window is not None:
            role["window"] = NPC_WINDOW[window]
    elif kind == "SoccerBall":
        role = {"kind": "soccer_ball"}
    else:
        raise SystemExit(f"unmapped NpcObjectKind.{kind}")

    if number in ERA_REVIEW:
        reviews.append(ERA_REVIEW[number])

    record = {"number": number, "name": name, "source_version": version,
              "role": role}
    if reviews:
        record["review"] = " ".join(reviews)
    return record


def extract_monsters(skill_numbers):
    by_number = {}
    for version in ("075", "095d"):
        for rel in MONSTER_FILES[version]:
            text = read(rel)
            for var, seg in blocks(text):
                rec = parse_monster(seg, var, version, skill_numbers)
                prior = by_number.get(rec["number"])
                if prior is None:
                    by_number[rec["number"]] = rec
                    continue
                a = {k: v for k, v in prior.items() if k != "source_version"}
                b = {k: v for k, v in rec.items() if k != "source_version"}
                if a != b:
                    raise SystemExit(
                        f"conflicting definitions for monster {rec['number']}")
    return sorted(by_number.values(), key=lambda r: r["number"])


# --------------------------------------------------------------------- spawns

SPAWN_RE = re.compile(r"this\.CreateMonsterSpawn\(([^;]*?)\)\s*;", re.S)
NPC_REF_RE = re.compile(r"this\.NpcDictionary\[(\d+)\]")
CONST_RE = re.compile(r"const (?:byte|short|int) (\w+) = (\d+);")
# DevilSquare3/4 spawn an S6 monster when present, else a 0.95d-era one:
# `if (TryGetValue(180|294, ...)) { ... } else { ... }`; the S6 monster is
# absent in 0.95d, so the baseline always takes the else branch.
TRY_GET_RE = re.compile(
    r"if \(this\.NpcDictionary\.TryGetValue\((\d+), [^)]*\)\)\s*"
    r"\{(?:[^{}]*)\}\s*else\s*\{([^{}]*)\}", re.S)
WAVE_CONSTS = {
    "DevilSquareInitializer.FirstWaveNumber": 1,
    "DevilSquareInitializer.SecondWaveNumber": 2,
    "DevilSquareInitializer.ThirdWaveNumber": 3,
    "DevilSquareInitializer.BossWaveNumber": 10,
}
WAVE_TRIGGERS = {"AutomaticDuringWave", "OnceAtWaveStart"}


def point(x, y):
    if not (0 <= x <= 255 and 0 <= y <= 255):
        raise SystemExit(f"point out of range ({x},{y})")
    return {"x": x, "y": y}


def rect(x1, y1, x2, y2):
    if not (0 <= x1 <= x2 <= 255 and 0 <= y1 <= y2 <= 255):
        raise SystemExit(f"rect not edge-ordered ({x1},{y1},{x2},{y2})")
    return {"x1": x1, "y1": y1, "x2": x2, "y2": y2}


def parse_spawn_call(call, consts):
    """Return (monster, nums, direction, trigger). Mirrors OpenMU's two
    CreateMonsterSpawn overloads: point (x, y[, dir]) and area
    (x1, x2, y1, y2[, quantity][, dir])."""
    parts = [p.strip() for p in call.split(",")]
    monster = int(NPC_REF_RE.fullmatch(parts[1]).group(1))
    nums, direction, trigger = [], None, "Automatic"
    for token in parts[2:]:
        if token.startswith("Direction."):
            direction = token[len("Direction."):]
        elif token.startswith("SpawnTrigger."):
            trigger = token[len("SpawnTrigger."):]
        elif token in WAVE_CONSTS:
            pass  # wave number: irrelevant, wave rows are dropped
        elif token.isdigit() or token in consts:
            value = int(token) if token.isdigit() else consts[token]
            if trigger != "Automatic" and len(nums) >= 4:
                pass  # positional waveNumber after a wave trigger; dropped
            else:
                nums.append(value)
        else:
            raise SystemExit(f"unresolved spawn token {token!r}")
    return monster, nums, direction, trigger


def placement_of(nums, direction):
    real_dir = direction is not None and direction != "Undefined"
    if real_dir and direction not in DIRECTIONS:
        raise SystemExit(f"unmapped Direction.{direction}")
    if len(nums) == 2:
        x, y = nums
        if real_dir:
            return {"kind": "fixed", "position": point(x, y),
                    "facing": DIRECTIONS[direction]}
        return {"kind": "spot", "position": point(x, y), "quantity": 1}
    if len(nums) in (4, 5):
        x1, x2, y1, y2 = nums[:4]
        quantity = nums[4] if len(nums) == 5 else 1
        degenerate = x1 == x2 and y1 == y2
        if real_dir:
            if not (degenerate and quantity == 1):
                raise SystemExit(f"facing on a non-fixed spawn {nums} {direction}")
            return {"kind": "fixed", "position": point(x1, y1),
                    "facing": DIRECTIONS[direction]}
        if degenerate:
            return {"kind": "spot", "position": point(x1, y1),
                    "quantity": quantity}
        return {"kind": "area", "area": rect(x1, y1, x2, y2),
                "quantity": quantity}
    raise SystemExit(f"odd spawn arity {nums}")


def parse_spawns(rel, version, map_number):
    """Return (spawn records, count of wave rows dropped)."""
    text = read(rel)
    for guarded in TRY_GET_RE.findall(text):
        if guarded[0] not in ("180", "294"):
            raise SystemExit(f"unexpected TryGetValue monster {guarded[0]} in {rel}")
    text = TRY_GET_RE.sub(lambda m: m.group(2), text)
    text = re.sub(r"//[^\n]*", "", text)  # strip comments (incl. a broken Atlans line)
    consts = {name: int(value) for name, value in CONST_RE.findall(text)}
    records, dropped = [], 0
    for call in SPAWN_RE.findall(text):
        monster, nums, direction, trigger = parse_spawn_call(call, consts)
        if trigger in WAVE_TRIGGERS:
            dropped += 1
            continue
        if map_number == 9:
            raise SystemExit(f"non-wave spawn on Devil Square map 9 in {rel}")
        schedule = "wandering" if trigger == "Wandering" else "permanent"
        records.append({
            "map": map_number,
            "monster": monster,
            "placement": placement_of(nums, direction),
            "schedule": {"kind": schedule},
            "source_version": version,
        })
    return records, dropped


def extract_spawns():
    records, dropped = [], 0
    for rel, version, map_number in SPAWN_FILES:
        recs, drop = parse_spawns(rel, version, map_number)
        records.extend(recs)
        dropped += drop
    return records, dropped


# ----------------------------------------------------------------------- main

def by_version(records):
    counts = {"075": 0, "095d": 0, "s6": 0}
    for r in records:
        counts[r["source_version"]] += 1
    return counts


def main():
    skill_numbers = load_skill_numbers()

    monsters = extract_monsters(skill_numbers)
    spawns, dropped_wave = extract_spawns()

    mcounts = by_version(monsters)
    if len(monsters) != 100 or mcounts != {"075": 73, "095d": 27, "s6": 0}:
        raise SystemExit(f"monster count check failed: {len(monsters)} {mcounts}")
    approved_095d = {43, 44, 53, 54, 235, 236, 237}
    present = {r["number"] for r in monsters}
    if approved_095d - present or 150 not in present or 200 not in present:
        raise SystemExit(f"approved monsters missing: {approved_095d - present}")

    scounts = by_version(spawns)
    if len(spawns) != 1847 or dropped_wave != 28:
        raise SystemExit(f"spawn count check failed: {len(spawns)} dropped={dropped_wave}")

    # Referential integrity sanity: every spawned monster is a defined monster.
    dangling = {s["monster"] for s in spawns} - present
    if dangling:
        raise SystemExit(f"spawns reference undefined monsters: {sorted(dangling)}")

    # Display names -> host-owned sidecar keyed by number; the core file carries
    # only the number and rules. Spawns never carried a name.
    write_names("monster_definitions.json", {"records": [
        {"number": r["number"], "name": r["name"]} for r in monsters]})
    m_out = write_datafile("monster_definitions.json",
                           [without_name(r) for r in monsters])
    s_out = write_datafile("spawns.json", spawns)
    print(f"wrote {m_out}: {len(monsters)} monsters {mcounts}")
    print(f"wrote {s_out}: {len(spawns)} spawns {scounts} "
          f"(dropped {dropped_wave} Devil Square wave rows)")

    reviewed = [r for r in monsters if "review" in r]
    from collections import Counter
    role_counts = Counter(r["role"]["kind"] for r in monsters)
    placement_counts = Counter(s["placement"]["kind"] for s in spawns)
    schedule_counts = Counter(s["schedule"]["kind"] for s in spawns)

    coverage("monsters", {
        "category": "monsters",
        "files": {
            "monster_definitions.json": len(monsters),
            "spawns.json": len(spawns),
        },
        "counts_by_source_version": {
            "monster_definitions.json": mcounts,
            "spawns.json": scounts,
        },
        "role_counts": dict(role_counts),
        "placement_counts": dict(placement_counts),
        "schedule_counts": dict(schedule_counts),
        "review": [{"number": r["number"], "name": r["name"], "review": r["review"]}
                   for r in reviewed],
        "review_families": {
            "water_to_lightning_remap": sum(
                1 for r in reviewed
                if "water_resistance slot" in r["review"]),
            "phantom_skill_150": sum(
                1 for r in reviewed if "skill 150" in r["review"]),
            "golden_invasion_era_doubt": sorted(ERA_REVIEW),
        },
        "gaps": [
            "Devil Square wave spawns (28 rows on map 9) omitted from spawns.json "
            "-> W-DS event configs; residue (four square rectangles, per-wave "
            "participant monster numbers, boss) recorded in the monsters_spawns v2 "
            "section, flagged as OpenMU 0.95d tuning to re-source under W-SRC",
            "merchant NPCs here carry only role/window; store contents ship "
            "separately in npc_shops.json (tools/extract/shops.py)",
            "quests startable at NPCs (Sevina #235, window=quest) deferred to "
            "quests.json (follow-up wave)",
            "MP / MagicDefense Monster.txt columns: OpenMU never modeled them, so "
            "no values ship; they enter MonsterCombat when an authentic Monster.txt "
            "source is wired in (W-SRC), never as fabricated zeros",
            "no s6 monster backports this wave: none in the approved source list; "
            "known 1.0-era candidates (Blood Castle mobs, Archangel Messenger NPC, "
            "higher golden-invasion tiers) left out by that decision",
            "the wandering-merchant rotation rule (one active at a time; timing "
            "unsourced) belongs to a future event/NPC wave, not static spawn "
            "data; the two wandering rows (#248, #250 on Lorencia) carry "
            "schedule=wandering only",
        ],
        "notes": [
            "the v1 stat slug-list is gone: combat columns are typed MonsterCombat "
            "(level/hp/min+max_phys_damage/defense/attack_rate/defense_rate) and "
            "MobBehavior (move/attack/view ranges + move/attack/respawn delays_ms) "
            "on the fighting role variants; no f64 survives",
            "resistances are raw 0..255 bytes recovered from OpenMU's n/255 "
            "fractions as round(f*255) (exact; denominator asserted 255); "
            "PerElement carries all 7 keys, earth/wind/water always explicit 0",
            "48 records take their lightning byte from OpenMU's water_resistance "
            "slot (pre-S3 has no water damage type); each carries the per-record "
            "water->lightning review with its byte value",
            "14 records OpenMU points at skill 150 (an S6-only backport) ship as "
            "attack{plain} with a phantom-150 review; no phantom skill number ships "
            "in any file; real attack skills (1,2,3,7,11,17,50) ship as "
            "attack{skill}, Atlas-proven at load",
            "Golden Titan #53 / Golden Soldier #54 keep their golden-invasion "
            "era-doubt review; #53 also carries phantom-150 and water->lightning "
            "reasons, joined into one review string (space-separated)",
            "role split is total: 75 monster + 4 trap + 2 guard fighting records "
            "carry combat/resistances/behavior; 18 npc + 1 soccer_ball carry none "
            "(no fabricated zero columns); guards carry no attack field (plain by "
            "rule, D9); traps carry targeting (single_when_pressed/area_when_pressed/"
            "directional)",
            "durations are integer ms (RespawnDelay is seconds in source, x1000); "
            "traps #100-102 omit MoveDelay -> move_delay_ms 0; traps omit "
            "DefenseBase -> defense 0 (fixed Monster.txt zeros, not fabrication)",
            "Bali #150 defined identically in the 075 and 095d SkillsInitializers; "
            "deduped to source_version 075",
            "dropped with their mechanism: NumberOfMaximumItemDrops (drops-service "
            "const), monster-bound drop groups (drops domain, keyed by number), "
            "poison_damage_multiplier (skills_effects poison-tick rule), the opaque "
            "Attribute byte, MapRef.discriminator on spawns",
            "spawn placement is kind-tagged: 244 fixed (facing-bearing stationary "
            "objects, quantity 1), spot (single-tile mobile spawns, incl. the two "
            "Devias #20 quantity-10 rows), area (rectangle spawns); the "
            "degenerate-rect / point-as-rect convention is gone",
            "move/attack/respawn delays are treated as authentic Monster.txt "
            "content per the v2 section (D11), not OpenMU-invented tuning; they "
            "carry no review",
        ],
    })


if __name__ == "__main__":
    main()
