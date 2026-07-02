#!/usr/bin/env python3
"""Extract data/character_classes.json (spec section 2) from OpenMU class initializers.

Sources (shared CharacterClasses/ code is what 075/095d actually run):
  /tmp/openmu-ref/src/Persistence/Initialization/CharacterClasses/*.cs
Baseline: 095d classic-PvP class set (DW/DK/Elf 075-shared + MG 095d), plus curated
1.0-era backports from the S6 creators: Dark Lord and the second classes
(soul_master/blade_knight/muse_elf, evolution at level 150). Classic-PvP branch is
taken everywhere; master/shield/pet(S2+)/master-tree formulas are dropped and named
in the coverage gaps.
"""

import json
import os
import re

import common

CC_DIR = "/tmp/openmu-ref/src/Persistence/Initialization/CharacterClasses"

STAT_MAP = common.load_stat_map()
KNOWN_SLUGS = set(STAT_MAP.values())

OPERATORS = {
    "InputOperator.Multiply": "multiply",
    "InputOperator.Add": "add",
    "InputOperator.Exponentiate": "exponentiate",
    "InputOperator.ExponentiateByAttribute": "exponentiate_by_attribute",
    "InputOperator.Minimum": "minimum",
    "InputOperator.Maximum": "maximum",
}
AGGREGATES = {
    "AggregateType.AddRaw": "add_raw",
    "AggregateType.Multiplicate": "multiplicate",
    "AggregateType.AddFinal": "add_final",
    "AggregateType.Maximum": "maximum",
}

# Why a stat referenced by the source is intentionally absent from stat_map.json.
# Keyed by slugified OpenMU designation. Anything not listed here that fails to
# resolve is a genuine stat_map gap.
EXCLUDED_STATS = {
    "master_level": "master system (S4+) excluded; pre-S3 total_level == level",
    "master_points_per_level_up": "master system (S4+) excluded",
    "master_experience_rate": "master system (S4+) excluded",
    "resets": "reset system is a custom server feature, not classic pre-S3",
    "fenrir_base_dmg": "fenrir trainable pet (S2) excluded per spec",
    "raven_attack_damage_increase": "dark raven trainable pet excluded per spec",
    "raven_minimum_damage": "dark raven trainable pet excluded per spec",
    "raven_maximum_damage": "dark raven trainable pet excluded per spec",
    "raven_attack_speed": "dark raven trainable pet excluded per spec",
    "raven_attack_rate": "dark raven trainable pet excluded per spec",
    "raven_critical_damage_chance": "dark raven trainable pet excluded per spec",
    "raven_level": "dark raven trainable pet excluded per spec",
    "raven_bonus_damage": "dark raven trainable pet excluded per spec",
    "scepter_pet_bonus_damage": "master skill tree bucket (S4+) excluded",
    "bonus_damage_with_scepter_cmd_div": "master skill tree bucket (S4+) excluded",
    "master_skill_phys_bonus_dmg": "master skill tree bucket (S4+) excluded",
    "weapon_mastery_attack_speed": "master skill tree bucket (S4+) excluded",
    "one_handed_staff_bonus_base_damage": "master skill tree bucket (S4+) excluded",
    "two_handed_staff_bonus_base_damage": "master skill tree bucket (S4+) excluded",
    "bonus_defense_with_shield": "master skill tree bucket (S4+) excluded",
    "bonus_defense_rate_with_shield": "master skill tree bucket (S4+) excluded",
    "is_horse_equipped": "dark horse trainable pet excluded per spec",
    "bonus_defense_with_horse": "dark horse trainable pet excluded per spec",
    "damage_receive_horse_decrement": "dark horse trainable pet excluded per spec",
    "shield_item_defense_increase": "water-socket (S3+) shield defense multiplier excluded per facts",
    "innovation_def_decrement": "innovation skill (S3+) excluded",
    "temp_innovation_defense_decrement": "innovation skill (S3+) excluded",
}


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
    return text[open_idx + 1:match_brace(text, open_idx - 0)]


def resolve_branches(body):
    """Take the classic-PvP path: keep `if (this.UseClassicPvp)` bodies (drop else),
    drop every other conditional block (isMaster, !UseClassicPvp)."""
    while True:
        m = re.search(r"if \(([^)]+)\)\s*\{", body)
        if not m:
            return body
        cond = m.group(1).strip()
        open_idx = body.index("{", m.start())
        close_idx = match_brace(body, open_idx)
        inner = body[open_idx + 1:close_idx]
        rest = body[close_idx + 1:]
        em = re.match(r"\s*else\s*\{", rest)
        if em:
            eclose = match_brace(rest, rest.index("{"))
            rest = rest[eclose + 1:]
        keep = inner if cond == "this.UseClassicPvp" else ""
        body = body[:m.start()] + keep + rest


CREATORS = ("CreateAttributeRelationship", "CreateConditionalRelationship",
            "CreateConstValueAttribute", "CreateStatAttributeDefinition")
LOCAL_ATTR_RE = re.compile(
    r"var (\w+) = this\.Context\.CreateNew<AttributeDefinition>\(Guid\.NewGuid\(\), \"([^\"]+)\"")


def parse_calls(body):
    """Yield (creator, [arg strings]) for every this.Create*(...) call, in order."""
    calls = []
    for m in re.finditer(r"this\.(" + "|".join(CREATORS) + r")\(", body):
        start = m.end() - 1
        end = match_brace(body, start, "(", ")")
        args, depth, cur = [], 0, []
        for ch in body[start + 1:end]:
            if ch == "(":
                depth += 1
            elif ch == ")":
                depth -= 1
            if ch == "," and depth == 0:
                args.append("".join(cur).strip())
                cur = []
            else:
                cur.append(ch)
        args.append("".join(cur).strip())
        calls.append((m.group(1), args))
    return calls


def parse_locals(body):
    return {var: designation for var, designation in LOCAL_ATTR_RE.findall(body)}


def num(expr):
    # eval is deliberate: throwaway extractor evaluating constant arithmetic like
    # "1.0f / 3" scraped from the local OpenMU clone (trusted input, no builtins);
    # ast.literal_eval cannot handle division expressions.
    expr = re.sub(r"(?<=[\d.])f\b", "", expr)
    if not re.fullmatch(r"[\d.\s+\-*/()]+", expr):
        raise ValueError("not a constant expression: " + expr)
    return float(eval(expr, {"__builtins__": {}}))


def is_stat_token(tok):
    return tok.startswith("Stats.") or re.fullmatch(r"\w+", tok) is not None and not re.fullmatch(r"-?[\d.]+f?", tok)


# ------------------------------------------------------------ interpretation

class Extractor:
    def __init__(self):
        self.dropped = []  # {record, kind, missing:[slug], detail}
        self.record_id = None

    def stat(self, token, locals_map, missing):
        """Resolve a C# stat token to a mu-core slug; None + gap name if unmapped."""
        key = token[len("Stats."):] if token.startswith("Stats.") else locals_map.get(token, token)
        slug = STAT_MAP.get(key)
        if slug is None:
            missing.append(common.slugify(key))
        return slug

    def drop(self, kind, missing, detail):
        self.dropped.append({"record": self.record_id, "kind": kind,
                             "missing": sorted(set(missing)), "detail": detail})

    def formulas(self, calls, locals_map):
        out = []
        for creator, args in calls:
            missing = []
            if creator == "CreateAttributeRelationship":
                target = self.stat(args[0], locals_map, missing)
                source = self.stat(args[2], locals_map, missing)
                if args[1].startswith("Stats.") or args[1] in locals_map:
                    operand = {"kind": "stat", "stat": self.stat(args[1], locals_map, missing)}
                else:
                    operand = {"kind": "constant", "value": round(num(args[1]), 10)}
                operator, aggregate = "multiply", "add_raw"
                for extra in args[3:]:
                    extra = extra.split(":")[-1].strip()  # strip named-arg prefix
                    if extra in OPERATORS:
                        operator = OPERATORS[extra]
                    elif extra in AGGREGATES:
                        aggregate = AGGREGATES[extra]
            elif creator == "CreateConditionalRelationship":
                target = self.stat(args[0], locals_map, missing)
                source = self.stat(args[2], locals_map, missing)
                operand = {"kind": "stat", "stat": self.stat(args[1], locals_map, missing)}
                operator = "multiply"
                aggregate = "add_raw"
                for extra in args[3:]:
                    extra = extra.split(":")[-1].strip()
                    if extra in AGGREGATES:
                        aggregate = AGGREGATES[extra]
            else:
                continue
            if missing:
                self.drop("stat_formula", missing, "targets " + (target or missing[0]))
                continue
            out.append({"target": target, "input": source, "operator": operator,
                        "operand": operand, "aggregate": aggregate})
        return out

    def const_values(self, calls, locals_map):
        out = []
        for creator, args in calls:
            if creator != "CreateConstValueAttribute":
                continue
            missing = []
            slug = self.stat(args[1], locals_map, missing)
            if missing:
                self.drop("const_value", missing, "value {}".format(round(num(args[0]), 10)))
                continue
            out.append({"stat": slug, "value": round(num(args[0]), 10)})
        return out

    def base_stats(self, calls, locals_map):
        out = []
        for creator, args in calls:
            if creator != "CreateStatAttributeDefinition":
                continue
            missing = []
            slug = self.stat(args[0], locals_map, missing)
            if missing:
                self.drop("base_stat", missing, "value " + args[1])
                continue
            value = num(args[1])
            out.append({"stat": slug, "value": int(value) if value == int(value) else value,
                        "increasable": args[2] == "true"})
        return out


def main():
    ex = Extractor()
    shared = read("CharacterClassInitialization.cs")

    # --- global pseudo-record: common relationships + common/global const values
    ex.record_id = "global"
    common_rel_body = resolve_branches(method_body(shared, "AddCommonAttributeRelationships"))
    common_rel_locals = parse_locals(common_rel_body)
    global_formulas = ex.formulas(parse_calls(common_rel_body), common_rel_locals)

    common_base_body = resolve_branches(method_body(shared, "AddCommonBaseAttributeValues"))
    global_consts = ex.const_values(parse_calls(common_base_body), {})
    # GameConfigurationInitializerBase.AddGlobalBaseAttributeValues (shared by all versions)
    global_consts += [
        {"stat": "money_amount_rate", "value": 1.0},
        {"stat": "random_experience_min_multiplier", "value": 0.8},
        {"stat": "random_experience_max_multiplier", "value": 1.2},
        {"stat": "movement_speed_factor", "value": 1.0},
    ]

    dw_body = resolve_branches(method_body(shared, "AddDoubleWieldAttributeRelationships"))

    # --- per-class-line parsing (shared creators; identical code runs for 075/095d)
    lines = {}
    for method, filename in [("CreateDarkWizard", "ClassDarkWizard.cs"),
                             ("CreateDarkKnight", "ClassDarkKnight.cs"),
                             ("CreateFairyElf", "ClassFairyElf.cs"),
                             ("CreateMagicGladiator", "ClassMagicGladiator.cs"),
                             ("CreateDarkLord", "ClassDarkLord.cs")]:
        body = resolve_branches(method_body(read(filename), method))
        if "AddDoubleWieldAttributeRelationships" in body:
            body += dw_body  # DK and MG lines inherit the double-wield formulas
        lines[method] = (body, parse_locals(body))

    def class_record(rec_id, number, method, source_version, home_map_number,
                     created_by_player, creation_unlock_level, warp_reduction,
                     fruit, evolution, review=None):
        ex.record_id = rec_id
        body, locals_map = lines[method]
        calls = parse_calls(body)
        base_stats = ex.base_stats(calls, locals_map)
        points = next(s["value"] for s in base_stats if s["stat"] == "points_per_level_up")
        base_stats = [s for s in base_stats if s["stat"] != "points_per_level_up"]
        rec = {"id": rec_id, "number": number, "source_version": source_version}
        if review:
            rec["review"] = review
        rec.update({
            "created_by_player": created_by_player,
            "creation_unlock_level": creation_unlock_level,
            "home_map": common.map_ref(home_map_number),
            "points_per_level": points,
            "fruit_calculation": fruit,
            "warp_level_reduction_percent": warp_reduction,
            "evolution": evolution,
            "base_stats": base_stats,
            "const_values": ex.const_values(calls, locals_map),
            "stat_formulas": ex.formulas(calls, locals_map),
        })
        return rec

    evolution_review = ("evolution to {0} at level 150 is a curated 1.0-era backport "
                        "(075/095d datasets ship no second classes); rest of the record "
                        "is the shipped shared-initializer data")
    backport_review = ("1.0-era backport: second class of the {0} line (class change at "
                       "level 150 predates season 3); defined only in the s6 dataset; "
                       "rebuilt with classic-pvp rules, shield/master/pet formulas dropped")

    records = [
        {"id": "global", "source_version": "075",
         "const_values": global_consts, "stat_formulas": global_formulas},
        class_record("dark_wizard", 0, "CreateDarkWizard", "075", 0, True, 0, 0,
                     "default", {"class": "soul_master", "at_level": 150},
                     review=evolution_review.format("soul_master")),
        class_record("soul_master", 2, "CreateDarkWizard", "s6", 0, False, 0, 0,
                     "default", None, review=backport_review.format("dark_wizard")),
        class_record("dark_knight", 4, "CreateDarkKnight", "075", 0, True, 0, 0,
                     "default", {"class": "blade_knight", "at_level": 150},
                     review=evolution_review.format("blade_knight")),
        class_record("blade_knight", 6, "CreateDarkKnight", "s6", 0, False, 0, 0,
                     "default", None, review=backport_review.format("dark_knight")),
        class_record("fairy_elf", 8, "CreateFairyElf", "075", 3, True, 0, 0,
                     "default", {"class": "muse_elf", "at_level": 150},
                     review=evolution_review.format("muse_elf")),
        class_record("muse_elf", 10, "CreateFairyElf", "s6", 3, False, 0, 0,
                     "default", None, review=backport_review.format("fairy_elf")),
        class_record("magic_gladiator", 12, "CreateMagicGladiator", "095d", 0, True, 220, 34,
                     "magic_gladiator", None,
                     review="fruit_calculation curated to magic_gladiator per the domain "
                            "fruit caps (default 127 / mg 100 / dl 115); the shipped "
                            "initializer never assigns a strategy (all default) and "
                            "fruits are 1.0-era content"),
        class_record("dark_lord", 16, "CreateDarkLord", "s6", 0, True, 250, 34,
                     "dark_lord", None,
                     review="1.0-era backport: dark lord shipped around v1.0 but is absent "
                            "from the 075/095d datasets; rebuilt with classic-pvp rules; "
                            "raven/horse/pet and master-tree formulas dropped (see gaps); "
                            "fruit_calculation dark_lord per domain caps"),
    ]

    # ------------------------------------------------------------- validation
    allowed_ops = set(OPERATORS.values())
    allowed_aggs = set(AGGREGATES.values())
    for rec in records:
        assert rec["source_version"] in ("075", "095d", "s6"), rec["id"]
        assert rec["source_version"] != "s6" or "review" in rec, rec["id"]
        for f in rec["stat_formulas"]:
            assert f["operator"] in allowed_ops and f["aggregate"] in allowed_aggs, f
            assert f["operand"]["kind"] in ("constant", "stat"), f
            used = [f["target"], f["input"]] + ([f["operand"]["stat"]] if f["operand"]["kind"] == "stat" else [])
            assert all(s in KNOWN_SLUGS for s in used), f
        for c in rec["const_values"]:
            assert c["stat"] in KNOWN_SLUGS, c
        for s in rec.get("base_stats", []):
            assert s["stat"] in KNOWN_SLUGS, s

    path = common.write_datafile("character_classes.json", records)
    with open(path, encoding="utf-8") as f:
        json.load(f)  # round-trip check

    # --------------------------------------------------------------- coverage
    by_version = {}
    for rec in records:
        by_version[rec["source_version"]] = by_version.get(rec["source_version"], 0) + 1

    gap_index = {}
    for d in ex.dropped:
        for slug in d["missing"]:
            gap_index.setdefault(slug, []).append("{}:{} ({})".format(d["record"], d["kind"], d["detail"]))
    gaps = []
    for slug in sorted(gap_index):
        reason = EXCLUDED_STATS.get(slug, "MISSING from stat_map.json (possibly pre-S3 relevant) "
                                          "- stats agent should add or confirm exclusion")
        gaps.append("{}: {} - dropped {}".format(slug, reason, "; ".join(sorted(set(gap_index[slug])))))

    info = {
        "category": "classes",
        "files": {"data/character_classes.json": len(records)},
        "records_by_source_version": by_version,
        "review_flagged": [{"id": r["id"], "review": r["review"]} for r in records if "review" in r],
        "gaps": gaps,
        "notes": [
            "combo_bonus formulas (dark_knight line) ship in the shared initializer that 075/095d "
            "run, so they are kept under 075 even though the skill combo historically arrived ~1.0",
            "nova_stage_damage const 0.0 (dark_wizard line) ships in the shared initializer; the "
            "nova skill itself is a 1.0-era backport handled by the skills extractor",
            "can_fly gained from the dinorant-equipped flag ships in the shared initializer "
            "(dinorant is ~0.97 content); kept under 075",
            "total_level == level pre-S3: the master-level contribution to total_level was dropped "
            "with the master system",
            "second-class backports reuse the shared creators under classic-pvp rules; the s6 "
            "dataset ships them with shield/pvp-rate blocks which are excluded pre-S3",
            "the client creation-unlock bitmask (mg=4, dl=2) is a client protocol concern and is "
            "not modeled; creation_unlock_level carries the domain rule",
        ],
    }
    cov_path = common.coverage("classes", info)

    print(json.dumps({"data": path, "coverage": cov_path,
                      "records": len(records), "by_version": by_version,
                      "reviews": len(info["review_flagged"]), "gaps": len(gaps)}))


if __name__ == "__main__":
    main()
