#!/usr/bin/env python3
"""Extract monster/NPC definitions (spec section 9) from OpenMU initializers.

Outputs:
  data/monster_definitions.json   spec section 9 records
  data/_coverage/monsters.json    counts, review list, named gaps

Sources (approved): Version075 map files' CreateMonsters + NpcInitialization
+ the Bali summon (#150) in SkillsInitializer; Version095d map additions
(Tarkan, Icarus, DevilSquare3/4), invasion mobs (#43/#44/#53/#54) and NPCs
(#235-237). Merchant STORE CONTENTS are deferred (follow-up wave).

Conversions: delays -> integer ms (RespawnDelay seconds -> ms); resistances
kept as fractions (source writes them as `n`f / 255, i.e. divisor 255).
OpenMU's opaque `Attribute` byte is dropped; `role` discriminates instead.
Monster-bound drop groups reference the canonical ids the drops extractor
emits into drop_groups.json (DROP_GROUP_IDS maps source Description -> id).
"""

import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import DATA_DIR, coverage, load_stat_map, write_datafile

OPENMU = "/tmp/openmu-ref/src/Persistence/Initialization"
SKILL_NUMBER_CS = os.path.join(OPENMU, "Skills/SkillNumber.cs")

V075 = "Version075"
V095D = "Version095d"

SOURCE_FILES = {
    "075": [
        f"{V075}/Maps/Lorencia.cs",
        f"{V075}/Maps/Dungeon.cs",
        f"{V075}/Maps/Devias.cs",
        f"{V075}/Maps/Noria.cs",
        f"{V075}/Maps/LostTower.cs",
        f"{V075}/Maps/Exile.cs",
        f"{V075}/Maps/Arena.cs",
        f"{V075}/Maps/Atlans.cs",
        f"{V075}/NpcInitialization.cs",
        f"{V075}/SkillsInitializer.cs",  # summon monster Bali #150
    ],
    "095d": [
        f"{V095D}/Maps/Tarkan.cs",
        f"{V095D}/Maps/Icarus.cs",
        f"{V095D}/Maps/DevilSquare1.cs",
        f"{V095D}/Maps/DevilSquare2.cs",
        f"{V095D}/Maps/DevilSquare3.cs",
        f"{V095D}/Maps/DevilSquare4.cs",
        f"{V095D}/NpcInitialization.cs",
        f"{V095D}/InvasionMobsInitialization.cs",
        f"{V095D}/SkillsInitializer.cs",  # Bali again, identical -> deduped to 075
    ],
}

NPC_WINDOW = {
    "Merchant": "merchant",
    "Merchant1": "merchant",
    "Storage": "storage",
    "VaultStorage": "vault",
    "ChaosMachine": "chaos_machine",
    "GuildMaster": "guild_master",
    "DevilSquare": "devil_square",
    "LegacyQuest": "legacy_quest",
}

TRAP_AI = {
    "AttackSingleWhenPressedTrapIntelligence": "attack_single_pressed",
    "AttackAreaWhenPressedTrapIntelligence": "attack_area_pressed",
    "RandomAttackInRangeTrapIntelligence": "random_in_range",
    "AttackAreaTargetInDirectionTrapIntelligence": "area_target_in_direction",
}

# Records whose era inside the 095d dataset is doubtful -> review flag.
REVIEW = {
    53: "Golden Titan ships in the upstream 095d dataset, but this golden-invasion tier is commonly dated ~0.97+; kept as 095d per dataset policy.",
    54: "Golden Soldier ships in the upstream 095d dataset, but this golden-invasion tier is commonly dated ~0.97+; kept as 095d per dataset policy.",
}


def load_skill_numbers():
    """SkillNumber enum member -> number."""
    numbers = {}
    with open(SKILL_NUMBER_CS, encoding="utf-8") as f:
        for line in f:
            m = re.match(r"\s*(\w+) = (\d+),", line)
            if m:
                numbers[m.group(1)] = int(m.group(2))
    if not numbers:
        raise SystemExit(f"no skill numbers parsed from {SKILL_NUMBER_CS}")
    return numbers


def load_stat_ids():
    with open(os.path.join(DATA_DIR, "stats.json"), encoding="utf-8") as f:
        return {r["id"] for r in json.load(f)["records"]}


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


def stat_value(raw):
    """'6' -> 6.0; '0.03f' -> 0.03; '6f / 255' -> 6/255 (resistance fraction)."""
    raw = raw.strip()
    m = re.fullmatch(r"(\d+(?:\.\d+)?)f? / 255", raw)
    if m:
        return float(m.group(1)) / 255.0
    return float(raw.rstrip("f"))


def parse_stats(seg, stat_map, stat_ids):
    m = re.search(r"new Dictionary<AttributeDefinition, float>\s*\{(.*?)\};",
                  seg, re.DOTALL)
    if not m:
        return []
    stats = []
    for name, raw in re.findall(r"\{\s*Stats\.(\w+),\s*(.+?)\s*\},", m.group(1)):
        slug = stat_map.get(name)
        if slug is None or slug not in stat_ids:
            raise SystemExit(f"unmapped stat Stats.{name} (slug {slug!r})")
        stats.append({"stat": slug, "value": stat_value(raw)})
    return stats


def parse_role(seg, var):
    kind = grab(seg, var, "ObjectKind", r"NpcObjectKind\.(\w+)")
    intel = grab(seg, var, "IntelligenceTypeName", r"typeof\((\w+)\)\.FullName")
    window = grab(seg, var, "NpcWindow", r"NpcWindow\.(\w+)")
    if kind is None or kind == "Monster":
        if intel is not None:
            raise SystemExit(f"monster with unexpected AI {intel}")
        return {"kind": "monster"}
    if kind == "Guard":
        if intel != "GuardIntelligence":
            raise SystemExit(f"guard with unexpected AI {intel}")
        return {"kind": "guard"}
    if kind == "Trap":
        if intel not in TRAP_AI:
            raise SystemExit(f"trap with unexpected AI {intel}")
        return {"kind": "trap", "ai": TRAP_AI[intel]}
    if kind == "PassiveNpc":
        if window is not None and window not in NPC_WINDOW:
            raise SystemExit(f"unmapped NpcWindow.{window}")
        return {"kind": "npc", "window": NPC_WINDOW[window] if window else None}
    if kind == "SoccerBall":
        return {"kind": "soccer_ball"}
    raise SystemExit(f"unmapped NpcObjectKind.{kind}")


# The drops extractor owns drop_groups.json and names monster-bound groups
# with monster-scoped ids (raw Description slugs like "box_of_luck" would be
# ambiguous). Map source Description text to those canonical ids; unmapped
# descriptions fail loud so new groups get a deliberate id, never a guess.
DROP_GROUP_IDS = {
    "Box of Luck": "golden_budge_dragon_box_of_luck",       # monster 43
    "Items from red dragon": "red_dragon_jewels",           # monster 44
    "Box of Kundun +2": "golden_titan_box_of_kundun_2",     # monster 53
}


def parse_drop_groups(seg, var):
    """Monster-bound drop groups -> canonical drop_groups.json ids.

    Covers literal `itemDrop.Description = "..."` assignments and the
    AddBoxOfKundunToMonster(lvl, ...) helper (description "Box of Kundun +lvl").
    """
    descriptions = re.findall(r'itemDrop\.Description = "(.*?)";', seg)
    for lvl in re.findall(rf"this\.AddBoxOfKundunToMonster\((\d+), {var}\);", seg):
        descriptions.append(f"Box of Kundun +{lvl}")
    groups = []
    for desc in descriptions:
        if desc not in DROP_GROUP_IDS:
            raise SystemExit(f"drop group description {desc!r} has no canonical id")
        groups.append(DROP_GROUP_IDS[desc])
    return groups


def parse_file(path, version, stat_map, stat_ids, skill_numbers):
    with open(path, encoding="utf-8") as f:
        text = f.read()
    records = []
    for var, seg in blocks(text):
        skill_name = grab(seg, var, "AttackSkill",
                          r".*?SkillNumber\.(\w+)\)")
        record = {
            "number": grab_int(seg, var, "Number", default=None),
            "name": grab(seg, var, "Designation", r'"(.*?)"'),
            "source_version": version,
            "role": parse_role(seg, var),
            "move_range": grab_int(seg, var, "MoveRange"),
            "attack_range": grab_int(seg, var, "AttackRange"),
            "view_range": grab_int(seg, var, "ViewRange"),
            "move_delay_ms": delay_ms(seg, var, "MoveDelay"),
            "attack_delay_ms": delay_ms(seg, var, "AttackDelay"),
            "respawn_ms": delay_ms(seg, var, "RespawnDelay"),
            "max_item_drops": grab_int(seg, var, "NumberOfMaximumItemDrops"),
            "attack_skill": skill_numbers[skill_name] if skill_name else None,
            "stats": parse_stats(seg, stat_map, stat_ids),
            "drop_groups": parse_drop_groups(seg, var),
        }
        if record["number"] is None or not record["name"]:
            raise SystemExit(f"block without Number/Designation in {path}")
        if record["number"] in REVIEW:
            record["review"] = REVIEW[record["number"]]
        records.append(record)
    return records


def main():
    stat_map = load_stat_map()
    stat_ids = load_stat_ids()
    skill_numbers = load_skill_numbers()

    by_number = {}
    for version in ("075", "095d"):  # oldest first: 075 wins ties
        for rel in SOURCE_FILES[version]:
            path = os.path.join(OPENMU, rel)
            for rec in parse_file(path, version, stat_map, stat_ids, skill_numbers):
                prior = by_number.get(rec["number"])
                if prior is None:
                    by_number[rec["number"]] = rec
                    continue
                # Same number in both datasets: identical (modulo version tag)
                # -> keep the older; anything else must be decided, not guessed.
                a = {k: v for k, v in prior.items() if k != "source_version"}
                b = {k: v for k, v in rec.items() if k != "source_version"}
                if a != b:
                    raise SystemExit(
                        f"conflicting definitions for monster {rec['number']}")

    records = sorted(by_number.values(), key=lambda r: r["number"])

    counts = {"075": 0, "095d": 0, "s6": 0}
    for r in records:
        counts[r["source_version"]] += 1
    expected = {"075": 73, "095d": 27, "s6": 0}
    if counts != expected:
        raise SystemExit(f"count check failed: {counts} != {expected}")
    approved_095d = {43, 44, 53, 54, 235, 236, 237}
    missing = approved_095d - set(by_number)
    if missing or 150 not in by_number or 200 not in by_number:
        raise SystemExit(f"approved records missing: {missing}")

    out = write_datafile("monster_definitions.json", records)
    print(f"wrote {out}: {len(records)} records {counts}")

    coverage("monsters", {
        "category": "monsters",
        "file": "data/monster_definitions.json",
        "counts": {**counts, "total": len(records)},
        "review": [{"number": r["number"], "name": r["name"], "review": r["review"]}
                   for r in records if "review" in r],
        "gaps": [
            "merchant store contents deferred by decision (merchant_stores.json, follow-up wave); merchant NPCs here carry only role/window",
            "quests startable at NPCs (Sevina #235) deferred to quests.json (follow-up wave)",
            "no s6 monster backports this wave: none named in the approved source list; known 1.0-era candidates left out by that decision include Blood Castle monsters + Archangel Messenger NPC and higher golden-invasion tiers beyond what the upstream 095d dataset ships",
            "Golden Soldier #54 has no drop group in source (unlike #43 box_of_luck and #53 box_of_kundun_2) - faithful to the upstream 095d data, likely upstream omission of Box of Kundun +1",
        ],
        "notes": [
            "resistances are fractions: source stores 0-255 bytes, upstream initializers write n/255; values here are n/255 with divisor 255",
            "durations converted to integer ms; RespawnDelay is seconds in source (x1000); passive NPCs without delays get 0",
            "the upstream opaque monster 'attribute' byte (1=npc/guard/trap, 2=monster) dropped per spec; role kind discriminates",
            "the upstream free-form guard AI name folded into role kind 'guard'; trap AIs kind-tagged; trap AI random_in_range unused pre-S3",
            "monster-bound drop groups reference the drops extractor's canonical ids: golden_budge_dragon_box_of_luck (#43), red_dragon_jewels (#44), golden_titan_box_of_kundun_2 (#53); source Description text is mapped, not slugified",
            "attack_skill is the numeric skill number (Poison=1, Meteorite=2, Lightning=3, Ice=7, PowerWave=11, EnergyBall=17, FlameofEvil=50, MonsterSkill=150); skill 150 is defined only by the S6 skills initializer and ships in skills.json as an s6 backport",
            "Bali #150 defined identically in 075 and 095d SkillsInitializers; deduped to source_version 075",
        ],
    })


if __name__ == "__main__":
    main()
