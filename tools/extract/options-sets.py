#!/usr/bin/env python3
"""Extract ancient_sets.json (v2 options_sets domain).

The v2 options_sets domain keeps ONE data file: the 36-record ancient set
roster. Everything else the v1 extractor emitted dies into Rust:

- item_options.json dies entirely: every option family (normal/luck/excellent/
  dinorant/2nd-wing/jewelry-accessory/ancient-per-piece-bonus) is a closed enum
  in core/src/components/item_options.rs with magnitudes as services constants.
- The 45 generic armor-set records die: OpenMU-invented x1.1/x1.05 values become
  the FULL_ARMOR_SET_* rule constants in core/src/services/item_sets.rs.
- Roll policy (0.25/0.001/0.3/0.1 + caps) moves to game_config's option_roll
  section (owned by the constants-exp extractor).

C#-scraping logic is reused verbatim from v1 (the numbers it pulls are correct);
only the EMIT shape changes to the v2 AncientSet contract. Each OpenMU ancient
set option (Stats.X, value, aggregate) is resolved to the stats-owned CombatBonus
wire shape inline (kind-tagged, snake_case), with the one shield-conditional
effect emitted as ConditionalSetBonus. No stat slugs, no PowerUp/Aggregate
vocabulary crosses into the output.

Source: VersionSeasonSix/Items/AncientSets.cs (36 sets + per-piece ancient
bonus, s6 dataset, curated 1.0-era backport).

Known source data bug fixed here: Kantata (set 9) option 6 is
(Stats.ExcellentDamageChance, 10.0f) where every comparable excellent-chance
option uses a 0.05-0.15 fraction. Emitted as percent 10 (from 0.10) with a
verbatim review note.
"""

import json
import os
import re

import common

INIT = common.OPENMU_ROOT + "/src/Persistence/Initialization"

ITEM_GROUPS = {
    "Swords": 0, "Axes": 1, "Scepters": 2, "Spears": 3, "Bows": 4, "Staff": 5,
    "Shields": 6, "Helm": 7, "Armor": 8, "Pants": 9, "Gloves": 10, "Boots": 11,
    "Orbs": 12, "Misc1": 13, "Misc2": 14, "Scrolls": 15,
}
AGGREGATE = {
    "AddRaw": "add_raw", "Multiplicate": "multiplicate",
    "AddFinal": "add_final", "Maximum": "maximum",
}

# OpenMU Stats.X -> CombatBonus kind, for options resolving to an integer amount
# (points/flat additions). AddRaw or AddFinal in source; the aggregate itself is
# a killed mechanism and is not emitted.
AMOUNT_KINDS = {
    "TotalStrength": "strength",
    "TotalAgility": "agility",
    "TotalVitality": "vitality",
    "TotalEnergy": "energy",
    "TotalLeadership": "command",         # the Broy set's "Leadership" bonus
    "MaximumHealth": "max_health",
    "MaximumMana": "max_mana",
    "MaximumAbility": "max_ability",
    "AbilityRecoveryAbsolute": "ability_recovery",
    "DefenseBase": "defense",
    "AttackRatePvm": "attack_rate",
    "MinimumPhysBaseDmg": "min_physical_damage",
    "MaximumPhysBaseDmg": "max_physical_damage",
    "SkillDamageBonus": "skill_damage",
    "FinalDamageBonus": "damage",         # flat final damage
    "CriticalDamageBonus": "critical_damage",
    "ExcellentDamageBonus": "excellent_damage",
}

# OpenMU Stats.X -> CombatBonus percent kind, source value is a 0..1 fraction
# (AddRaw) converted x100 to whole Percent points.
FRACTION_PCT_KINDS = {
    "CriticalDamageChance": "critical_chance_pct",
    "ExcellentDamageChance": "excellent_chance_pct",
    "DoubleDamageChance": "double_damage_chance_pct",
    "DefenseIgnoreChance": "defense_ignore_chance_pct",
    "TwoHandedWeaponDamageIncrease": "two_handed_weapon_damage_pct",
}

# OpenMU Stats.X -> CombatBonus percent kind, source value is a Multiplicate
# factor (1 + pct/100) converted to whole Percent points.
MULTIPLICATE_PCT_KINDS = {
    "WizardryBaseDmgIncrease": "wizardry_damage_pct",
}

# OpenMU Stats.X -> ConditionalSetBonus kind (effect depends on a runtime
# equipment fact, so it cannot be a resolved contribution). Source value is a
# 0..1 fraction converted x100.
CONDITIONAL_PCT_KINDS = {
    "DefenseIncreaseWithEquippedShield": "defense_with_shield_pct",
}

# Per-piece ancient bonus stat -> AncientPieceStat variant (snake_case). The
# roster grants only these four; no command piece bonus exists.
PIECE_STAT = {
    "TotalStrength": "strength",
    "TotalAgility": "agility",
    "TotalVitality": "vitality",
    "TotalEnergy": "energy",
}

BASE_REVIEW = (
    "s1-era ancient set backported from the s6 dataset; set_number ordering "
    "transcribed from OpenMU's initializer, pending verification against a "
    "classic SetItemOption client file"
)
KANTATA_NOTE = (
    "fixed OpenMU data bug: excellent damage chance 10.0 -> 0.10 (percent 10)"
)


def read(rel):
    with open(os.path.join(INIT, rel), encoding="utf-8-sig") as f:
        return f.read()


# ---------------------------------------------------------------------------
# C# scraping (reused from v1 — the numbers it pulls are correct)
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


# ---------------------------------------------------------------------------
# v2 emit — resolve OpenMU options to CombatBonus / ConditionalSetBonus
# ---------------------------------------------------------------------------

def whole_amount(value):
    assert abs(value - round(value)) < 1e-9, "non-integer amount: %r" % value
    return int(round(value))


def percent_from_fraction(value):
    pct = round(value * 100.0)
    assert abs(pct - value * 100.0) < 1e-6, "non-whole percent from %r" % value
    assert 0 <= pct <= 100, "percent %d out of Percent range (from %r)" % (pct, value)
    return int(pct)


def percent_from_multiplicate(value):
    pct = round((value - 1.0) * 100.0)
    assert abs(pct - (value - 1.0) * 100.0) < 1e-6, "non-whole percent from %r" % value
    assert 0 <= pct <= 100, "percent %d out of Percent range (from %r)" % (pct, value)
    return int(pct)


def set_option(stat_name, value, aggregate):
    """Resolve one (Stats.X, value, aggregate) to an AncientSetOption wire dict.

    The outer `scope` tag ("resolved" for a CombatBonus, "conditional" for a
    ConditionalSetBonus) is the core's internally-tagged discriminator: the two
    inner `kind` namespaces cannot collide at parse. Raises on an unmapped stat
    (fail loudly)."""
    if stat_name in AMOUNT_KINDS:
        assert aggregate in ("add_raw", "add_final"), \
            "unexpected aggregate %s for %s" % (aggregate, stat_name)
        return {"scope": "resolved", "kind": AMOUNT_KINDS[stat_name],
                "amount": whole_amount(value)}
    if stat_name in FRACTION_PCT_KINDS:
        assert aggregate == "add_raw", \
            "unexpected aggregate %s for %s" % (aggregate, stat_name)
        return {"scope": "resolved", "kind": FRACTION_PCT_KINDS[stat_name],
                "percent": percent_from_fraction(value)}
    if stat_name in MULTIPLICATE_PCT_KINDS:
        assert aggregate == "multiplicate", \
            "unexpected aggregate %s for %s" % (aggregate, stat_name)
        return {"scope": "resolved", "kind": MULTIPLICATE_PCT_KINDS[stat_name],
                "percent": percent_from_multiplicate(value)}
    if stat_name in CONDITIONAL_PCT_KINDS:
        assert aggregate == "add_raw", \
            "unexpected aggregate %s for %s" % (aggregate, stat_name)
        return {"scope": "conditional", "kind": CONDITIONAL_PCT_KINDS[stat_name],
                "percent": percent_from_fraction(value)}
    raise KeyError("unmapped ancient set option stat: " + stat_name)


def build_set_options(set_name, options):
    """Ordered set_options (unlock order) + whether the Kantata fix applied."""
    out = []
    kantata_fixed = False
    for stat_name, value, aggregate in options:
        if set_name == "Kantata" and stat_name == "ExcellentDamageChance" \
                and value == 10.0:
            value = 0.10
            kantata_fixed = True
        out.append(set_option(stat_name, value, aggregate))
    return out, kantata_fixed


def build_pieces(pieces):
    out = []
    for p in pieces:
        piece = {
            "item": common.item_ref(p["group"], p["number"]),
            "discriminator": p["discriminator"],  # AncientDiscriminator 1|2
        }
        if p["bonus"]:
            piece["bonus_stat"] = PIECE_STAT[p["bonus"]]  # omit = no per-piece bonus
        out.append(piece)
    return out


def build_records():
    records = []
    for s in sorted(parse_ancient_sets(), key=lambda s: s["number"]):
        set_options, kantata_fixed = build_set_options(s["name"], s["options"])
        review = BASE_REVIEW + ("; " + KANTATA_NOTE if kantata_fixed else "")
        records.append({
            "set_number": s["number"],                     # NonZeroU8 1..=36
            "name": (s["name"] + " " + s["family"]).strip(),
            "source_version": "s6",
            "review": review,
            "pieces": build_pieces(s["pieces"]),
            "set_options": set_options,
        })
    return records


# ---------------------------------------------------------------------------
# verification + main
# ---------------------------------------------------------------------------

ALL_KINDS = (set(AMOUNT_KINDS.values()) | set(FRACTION_PCT_KINDS.values())
             | set(MULTIPLICATE_PCT_KINDS.values())
             | set(CONDITIONAL_PCT_KINDS.values()))


def verify(path):
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    records = data["records"]
    assert len(records) == 36, "expected 36 records, got %d" % len(records)
    seen_numbers = set()
    for r in records:
        n = r["set_number"]
        assert isinstance(n, int) and 1 <= n <= 255, r
        assert n not in seen_numbers, "duplicate set_number %d" % n
        seen_numbers.add(n)
        assert r["source_version"] == "s6", r
        assert r["review"], r
        assert "name" not in r, "core record must not carry a display name"
        assert r["pieces"], r
        assert r["set_options"], r
        for p in r["pieces"]:
            assert set(p["item"]) == {"group", "number"}, p
            assert p["discriminator"] in (1, 2), p
            if "bonus_stat" in p:
                assert p["bonus_stat"] in ("strength", "agility", "vitality",
                                           "energy"), p
        for opt in r["set_options"]:
            assert opt["scope"] in ("resolved", "conditional"), opt
            assert opt["kind"] in ALL_KINDS, opt
            assert ("amount" in opt) ^ ("percent" in opt), opt
            if "percent" in opt:
                assert 0 <= opt["percent"] <= 100, opt
    return records


def main():
    records = build_records()

    # v1 outputs of this extractor die in v2: item_options.json entirely, and
    # item_sets.json is superseded by ancient_sets.json (ancients only).
    for dead in ("item_options.json", "item_sets.json"):
        dead_path = os.path.join(common.DATA_DIR, dead)
        if os.path.exists(dead_path):
            os.remove(dead_path)

    # Display names -> host-owned sidecar, keyed by set_number; the core file
    # carries only identities and rules.
    common.write_names("ancient_sets.json", {"records": [
        {"set_number": r["set_number"], "name": r["name"]} for r in records]})
    path = common.write_datafile(
        "ancient_sets.json", [common.without_name(r) for r in records])
    verify(path)

    info = {
        "category": "options_sets",
        "file": "data/ancient_sets.json",
        "counts": {"075": 0, "095d": 0, "s6": len(records), "total": len(records)},
        "review": [{"set_number": r["set_number"], "name": r["name"],
                    "review": r["review"]} for r in records],
        "gaps": [
            "item_options.json deleted: every option family (normal/luck/"
            "excellent/dinorant/2nd-wing/jewelry-accessory/ancient-per-piece) is "
            "a closed Rust enum in core/src/components/item_options.rs with "
            "magnitudes as services constants; no option data file remains",
            "45 generic armor-set records deleted: the OpenMU-invented "
            "x1.1/x1.05 BuildSets values become the FULL_ARMOR_SET_* rule "
            "constants in core/src/services/item_sets.rs (review-flagged there)",
            "option roll policy (luck/option 0.25, extra-excellent 0.001, "
            "dinorant 0.3, 2nd-wing 0.1, max 2 excellent, max option level 3) "
            "moves to game_config's option_roll section (constants-exp extractor)",
            "per-piece +5/+10 ancient bonus tier + roll live in Rust "
            "(AncientBonusLevel + ancient_piece_bonus + roll_ancient_bonus_level); "
            "the piece here carries only its bonus_stat",
        ],
        "notes": [
            "each set option resolves to the stats-owned CombatBonus inline "
            "(kind-tagged snake_case); no stat slug, PowerUp, Aggregate, or "
            "Operator vocabulary is emitted",
            "percent encodings: AddRaw chance fractions x100 (0.05->5, 0.15->15, "
            "0.20->20, 0.25->25, 0.30->30); Multiplicate wizardry factors as "
            "1+pct/100 (1.05->5, 1.10->10, 1.15->15); both land in Percent 0..=100",
            "amount encodings: bare integers, preserved verbatim (defense uses "
            "AddFinal in source but is still a flat amount)",
            "DefenseIncreaseWithEquippedShield -> ConditionalSetBonus "
            "defense_with_shield_pct (Anonymous 25, Heras/Ceto 5); its kind "
            "namespace is disjoint from CombatBonus so the untagged split parses",
            "TotalLeadership (Broy set 34) -> CombatBonus command amount 30",
            "Kantata (set 9) data bug fixed: ExcellentDamageChance 10.0 -> 0.10 "
            "(excellent_chance_pct percent 10); review note carried on the record",
            "name = OpenMU set name + armor-family comment (\"Warrior\" + "
            "\"Leather\" = \"Warrior Leather\"); display/debug only, never a key",
            "discriminator is the client's 1|2 ancient-set selector "
            "(AncientDiscriminator); Gywen's Pendant of Ability (28, Misc1) is "
            "the one piece with no per-piece bonus (bonus_stat omitted)",
            "piece ItemRefs are the sets' only cross-file references; Atlas "
            "proves each resolves to an item_definitions.json record at load",
        ],
    }
    cov_path = common.coverage("options_sets", info)
    print(json.dumps({"file": "ancient_sets.json", "records": len(records),
                      "by_source_version": {"s6": len(records)},
                      "coverage": cov_path}))


if __name__ == "__main__":
    main()
