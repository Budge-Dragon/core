#!/usr/bin/env python3
"""Extract item_options.json + item_sets.json (spec sections 5 and 6).

Sources (OpenMU clone at /tmp/openmu-ref):
- GameConfigurationInitializerBase.cs: luck + defense/physical/wizardry/defense-rate
  normal options (075).
- Version075/Items/Jewelery.cs: jewelry health-recover option (075, max option level 3).
- Version075|095d/Items/Wings.cs: per-wing options (identical in both -> 075).
- VersionSeasonSix/Items/Wings.cs + WingsInitializerBase.cs: 2nd-wing 'wing'-type
  options and per-wing jewel-of-life option groups for wings 12/3-6 (s6,
  backported with the 2nd-wing items).
- Items/ExcellentOptions.cs: excellent defense/physical/wizardry (attached from 095d on).
- Version095d/Items/Pets.cs: Dinorant options (095d).
- VersionSeasonSix/Items/AncientSets.cs: 36 ancient sets + per-piece ancient bonus (s6).
- Items/ArmorInitializerBase.cs BuildSets(): generic armor sets. Called by BOTH
  Version075/Items/Armors.cs:137 and Version095d/Items/Armors.cs:137 -> the fact-file
  claim that 075 only has a TODO is stale; generic sets are 075 data.

Known source data bug fixed here: Kantata set option 6 has
(Stats.ExcellentDamageChance, 10.0f) where every comparable option uses 0.10 or 0.15.
Emitted as 0.10 with a review note.
"""

import json
import os
import re

import common

INIT = "/tmp/openmu-ref/src/Persistence/Initialization"
STAT = common.load_stat_map()

ITEM_GROUPS = {
    "Swords": 0, "Axes": 1, "Scepters": 2, "Spears": 3, "Bows": 4, "Staff": 5,
    "Shields": 6, "Helm": 7, "Armor": 8, "Pants": 9, "Gloves": 10, "Boots": 11,
    "Orbs": 12, "Misc1": 13, "Misc2": 14, "Scrolls": 15,
}
AGGREGATE = {
    "AddRaw": "add_raw", "Multiplicate": "multiplicate",
    "AddFinal": "add_final", "Maximum": "maximum",
}


def read(rel):
    with open(os.path.join(INIT, rel), encoding="utf-8-sig") as f:
        return f.read()


def stat(openmu_name):
    return STAT[openmu_name]  # KeyError = unmapped stat, fail loudly


def power_up(stat_name, value, aggregate, scaled_by=None):
    p = {"stat": stat(stat_name), "value": value, "aggregate": aggregate}
    if scaled_by:
        p["scaled_by"] = scaled_by
    return p


def fixed(number, stat_name, value, aggregate, scaled_by=None):
    return {"number": number, "kind": "fixed",
            "power_up": power_up(stat_name, value, aggregate, scaled_by)}


def per_level(number, stat_name, values_by_level, aggregate="add_raw",
              level_type="option_level", first_level=1):
    return {
        "number": number, "kind": "per_level", "level_type": level_type,
        "stat": stat(stat_name), "aggregate": aggregate,
        "levels": [{"level": first_level + i, "value": v}
                   for i, v in enumerate(values_by_level)],
    }


def option_record(oid, option_type, source_version, options, adds_randomly=True,
                  add_chance=None, max_per_item=1, review=None):
    rec = {"id": oid, "option_type": option_type, "source_version": source_version}
    if review:
        rec["review"] = review
    rec["adds_randomly"] = adds_randomly
    # add_chance is required by the schema; 0.0 for non-random options
    # (matches OpenMU AddChance = 0 on those definitions).
    rec["add_chance"] = add_chance if add_chance is not None else 0.0
    rec["max_per_item"] = max_per_item
    rec["options"] = options
    return rec


# ---------------------------------------------------------------------------
# item_options.json
# ---------------------------------------------------------------------------

def excellent_options(oid, dmg_kind):
    """Shared shape from Items/ExcellentOptions.cs; dmg_kind picks the attack group."""
    if dmg_kind == "defense":
        opts = [
            fixed(1, "MoneyAmountRate", 1.4, "multiplicate"),
            fixed(2, "DefenseRatePvm", 1.1, "multiplicate"),
            fixed(3, "DamageReflection", 0.05, "add_raw"),
            fixed(4, "ArmorDamageDecrease", 0.04, "add_raw"),
            fixed(5, "MaximumMana", 1.04, "multiplicate"),
            fixed(6, "MaximumHealth", 1.04, "multiplicate"),
        ]
    else:
        inc = "PhysicalBaseDmgIncrease" if dmg_kind == "physical" else "WizardryBaseDmgIncrease"
        dmg = "PhysicalBaseDmg" if dmg_kind == "physical" else "WizardryBaseDmg"
        opts = [
            fixed(1, "ManaAfterMonsterKillMultiplier", 0.125, "add_raw"),   # 1/8
            fixed(2, "HealthAfterMonsterKillMultiplier", 0.125, "add_raw"),  # 1/8
            fixed(3, "AttackSpeedAny", 7.0, "add_raw"),
            fixed(4, inc, 1.02, "multiplicate"),
            fixed(5, dmg, 0.0, "add_raw",
                  scaled_by=[{"stat": stat("TotalLevel"), "operator": "multiply",
                              "operand": 0.05}]),                            # level/20
            fixed(6, "ExcellentDamageChance", 0.1, "add_raw"),
        ]
    return option_record(oid, "excellent", "095d", opts, add_chance=0.001, max_per_item=2)


def build_item_options(ancient_bonus_stats):
    records = []

    # GameConfigurationInitializerBase (075): luck + the four normal options.
    records.append(option_record(
        "luck", "luck", "075",
        [fixed(0, "CriticalDamageChance", 0.05, "add_raw")], add_chance=0.25))
    for oid, stat_name, base in (
            ("defense", "DefenseBase", 4.0),
            ("physical_attack", "PhysicalBaseDmg", 4.0),
            ("wizardry_attack", "WizardryBaseDmg", 4.0),
            ("defense_rate", "DefenseRatePvm", 5.0)):
        records.append(option_record(
            oid, "option", "075",
            [per_level(0, stat_name, [base * lvl for lvl in range(1, 5)])],
            add_chance=0.25))

    # Version075/Items/Jewelery.cs (075): CreateOption("Health recover for
    # jewelery", ...), max option level 3. Id = slugified source designation,
    # which is the slug item_definitions.json references.
    records.append(option_record(
        common.slugify("Health recover for jewelery"), "option", "075",
        [per_level(0, "HealthRecoveryMultiplier", [0.01, 0.02, 0.03])],
        add_chance=0.25))

    # VersionSeasonSix/Items/Jewelery.cs (s6, backported with the 1.0-era
    # jewelry items 13/24 + 13/28, ancient-set pieces): "Jewelery option
    # <designation>" = +1% maximum mana/ability per option level
    # (multiplicate, base 1.0 + 0.01/level, max option level 3).
    for designation, stat_name in (("Maximum Mana", "MaximumMana"),
                                   ("Maximum Ability", "MaximumAbility")):
        records.append(option_record(
            common.slugify("Jewelery option " + designation), "option", "s6",
            [per_level(0, stat_name, [1.01, 1.02, 1.03],
                       aggregate="multiplicate")],
            add_chance=0.25,
            review="S6-only jewelry option backported with the 1.0-era "
                   "ring_of_magic/pendant_of_ability items (ancient-set "
                   "pieces)"))

    # Wings initializers (identical 075/095d -> 075): one option group per wing.
    for oid, stat_name, values in (
            ("wings_of_elf_options", "HealthRecoveryMultiplier", [0.01, 0.02, 0.03, 0.04]),
            ("wings_of_heaven_options", "WizardryBaseDmg", [4.0, 8.0, 12.0, 16.0]),
            ("wings_of_satan_options", "PhysicalBaseDmg", [4.0, 8.0, 12.0, 16.0])):
        records.append(option_record(
            oid, "option", "075", [per_level(0, stat_name, values)], add_chance=0.25))

    # VersionSeasonSix/Items/Wings.cs (s6, backported with the 2nd-wing items):
    # CreateSecondClassWingOptions -> shared "2nd Wing Options" ('wing' type,
    # add chance 0.1). HP/mana entries are item-leveled (base 50 +5/level);
    # source runs to max item level 15, truncated at the approved cap 11.
    hp_mana = [50.0 + 5 * level for level in range(12)]
    records.append(option_record(
        "2nd_wing_options", "wing", "s6",
        [per_level(1, "MaximumHealth", hp_mana,
                   level_type="item_level", first_level=0),
         per_level(2, "MaximumMana", hp_mana,
                   level_type="item_level", first_level=0),
         fixed(3, "DefenseIgnoreChance", 0.03, "add_raw")],
        add_chance=0.1,
        review="1.0-era 2nd-wing options backported with the s6 wing items; "
               "hp/mana item-level values truncated from source cap 15 to 11"))

    # Per-wing jewel-of-life groups for the backported 2nd wings (BuildOptions:
    # recover 0.01/level, phys/wiz dmg 4/level; option numbers are the source
    # bit codes 0b00/0b10 per wing).
    recover = ("HealthRecoveryMultiplier", [0.01, 0.02, 0.03, 0.04])
    phys = ("PhysicalBaseDmg", [4.0, 8.0, 12.0, 16.0])
    wiz = ("WizardryBaseDmg", [4.0, 8.0, 12.0, 16.0])
    for oid, entries in (
            ("wings_of_spirits_options", ((2,) + recover, (0,) + phys)),
            ("wings_of_soul_options", ((0,) + recover, (2,) + wiz)),
            ("wings_of_dragon_options", ((0,) + recover, (2,) + phys)),
            ("wings_of_darkness_options", ((0,) + wiz, (2,) + phys))):
        records.append(option_record(
            oid, "option", "s6",
            [per_level(number, stat_name, values)
             for number, stat_name, values in entries],
            add_chance=0.25,
            review="1.0-era jewel-of-life options of a backported s6 2nd wing; "
                   "same value shape as the 075 first-wing options"))

    # Items/ExcellentOptions.cs, attached from 095d on (Excellent type absent in 075).
    records.append(excellent_options("excellent_defense", "defense"))
    records.append(excellent_options("excellent_physical", "physical"))
    records.append(excellent_options("excellent_wizardry", "wizardry"))

    # Version095d/Items/Pets.cs (095d): Dinorant. All three options carry number 4.
    records.append(option_record(
        "dinorant_options", "option", "095d",
        [fixed(4, "DamageReceiveDecrement", 0.95, "multiplicate"),
         fixed(4, "MaximumAbility", 50.0, "add_final"),
         fixed(4, "AttackSpeedAny", 5.0, "add_final")],
        add_chance=0.3))

    # AncientSets.cs (s6): per-piece bonus, one definition per stat, +5/+10.
    for stat_name in ancient_bonus_stats:
        records.append(option_record(
            "ancient_bonus_" + stat(stat_name), "ancient_bonus", "s6",
            [per_level(0, stat_name, [5.0, 10.0])],
            adds_randomly=False,
            review="s1-era ancient piece bonus (+5/+10, level rolled at drop); "
                   "OpenMU ships it only in the s6 dataset"))
    return records


# ---------------------------------------------------------------------------
# item_sets.json — generic armor sets (075, BuildSets)
# ---------------------------------------------------------------------------

def parse_armor_families():
    """(family_name, [(group, number) x5]) per armor number, from 095d Armors.cs
    (item lines identical to 075; verified by identical Create* calls)."""
    text = read("Version095d/Items/Armors.cs")
    pieces = {}   # number -> [(group, number)]
    names = {}    # number -> family name (first word of helm name, per BuildSets)
    for m in re.finditer(r'this\.CreateArmor\((\d+), (\d+), \d+, \d+, "([^"]+)"', text):
        number, slot, name = int(m.group(1)), int(m.group(2)), m.group(3)
        group = slot + 5  # ArmorInitializerBase.cs:371
        pieces.setdefault(number, []).append((group, number))
        if group == 7:
            names[number] = name.split(" ")[0]
    for m in re.finditer(r'this\.CreateGloves\((\d+), "([^"]+)"', text):
        pieces.setdefault(int(m.group(1)), []).append((10, int(m.group(1))))
    for m in re.finditer(r'this\.CreateBoots\((\d+), 6, \d+, \d+, "([^"]+)"', text):
        pieces.setdefault(int(m.group(1)), []).append((11, int(m.group(1))))
    return [(names[n], sorted(pieces[n])) for n in sorted(pieces)]


def generic_set(sid, pieces, set_level, always_applies, set_options):
    # number 0: generic armor sets have no client-facing set number in the
    # source (1..36 are the ancient sets); 0 = generic, like discriminator 0.
    return {
        "id": sid, "number": 0, "source_version": "075",
        "min_item_count": len(pieces), "count_distinct": False,
        "set_level": set_level, "always_applies": always_applies,
        "pieces": [{"item": common.item_ref(g, n), "discriminator": 0,
                    "bonus_stat": None} for g, n in pieces],
        "set_options": set_options,
    }


def build_generic_sets():
    records = []
    for family, pieces in parse_armor_families():
        slug = common.slugify(family)
        # "<Family> Defense Rate Bonus": full set at any level, DefenseRatePvm x1.1
        records.append(generic_set(
            slug + "_defense_rate_bonus", pieces, 0, True,
            [{"number": 0,
              "power_up": power_up("DefenseRatePvm", 1.1, "multiplicate")}]))
        # "<Family> Defense Bonus (Level N)": setLevel 10..MaximumArmorLevel (11),
        # DefenseBase x (1 + (setLevel - 9) * 0.05)
        for set_level in (10, 11):
            records.append(generic_set(
                "%s_defense_bonus_level_%d" % (slug, set_level), pieces, set_level, True,
                [{"number": 0,
                  "power_up": power_up("DefenseBase", 1 + (set_level - 9) * 0.05,
                                       "multiplicate")}]))
    return records


# ---------------------------------------------------------------------------
# item_sets.json — ancient sets (s6, curated backport)
# ---------------------------------------------------------------------------

def parse_ancient_sets():
    text = read("VersionSeasonSix/Items/AncientSets.cs")
    sets = []
    for m in re.finditer(
            r'var (\w+) = this\.AddAncientSet\(\s*"([^"]+)",\s*//\s*([^\n]+?)\s*\n'
            r'\s*(\d+),\n(.*?)\);', text, re.S):
        var, name, family, number, opts_text = m.groups()
        options = []
        for o in re.finditer(
                r'\(Stats\.(\w+), ([0-9.]+)f?(?:, AggregateType\.(\w+))?\)', opts_text):
            options.append((o.group(1), float(o.group(2)),
                            AGGREGATE[o.group(3) or "AddRaw"]))
        # capture through the closing "));" plus any trailing "// item name" comment
        items_m = re.search(r'this\.AddItems\(\s*%s,\n(.*?\)\);[^\n]*)' % var, text, re.S)
        pieces = []
        for line in items_m.group(1).splitlines():
            t = re.search(r'\((\d+), ItemGroups\.(\w+), (?:Stats\.(\w+)|null), (\d+)\)',
                          line)
            if not t:
                continue
            comment = re.search(r'//\s*(.+)', line)
            pieces.append({
                "group": ITEM_GROUPS[t.group(2)], "number": int(t.group(1)),
                "bonus": t.group(3), "discriminator": int(t.group(4)),
                "comment": comment.group(1).strip() if comment else None,
            })
        sets.append({"name": name, "family": family, "number": int(number),
                     "options": options, "pieces": pieces})
    assert len(sets) == 36, "expected 36 ancient sets, parsed %d" % len(sets)
    return sets


def baseline_item_set():
    """(group, number) pairs of our baseline items, to flag ancient pieces that
    reference post-095d items. Prefers the extracted item_definitions.json when
    present; falls back to parsing the 095d initializers."""
    path = os.path.join(common.DATA_DIR, "item_definitions.json")
    if os.path.exists(path):
        with open(path, encoding="utf-8") as f:
            return ({(r["id"]["group"], r["id"]["number"])
                     for r in json.load(f)["records"]}, "item_definitions.json")
    items = set()
    for m in re.finditer(r'this\.CreateWeapon\((\d+), (\d+),',
                         read("Version095d/Items/Weapons.cs")):
        items.add((int(m.group(1)), int(m.group(2))))
    armors = read("Version095d/Items/Armors.cs")
    for m in re.finditer(r'this\.CreateShield\((\d+),', armors):
        items.add((6, int(m.group(1))))
    for m in re.finditer(r'this\.CreateArmor\((\d+), (\d+),', armors):
        items.add((int(m.group(2)) + 5, int(m.group(1))))
    for m in re.finditer(r'this\.CreateGloves\((\d+),', armors):
        items.add((10, int(m.group(1))))
    for m in re.finditer(r'this\.CreateBoots\((\d+),', armors):
        items.add((11, int(m.group(1))))
    # Jewelery.cs: pets 0-3, rings 8/9, transformation ring 10, pendants 12/13; wings 0-2.
    items |= {(13, n) for n in (0, 1, 2, 3, 8, 9, 10, 12, 13)}
    items |= {(12, n) for n in (0, 1, 2)}
    return items, "parsed 095d initializers"


def build_ancient_sets(kantata_notes):
    baseline, baseline_source = baseline_item_set()
    records = []
    bonus_stats = []
    for s in parse_ancient_sets():
        review = "s1-era ancient set backported from the s6 dataset"
        missing = [p for p in s["pieces"]
                   if (p["group"], p["number"]) not in baseline]
        if missing:
            review += "; pieces not in baseline items: " + ", ".join(
                "%s (%d,%d)" % (p["comment"] or s["family"] + " piece",
                                p["group"], p["number"]) for p in missing)
        set_options = []
        for i, (stat_name, value, aggregate) in enumerate(s["options"], start=1):
            if s["name"] == "Kantata" and stat_name == "ExcellentDamageChance" \
                    and value == 10.0:
                value = 0.10
                review += "; fixed OpenMU data bug: excellent_damage_chance 10.0 -> 0.10"
                kantata_notes.append("kantata_plate option %d" % i)
            set_options.append({"number": i,
                                "power_up": power_up(stat_name, value, aggregate)})
        for p in s["pieces"]:
            if p["bonus"] and p["bonus"] not in bonus_stats:
                bonus_stats.append(p["bonus"])
        records.append({
            "id": common.slugify(s["name"] + " " + s["family"]),
            "number": s["number"], "source_version": "s6", "review": review,
            "min_item_count": 2, "count_distinct": True,
            "set_level": 0, "always_applies": False,
            "pieces": [{"item": common.item_ref(p["group"], p["number"]),
                        "discriminator": p["discriminator"],
                        "bonus_stat": stat(p["bonus"]) if p["bonus"] else None}
                       for p in s["pieces"]],
            "set_options": set_options,
        })
    return records, bonus_stats, baseline_source


# ---------------------------------------------------------------------------
# verification + main
# ---------------------------------------------------------------------------

def verify(path, known_stats):
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    assert data["schema_version"] == 1 and isinstance(data["records"], list)
    def walk(node):
        if isinstance(node, dict):
            if "stat" in node:
                assert node["stat"] in known_stats, "unknown stat " + node["stat"]
            if "bonus_stat" in node and node["bonus_stat"] is not None:
                assert node["bonus_stat"] in known_stats
            for v in node.values():
                walk(v)
        elif isinstance(node, list):
            for v in node:
                walk(v)
    for rec in data["records"]:
        assert rec["source_version"] in ("075", "095d", "s6"), rec
        assert rec["source_version"] != "s6" or "review" in rec, rec["id"]
        for opt in rec.get("options", []):
            assert opt["kind"] in ("fixed", "per_level"), opt
        walk(rec)
    return data["records"]


def main():
    kantata_notes = []
    ancient_sets, bonus_stats, baseline_source = build_ancient_sets(kantata_notes)
    options = build_item_options(bonus_stats)
    sets = build_generic_sets() + ancient_sets

    opt_path = common.write_datafile("item_options.json", options)
    set_path = common.write_datafile("item_sets.json", sets)

    with open(os.path.join(common.DATA_DIR, "stats.json"), encoding="utf-8") as f:
        known_stats = {r["id"] for r in json.load(f)["records"]}
    known_stats.add("total_level")  # referenced by excellent option 5
    options = verify(opt_path, known_stats)
    sets = verify(set_path, known_stats)

    def by_version(records):
        counts = {}
        for r in records:
            counts[r["source_version"]] = counts.get(r["source_version"], 0) + 1
        return counts

    info = {
        "files": {
            "item_options.json": {"records": len(options),
                                  "by_source_version": by_version(options),
                                  "ids": [r["id"] for r in options]},
            "item_sets.json": {"records": len(sets),
                               "by_source_version": by_version(sets)},
        },
        "review_flagged": sum("review" in r for r in options + sets),
        "notes": [
            "BuildSets contradiction resolved: Version075/Items/Armors.cs:137 AND "
            "Version095d/Items/Armors.cs:137 both call BuildSets(); generic armor sets "
            "(15 families x [defense-rate + set-level 10/11 defense]) are 075 data. "
            "The fact-file claim that 075 has only a TODO is stale.",
            "Kantata data bug fixed (10.0 -> 0.10 excellent_damage_chance): "
            + "; ".join(kantata_notes),
            "ancient piece existence checked against: " + baseline_source,
            "normal/wing/dinorant option entries keep source option numbers "
            "(0 for first wings, 4 for dinorant, bit codes 0/2 for 2nd-wing "
            "jewel-of-life groups); excellent, ancient and shared 2nd-wing "
            "options are numbered 1..n.",
            "generic armor sets carry set number 0 (source has no client-facing "
            "number for them; 1..36 are the ancient sets); non-random option "
            "definitions carry add_chance 0.0, matching source AddChance = 0.",
        ],
        "gaps": [
            "wing option groups for excluded wing items stay unextracted: Wings of "
            "Curse 12/41 + Wings of Despair 12/42 (summoner), Cape of Fighter 12/49 "
            "(rage fighter), Cape of Lord 13/30 options incl. leadership entry "
            "(dark-lord item wave), all 3rd-wing options (post-S3)",
            "S6-only normal options phys+wiz combined (0x07, MG magic swords) and curse "
            "attack (0x05, Summoner): excluded, no pre-S3 items reference them",
            "excellent curse options: disabled in OpenMU until S14, excluded per spec",
            "harmony/guardian/socket option groups: post-S3, excluded per spec",
            "Fenrir/Dark Horse option types + combination bonuses: S6 dataset only, "
            "excluded (Dinorant options extracted from 095d)",
            "ancient-set pieces that were post-095d items (storm crow/adamantine/"
            "red wing armor 15/26/40, jewelry 13/21-28) are backported into "
            "item_definitions.json (s6, items.py) so the piece references resolve; "
            "any piece still missing from the baseline gets named in its set's "
            "review flag rather than dropped",
        ],
    }
    cov_path = common.coverage("options_sets", info)
    print(json.dumps({"item_options": info["files"]["item_options.json"]["records"],
                      "item_sets": info["files"]["item_sets.json"]["records"],
                      "coverage": cov_path}))


if __name__ == "__main__":
    main()
