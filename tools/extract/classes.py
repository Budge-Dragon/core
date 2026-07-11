#!/usr/bin/env python3
"""Extract data/classes.json (v2 ClassRecord) from OpenMU class initializers.

Eight records, the complete pre-S3 roster. No "global" pseudo-record: the
shared attribute-graph relationships/const values became Rust services, and
per-class derived-stat formulas live in services/ too. This extractor keeps
only the extracted per-class *values*.

The C#-scraped numbers (base stats, starting vitals, points-per-level) are
pulled straight from the shared class initializers — identical code runs for
075/095d. Second tiers (soul_master/blade_knight/muse_elf) reuse their base
line's creator, so their creation values mirror the base tier; they are 1.0-era
backports present only in the s6 dataset (review-flagged). dark_lord is likewise
an s6-era backport. Gating/home-map/warp/fruit values are verified verbatim
against source (LevelRequirementByCreation 220/250, HomeMap Lorencia 0 / Noria 3,
LevelWarpRequirementReductionPercent = ceil(100/3) = 34 -> 2/3 fraction).
"""

import json
import os
import re

import common

CC_DIR = common.OPENMU_ROOT + "/src/Persistence/Initialization/CharacterClasses"

# raw OpenMU Stats.<token> on a CreateStatAttributeDefinition -> v2 destination.
# Everything else in the base-stat block (Level, IsInSafezone, Resets,
# AmmunitionAmount, MasterLevel, CurrentShield) is engine bookkeeping, dropped.
STARTING_STAT_FIELDS = {
    "BaseStrength": "strength",
    "BaseAgility": "agility",
    "BaseVitality": "vitality",
    "BaseEnergy": "energy",
}
COMMAND_TOKEN = "BaseLeadership"
VITAL_FIELDS = {
    "CurrentHealth": "health",
    "CurrentMana": "mana",
    "CurrentAbility": "ability",
}
POINTS_TOKEN = "PointsPerLevelUp"

STAT_DEF_RE = re.compile(
    r"CreateStatAttributeDefinition\(Stats\.(\w+),\s*([^,]+?),\s*(?:true|false)\)")


# ---------------------------------------------------------------- C# scraping

def read(name):
    with open(os.path.join(CC_DIR, name), encoding="utf-8-sig") as f:
        return f.read()


def match_brace(text, open_idx, open_ch="{", close_ch="}"):
    depth = 0
    for i in range(open_idx, len(text)):
        if text[i] == open_ch:
            depth += 1
        elif text[i] == close_ch:
            depth -= 1
            if depth == 0:
                return i
    raise ValueError("unbalanced braces")


def method_body(text, name):
    m = re.search(r"(?:private|protected|public)[^\n(]*\b" + name + r"\(", text)
    if not m:
        raise ValueError("method not found: " + name)
    open_idx = text.index("{", text.index(")", m.end()))
    return text[open_idx + 1:match_brace(text, open_idx)]


def num(expr):
    expr = re.sub(r"(?<=[\d.])f\b", "", expr).strip()
    if not re.fullmatch(r"[\d.]+", expr):
        raise ValueError("not a plain number: " + expr)
    value = float(expr)
    return int(value) if value == int(value) else value


def scrape_stat_defs(body):
    """{raw Stats token -> numeric value} for every base-stat definition."""
    return {name: num(val) for name, val in STAT_DEF_RE.findall(body)}


# ------------------------------------------------------------ record building

def pick(defs, token, ctx):
    if token not in defs:
        raise ValueError("{}: missing base stat {}".format(ctx, token))
    return defs[token]


def starting_stats(defs, ctx):
    fields = {v: pick(defs, k, ctx) for k, v in STARTING_STAT_FIELDS.items()}
    if COMMAND_TOKEN in defs:
        return {"kind": "with_command", **fields, "command": defs[COMMAND_TOKEN]}
    return {"kind": "standard", **fields}


def starting_vitals(defs, ctx):
    return {v: pick(defs, k, ctx) for k, v in VITAL_FIELDS.items()}


# Review strings. The 075 base tiers, s6 backports (SM/BK/ME/DL) and the 095d
# hybrid each carry the OpenMU-default flags they own: starting ability 1
# (initializer seed), the fruit divisor (OpenMU FruitCalculationStrategy), the
# level-150 evolution (1.0-era content) and the 2/3 warp fraction (OpenMU's
# ceil(100/3)=34 percent encoding). dark_wizard / soul_master / dark_lord match
# the design section verbatim; the rest follow the same pattern per line.
BASE_REVIEW = (
    "starting ability 1 is an OpenMU initializer seed; fruit divisor encodes "
    "OpenMU's FruitCalculationStrategy (cap ~127 community-verified, per-level "
    "curve pending); the level-150 evolution is 1.0-era content backported from "
    "the s6 dataset")
SECOND_REVIEW = (
    "1.0-era backport: second tiers are absent from the 075/095d datasets; "
    "creation values mirror the base tier; starting ability 1 is an OpenMU "
    "initializer seed; fruit divisor as on {base}")
MG_REVIEW = (
    "starting ability 1 is an OpenMU initializer seed; fruit divisor encodes "
    "OpenMU's FruitCalculationStrategy (cap ~100); warp: authentic rule is 2/3 "
    "of the gate requirement, rounding direction pending classic verification "
    "(OpenMU encodes a 34% integer reduction)")
DL_REVIEW = (
    "1.0-era backport; warp: authentic rule is 2/3 of the gate requirement, "
    "rounding direction pending classic verification (OpenMU encodes a 34% "
    "integer reduction); fruit divisor encodes OpenMU strategy (cap ~115); "
    "starting ability 1 is an OpenMU initializer seed")

ALWAYS = {"kind": "always"}
EVOLUTION_ONLY = {"kind": "evolution_only"}
TERMINAL = {"kind": "terminal"}
FULL = {"kind": "full"}
TWO_THIRDS = {"kind": "fraction", "numerator": 2, "denominator": 3}


def evolves(into):
    return {"kind": "evolves", "into": into, "at_level": 150}


def unlocked_at(level):
    return {"kind": "unlocked_at", "level": level}


# class, number, C# creator, source file, source_version, home_map,
# creation, evolution, fruit_points_divisor, warp_requirement, review
SPECS = [
    ("dark_wizard", 0, "CreateDarkWizard", "ClassDarkWizard.cs", "075", 0,
     ALWAYS, evolves("soul_master"), 400, FULL, BASE_REVIEW),
    ("soul_master", 2, "CreateDarkWizard", "ClassDarkWizard.cs", "s6", 0,
     EVOLUTION_ONLY, TERMINAL, 400, FULL, SECOND_REVIEW.format(base="dark_wizard")),
    ("dark_knight", 4, "CreateDarkKnight", "ClassDarkKnight.cs", "075", 0,
     ALWAYS, evolves("blade_knight"), 400, FULL, BASE_REVIEW),
    ("blade_knight", 6, "CreateDarkKnight", "ClassDarkKnight.cs", "s6", 0,
     EVOLUTION_ONLY, TERMINAL, 400, FULL, SECOND_REVIEW.format(base="dark_knight")),
    ("fairy_elf", 8, "CreateFairyElf", "ClassFairyElf.cs", "075", 3,
     ALWAYS, evolves("muse_elf"), 400, FULL, BASE_REVIEW),
    ("muse_elf", 10, "CreateFairyElf", "ClassFairyElf.cs", "s6", 3,
     EVOLUTION_ONLY, TERMINAL, 400, FULL, SECOND_REVIEW.format(base="fairy_elf")),
    ("magic_gladiator", 12, "CreateMagicGladiator", "ClassMagicGladiator.cs", "095d", 0,
     unlocked_at(220), TERMINAL, 700, TWO_THIRDS, MG_REVIEW),
    ("dark_lord", 16, "CreateDarkLord", "ClassDarkLord.cs", "s6", 0,
     unlocked_at(250), TERMINAL, 500, TWO_THIRDS, DL_REVIEW),
]


def main():
    bodies = {}  # filename -> {creator: scraped defs}
    records = []
    for (cls, number, creator, filename, source_version, home_map,
         creation, evolution, fruit_divisor, warp, review) in SPECS:
        key = (filename, creator)
        if key not in bodies:
            bodies[key] = scrape_stat_defs(method_body(read(filename), creator))
        defs = bodies[key]
        records.append({
            "class": cls,
            "number": number,
            "creation": creation,
            "evolution": evolution,
            "home_map": home_map,
            "points_per_level": pick(defs, POINTS_TOKEN, cls),
            "starting_stats": starting_stats(defs, cls),
            "starting_vitals": starting_vitals(defs, cls),
            "fruit_points_divisor": fruit_divisor,
            "warp_requirement": warp,
            "source_version": source_version,
            "review": review,
        })

    # ------------------------------------------------------------- validation
    assert len(records) == 8, len(records)
    assert len({r["class"] for r in records}) == 8
    assert len({r["number"] for r in records}) == 8
    for r in records:
        assert r["source_version"] in ("075", "095d", "s6"), r["class"]
        ss = r["starting_stats"]
        assert (ss["kind"] == "with_command") == (r["class"] == "dark_lord"), r["class"]
        if ss["kind"] == "with_command":
            assert ss["energy"] >= 15, r["class"]  # command-class floor
        v = r["starting_vitals"]
        assert set(v) == {"health", "mana", "ability"}, r["class"]
        assert isinstance(r["home_map"], int) and 0 <= r["home_map"] <= 255
        assert r["fruit_points_divisor"] > 0

    path = common.write_datafile("classes.json", records)
    with open(path, encoding="utf-8") as f:
        loaded = json.load(f)
    assert len(loaded["records"]) == 8

    # --------------------------------------------------------------- coverage
    by_version = {}
    for r in records:
        by_version[r["source_version"]] = by_version.get(r["source_version"], 0) + 1

    info = {
        "category": "classes",
        "files": {"data/classes.json": len(records)},
        "records_by_source_version": by_version,
        "review_flagged": [{"class": r["class"], "review": r["review"]} for r in records],
        "gaps": [
            "stat_formulas + const_values (v1 per-class attribute-graph relationships and "
            "base const values) dropped from data: they became Rust services (per-class "
            "derived-stat formulas in stats_replacement; shared relationships in the common "
            "combat/vitals services). No number lost - coefficients live in the service sketches.",
            "master/pet/shield formulas (S2+/S4+): raven/horse/dark-raven and master-tree "
            "buckets are excluded pre-S3 and never entered v2 records.",
            "second-class derived formulas (SM/BK/ME/DL): the s6 dataset ships shield/pvp-rate "
            "blocks excluded pre-S3; only creation values are carried, under s6 provenance.",
        ],
        "notes": [
            "no 'global' pseudo-record: v2 classes.json is a plain 8-record DataFile; shared "
            "behaviour is service code, not a data record.",
            "base stats/vitals/points scraped verbatim from the shared class initializers "
            "(CreateStatAttributeDefinition); gating/home-map/warp/fruit verified against source "
            "(LevelRequirementByCreation 220/250, HomeMap Lorencia 0 / Noria 3, "
            "LevelWarpRequirementReductionPercent = ceil(100/3) = 34 -> 2/3 fraction).",
            "second tiers reuse their base line's creator, so creation values mirror the base "
            "tier - honest source fact carried under s6 provenance, not a copy-generation device.",
            "warp 34% integer reduction re-expressed as the authentic 2/3 fraction (rounding "
            "direction re-sourced under W-SRC); starting ability 1, fruit divisor and the "
            "level-150 evolution flagged OpenMU defaults on every affected record.",
            "unlock levels 220/250 and evolution level 150 are authentic; the v1 doc-comment "
            "'account level' error is corrected - it is the level of another character on the account.",
        ],
    }
    cov_path = common.coverage("classes", info)

    print(json.dumps({"data": path, "coverage": cov_path,
                      "records": len(records), "by_version": by_version,
                      "reviews": len(info["review_flagged"])}))


if __name__ == "__main__":
    main()
