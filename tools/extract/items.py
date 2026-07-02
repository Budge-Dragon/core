"""Extract data/item_definitions.json and data/item_level_bonus_tables.json.

Sources (OpenMU clone at /tmp/openmu-ref):
- Version095d/Items/{Weapons,Armors,Wings,Pets,Orbs,Scrolls,Jewels,Jewelery,
  BoxOfLuck,EventTicketItems}.cs (+ the Version075 initializers they reuse or
  subclass, used for source_version tagging)
- Version075/Items/Potions.cs (invoked directly by the 095d initializer)
- Items/ArmorInitializerBase.cs, WingsInitializerBase.cs (shared mechanics)
- VersionSeasonSix/Items/Wings.cs (curated 1.0-era backports: 2nd wings +
  Loch's Feather + Cape of Lord, tagged s6 + review)
- VersionSeasonSix/Items/Potions.cs CreateFruits (fruits chaos-mix result, s6)
- Version097d/Items/Jewels.cs (Jewel of Creation, fruits chaos-mix ingredient;
  tagged s6 because source_version has no 097d value)

Data lines (CreateWeapon/CreateArmor/... calls) are parsed from the C#;
the mechanics of those helpers are re-implemented below.

Conventions per docs/specs/2026-07-02-data-schemas.md (section 3 and 4):
item level cap 11 -> 12-entry dense bonus tables; class levels expanded to
slug lists (value 2 = second-stage only, per approved decision, diverging
from OpenMU's helper which also admits the first class); fractions stay as
source values; jewelry resistance stays in raw source units (source is
1 + level, NOT n/255 -- the n/255 convention applies to monster data).
"""

import re

import common

SRC = "/tmp/openmu-ref/src/Persistence/Initialization"

STAT_MAP = common.load_stat_map()


def stat(openmu_name):
    return STAT_MAP[openmu_name]  # KeyError = extraction bug, fail loudly


# ---------------------------------------------------------------------------
# tiny C# call parser
# ---------------------------------------------------------------------------

def read(path):
    with open(path, encoding="utf-8-sig") as f:
        return f.read()


def split_args(argstr):
    """Split a C# argument list, respecting quotes and nested parens."""
    args, depth, in_str, cur = [], 0, False, []
    for ch in argstr:
        if in_str:
            cur.append(ch)
            if ch == '"':
                in_str = False
        elif ch == '"':
            cur.append(ch)
            in_str = True
        elif ch in "(<[":
            depth += 1
            cur.append(ch)
        elif ch in ")>]":
            depth -= 1
            cur.append(ch)
        elif ch == "," and depth == 0:
            args.append("".join(cur).strip())
            cur = []
        else:
            cur.append(ch)
    if cur:
        args.append("".join(cur).strip())
    return args


def parse_value(token):
    token = token.strip()
    if token.startswith('"'):
        return token[1:-1]
    if token in ("true", "false"):
        return token == "true"
    try:
        return int(token)
    except ValueError:
        return token  # enum expressions etc., handled by caller


def calls(text, method):
    """All `this.Method(...)` calls in the file as parsed argument lists."""
    out = []
    for m in re.finditer(r"this\." + method + r"\((.*?)\);", text, re.S):
        out.append([parse_value(a) for a in split_args(m.group(1))])
    return out


def skill_numbers():
    """SkillNumber enum name -> number."""
    text = read(SRC + "/Skills/SkillNumber.cs")
    return {m.group(1): int(m.group(2))
            for m in re.finditer(r"^\s*(\w+) = (\d+),", text, re.M)}


SKILL_NUMBER = skill_numbers()

# ---------------------------------------------------------------------------
# class qualification expansion
# ---------------------------------------------------------------------------

CLASS_ORDER = ["dark_wizard", "soul_master", "dark_knight", "blade_knight",
               "fairy_elf", "muse_elf", "magic_gladiator", "dark_lord"]


def classes_from_levels(dw=0, dk=0, elf=0, mg=0, dl=0):
    """OpenMU class-level ints -> slug list.

    1 = base class (+ its evolved stage, since evolution is backported),
    2 = second-stage only (approved decision; OpenMU's own helper would also
    admit the base class -- divergence noted in coverage/review).
    """
    out = set()
    for level, first, second in ((dw, "dark_wizard", "soul_master"),
                                 (dk, "dark_knight", "blade_knight"),
                                 (elf, "fairy_elf", "muse_elf")):
        if level == 1:
            out.update((first, second))
        elif level == 2:
            out.add(second)
    if mg:
        out.add("magic_gladiator")
    if dl:
        out.add("dark_lord")
    return [c for c in CLASS_ORDER if c in out]


ALL_CLASSES = list(CLASS_ORDER)  # "all classes in config" items (jewelry)

# ---------------------------------------------------------------------------
# record builders
# ---------------------------------------------------------------------------


def power_up(stat_slug, value, aggregate, bonus_table=None):
    rec = {"stat": stat_slug, "value": float(value), "aggregate": aggregate}
    if bonus_table:
        rec["bonus_table"] = bonus_table
    return rec


def item(group, number, name, version, *, width, height, slot=None,
         drops=False, drop_level=0, max_drop_level=None, max_item_level=0,
         durability, value=0, skill=None, consume_effect=None, ammo=False,
         classes=(), requirements=(), power_ups=(), options=(),
         set_groups=(), box_drops=(), review=None):
    rec = {"id": {"group": group, "number": number},
           "name": name,
           "source_version": version}
    if review:
        rec["review"] = review
    rec["width"] = width
    rec["height"] = height
    if slot is not None:
        rec["slot"] = slot
    rec["drops_from_monsters"] = drops
    rec["drop_level"] = drop_level
    rec["maximum_drop_level"] = max_drop_level
    rec["max_item_level"] = max_item_level
    rec["durability"] = durability
    rec["value"] = value
    rec["skill"] = skill
    rec["consume_effect"] = consume_effect
    rec["is_ammunition"] = ammo
    rec["classes"] = list(classes)
    rec["requirements"] = [{"stat": s, "value": v} for s, v in requirements if v]
    rec["base_power_ups"] = list(power_ups)
    rec["possible_options"] = list(options)
    rec["possible_set_groups"] = list(set_groups)
    rec["box_drops"] = list(box_drops)
    return rec


def equip_skill(number):
    return {"kind": "granted_while_equipped", "skill": number}


def consume_skill(number):
    return {"kind": "taught_on_consume", "skill": number}


# ---------------------------------------------------------------------------
# bonus tables (12 entries, item levels 0..11; cap decision 2)
# ---------------------------------------------------------------------------

ITEM_LEVEL_CAP = 11
ARMOR_CURVE = [0, 3, 6, 9, 12, 15, 18, 21, 24, 27, 31, 36]  # of 16 in source


def dense12(values):
    """Pad/truncate to cap+1 entries; absent level = no bonus (0) in OpenMU."""
    values = list(values)[:ITEM_LEVEL_CAP + 1]
    values += [0] * (ITEM_LEVEL_CAP + 1 - len(values))
    return [int(v) if float(v) == int(v) else v for v in values]


def bonus_tables():
    def table(table_id, version, values, review=None):
        rec = {"id": table_id, "source_version": version}
        if review:
            rec["review"] = review
        rec["values_by_level"] = dense12(values)
        return rec

    return [
        # Version075/Items/Weapons.cs (reused values in 095d)
        table("weapon_damage", "075", [0, 3, 6, 9, 12, 15, 18, 21, 24, 27, 31, 36]),
        table("staff_rise_even", "075", [0, 3, 7, 10, 14, 17, 21, 24, 28, 31, 35, 40]),
        table("staff_rise_odd", "075", [0, 4, 7, 11, 14, 18, 21, 25, 28, 32, 36, 40]),
        # Version095d/Items/Weapons.cs (bolts/arrows, 3 source entries)
        table("ammunition_damage", "095d", [0, 0.03, 0.05]),
        # Items/ArmorInitializerBase.cs
        table("armor_defense", "075", ARMOR_CURVE),
        table("shield_defense", "075", list(range(12))),
        table("shield_defense_rate", "075", ARMOR_CURVE),
        table("running_movement_speed", "075", [0] * 5 + [15] * 7),
        # WingsInitializerBase.cs
        table("wing_absorb", "075", [-level * 2 / 100 for level in range(12)]),
        table("wing_damage_first", "075", [level * 2 / 100 for level in range(12)]),
        table("wing_damage_second", "s6",
              [level / 100 for level in range(12)],
              review="backported with 2nd wings; S6 table (1%/level) truncated to cap 11"),
        table("wing_defense", "075", ARMOR_CURVE),
        # Version075/Items/Jewelery.cs -- raw source units (1 + level), not n/255
        table("jewelry_resistance", "075", [0, 1, 2, 3, 4]),
        # Version075/Items/Jewelery.cs CreateTransformationRing (skin per level)
        table("transformation_ring_skins", "075", [2, 7, 14, 8, 9, 41]),
        # Version075/095d Weapons.cs DurabilityIncreasePerLevel (runtime table
        # in GameLogic/ItemExtensions.cs matches for levels 0..11)
        table("durability_per_level", "075", [0, 1, 2, 3, 4, 6, 8, 10, 12, 14, 17, 21]),
    ]


# ---------------------------------------------------------------------------
# weapons (group 0-5) -- parsed from Weapons.cs of both versions
# ---------------------------------------------------------------------------

def weapon_slot(slot, width, knight, mg):
    if slot == 0 and (knight > 0 or mg > 0) and width == 1:
        return "left_or_right_hand"
    return {0: "left_hand", 1: "right_hand"}[slot]


def build_weapons():
    text_095d = read(SRC + "/Version095d/Items/Weapons.cs")
    text_075 = read(SRC + "/Version075/Items/Weapons.cs")
    in_075 = {(a[0], a[1]) for a in calls(text_075, "CreateWeapon")}
    in_075 |= {(a[0], a[1]) for a in calls(text_075, "CreateAmmunition")}

    records = []
    for a in calls(text_095d, "CreateWeapon"):
        (group, number, slot, skill_no, width, height, drops, name, drop_level,
         min_dmg, max_dmg, speed, durability, magic_power, lvl_req, str_req,
         agi_req, ene_req, vit_req, wizard, knight, elf) = a[:22]
        mg = a[22] if len(a) > 22 else 0

        pups = [
            power_up(stat("MinimumPhysBaseDmgByWeapon"), min_dmg, "add_raw", "weapon_damage"),
            power_up(stat("MaximumPhysBaseDmgByWeapon"), max_dmg, "add_raw", "weapon_damage"),
            power_up(stat("AttackSpeedByWeapon"), speed, "add_raw"),
        ]
        if magic_power > 0:
            rise_table = "staff_rise_even" if magic_power % 2 == 0 else "staff_rise_odd"
            pups.append(power_up(stat("StaffRise"), magic_power / 2.0, "add_raw", rise_table))
        pups.append(power_up(stat("EquippedWeaponCount"), 1, "add_raw"))
        if group < 3 and width == 1:  # below Spears
            pups.append(power_up(stat("DoubleWieldWeaponCount"), 1, "add_raw"))
        if group == 4:  # Bows
            pups.append(power_up(stat("AmmunitionConsumptionRate"), 1, "add_raw"))
        if group < 4 and width == 2:
            pups.append(power_up(stat("IsTwoHandedWeaponEquipped"), 1, "add_raw"))

        if magic_power == 0:
            options = ["luck", "physical_attack", "excellent_physical"]
        else:
            options = ["luck", "wizardry_attack", "excellent_wizardry"]

        records.append(item(
            group, number, name,
            "075" if (group, number) in in_075 else "095d",
            width=width, height=height,
            slot=weapon_slot(slot, width, knight, mg),
            drops=drops, drop_level=drop_level, max_item_level=ITEM_LEVEL_CAP,
            durability=durability,
            skill=equip_skill(skill_no) if skill_no > 0 else None,
            classes=classes_from_levels(dw=wizard, dk=knight, elf=elf,
                                        mg=1 if (wizard == 1 or knight == 1 or mg == 1) else 0),
            requirements=[("level", lvl_req),
                          (stat("TotalStrengthRequirementValue"), str_req),
                          (stat("TotalAgilityRequirementValue"), agi_req),
                          (stat("TotalEnergyRequirementValue"), ene_req),
                          (stat("TotalVitalityRequirementValue"), vit_req)],
            power_ups=pups, options=options))

    for a in calls(text_095d, "CreateAmmunition"):
        (group, number, slot, width, height, drops, name, drop_level,
         durability, wizard, knight, elf) = a
        records.append(item(
            group, number, name,
            "075" if (group, number) in in_075 else "095d",
            width=width, height=height,
            slot=weapon_slot(slot, width, knight, 0),
            drops=drops, drop_level=drop_level, max_item_level=0,
            durability=durability, ammo=True,
            classes=classes_from_levels(dw=wizard, dk=knight, elf=elf),
            power_ups=[power_up(stat("AmmunitionDamageBonus"), 0, "add_raw",
                                "ammunition_damage")]))
    return records


# ---------------------------------------------------------------------------
# armors + shields (groups 6-11) -- parsed from Armors.cs
# (data lines verified identical between 075 and 095d, so all tagged 075)
# ---------------------------------------------------------------------------

ARMOR_SLOTS = {2: "helm", 3: "armor", 4: "pants", 5: "gloves", 6: "boots"}

# BuildSets in ArmorInitializerBase: groups 7-11 grouped by number; set name
# prefix = first word of the first created piece (the helm). Slugs must match
# the item_sets.json extractor (same slugify rule on OpenMU set-group names).
SET_PREFIX = {0: "Bronze", 1: "Dragon", 2: "Pad", 3: "Legendary", 4: "Bone",
              5: "Leather", 6: "Scale", 7: "Sphinx", 8: "Brass", 9: "Plate",
              10: "Vine", 11: "Silk", 12: "Wind", 13: "Spirit", 14: "Guardian"}


def set_groups_for(number):
    prefix = common.slugify(SET_PREFIX[number])
    return [prefix + "_defense_rate_bonus",
            prefix + "_defense_bonus_level_10",
            prefix + "_defense_bonus_level_11"]


def armor_common(defense):
    if defense > 0:
        return [power_up(stat("DefenseBase"), defense, "add_raw", "armor_defense")]
    return []


def build_armors():
    text_095d = read(SRC + "/Version095d/Items/Armors.cs")
    text_075 = read(SRC + "/Version075/Items/Armors.cs")
    for method in ("CreateShield", "CreateArmor", "CreateGloves", "CreateBoots"):
        assert calls(text_075, method) == calls(text_095d, method), \
            "075/095d armor data diverged; version tagging needs a real diff"

    records = []

    for a in calls(text_095d, "CreateShield"):
        (number, slot, skill_no, width, height, name, drop_level, defense,
         defense_rate, durability, str_req, agi_req, dw, dk, elf) = a
        pups = []
        if defense > 0:
            pups.append(power_up(stat("DefenseShield"), defense, "add_raw", "shield_defense"))
        if defense_rate > 0:
            pups.append(power_up(stat("DefenseRatePvm"), defense_rate, "add_raw", "shield_defense_rate"))
        pups.append(power_up(stat("IsShieldEquipped"), 1, "add_raw"))
        records.append(item(
            6, number, name, "075", width=width, height=height,
            slot="right_hand", drops=True, drop_level=drop_level,
            max_item_level=ITEM_LEVEL_CAP, durability=durability,
            skill=equip_skill(skill_no) if skill_no != 0 else None,
            classes=classes_from_levels(dw=dw, dk=dk, elf=elf),  # shields never add MG
            requirements=[(stat("TotalStrengthRequirementValue"), str_req),
                          (stat("TotalAgilityRequirementValue"), agi_req)],
            power_ups=pups,
            options=["luck", "excellent_defense", "defense_rate"]))

    for a in calls(text_095d, "CreateArmor"):
        (number, slot, width, height, name, drop_level, defense, durability,
         str_req, agi_req, dw, dk, elf) = a
        group = slot + 5
        # short CreateArmor overload: MG auto-qualifies on non-helm DW/DK gear
        mg = 1 if group != 7 and (dw == 1 or dk == 1) else 0
        records.append(item(
            group, number, name, "075", width=width, height=height,
            slot=ARMOR_SLOTS[slot], drops=True, drop_level=drop_level,
            max_item_level=ITEM_LEVEL_CAP, durability=durability,
            classes=classes_from_levels(dw=dw, dk=dk, elf=elf, mg=mg),
            requirements=[(stat("TotalStrengthRequirementValue"), str_req),
                          (stat("TotalAgilityRequirementValue"), agi_req)],
            power_ups=armor_common(defense),
            options=["luck", "defense", "excellent_defense"],
            set_groups=set_groups_for(number)))

    for a in calls(text_095d, "CreateGloves"):
        (number, name, drop_level, defense, attack_speed, durability,
         str_req, agi_req, dw, dk, elf) = a
        pups = armor_common(defense)
        if attack_speed > 0:
            pups.append(power_up(stat("AttackSpeedAny"), attack_speed, "add_raw"))
        pups.append(power_up(stat("MovementSpeedUnderwater"), 0, "maximum",
                             "running_movement_speed"))
        records.append(item(
            10, number, name, "075", width=2, height=2, slot="gloves",
            drops=True, drop_level=drop_level, max_item_level=ITEM_LEVEL_CAP,
            durability=durability,
            classes=classes_from_levels(dw=dw, dk=dk, elf=elf,
                                        mg=1 if (dw == 1 or dk == 1) else 0),
            requirements=[(stat("TotalStrengthRequirementValue"), str_req),
                          (stat("TotalAgilityRequirementValue"), agi_req)],
            power_ups=pups,
            options=["luck", "defense", "excellent_defense"],
            set_groups=set_groups_for(number)))

    for a in calls(text_095d, "CreateBoots"):
        (number, slot, width, height, name, drop_level, defense, walk_speed,
         durability, str_req, agi_req, dw, dk, elf) = a
        pups = armor_common(defense)
        if walk_speed > 0:
            pups.append(power_up(stat("WalkSpeed"), walk_speed, "add_raw"))
        pups.append(power_up(stat("MovementSpeed"), 0, "maximum",
                             "running_movement_speed"))
        records.append(item(
            11, number, name, "075", width=2, height=2, slot="boots",
            drops=True, drop_level=drop_level, max_item_level=ITEM_LEVEL_CAP,
            durability=durability,
            classes=classes_from_levels(dw=dw, dk=dk, elf=elf,
                                        mg=1 if (dw == 1 or dk == 1) else 0),
            requirements=[(stat("TotalStrengthRequirementValue"), str_req),
                          (stat("TotalAgilityRequirementValue"), agi_req)],
            power_ups=pups,
            options=["luck", "defense", "excellent_defense"],
            set_groups=set_groups_for(number)))

    return records


# ---------------------------------------------------------------------------
# s6 armor backports -- ancient-set pieces (VersionSeasonSix/Items/Armors.cs)
# ---------------------------------------------------------------------------

# The approved ancient-set backport (item_sets.json, s6) references three armor
# families that do not exist in the 095d dataset. Without these items the set
# pieces are dangling references, so they ride along as curated s6 backports.
S6_ARMOR_FAMILIES = {15: "Storm Crow", 26: "Adamantine", 40: "Red Wing"}


def s6_armor_classes_review(name, dw, dk, elf, mg, dl, summoner, rf):
    assert not (dw or dk or elf or rf), name
    if summoner:
        return [], ("Red Wing is Summoner (post-pre-S3 class) gear, not 1.0-era; "
                    "backported only as a piece of the approved chrono/semeden "
                    "ancient-set backports; classes empty -> unequippable in baseline")
    if dl:
        return (classes_from_levels(dl=1),
                "1.0-era Dark Lord (Adamantine) piece backported from the S6 "
                "dataset for the agnis/broy ancient sets; other DL gear is not "
                "backported this wave")
    assert mg, name
    return (classes_from_levels(mg=1),
            "1.0-era Magic Gladiator (Storm Crow) piece backported from the S6 "
            "dataset for the gaion/muren ancient sets")


def build_s6_armors():
    """Storm Crow (MG), Adamantine (DL) and Red Wing (Summoner) pieces from the
    S6 Armors.cs long overloads (extra level/energy/vitality/leadership
    requirement columns and class columns). No possible_set_groups: the generic
    BuildSets families only cover armor numbers 0..14."""
    text = read(SRC + "/VersionSeasonSix/Items/Armors.cs")
    records = []

    for a in calls(text, "CreateArmor"):
        if len(a) != 21 or a[0] not in S6_ARMOR_FAMILIES:
            continue
        (number, slot, width, height, name, drop_level, defense, durability,
         lvl_req, str_req, agi_req, ene_req, vit_req, lead_req,
         dw, dk, elf, mg, dl, summoner, rf) = a
        assert name.startswith(S6_ARMOR_FAMILIES[number]), name
        assert not (ene_req or vit_req or lead_req), name
        classes, review = s6_armor_classes_review(name, dw, dk, elf, mg, dl,
                                                  summoner, rf)
        records.append(item(
            slot + 5, number, name, "s6", review=review,
            width=width, height=height, slot=ARMOR_SLOTS[slot],
            drops=True, drop_level=drop_level, max_item_level=ITEM_LEVEL_CAP,
            durability=durability, classes=classes,
            requirements=[("level", lvl_req),
                          (stat("TotalStrengthRequirementValue"), str_req),
                          (stat("TotalAgilityRequirementValue"), agi_req)],
            power_ups=armor_common(defense),
            options=["luck", "defense", "excellent_defense"]))

    for a in calls(text, "CreateGloves"):
        if len(a) != 15 or a[0] not in S6_ARMOR_FAMILIES:
            continue
        (number, name, drop_level, defense, attack_speed, durability,
         lvl_req, str_req, agi_req, dw, dk, elf, mg, dl, summoner) = a
        assert name.startswith(S6_ARMOR_FAMILIES[number]), name
        classes, review = s6_armor_classes_review(name, dw, dk, elf, mg, dl,
                                                  summoner, 0)
        pups = armor_common(defense)
        if attack_speed > 0:
            pups.append(power_up(stat("AttackSpeedAny"), attack_speed, "add_raw"))
        pups.append(power_up(stat("MovementSpeedUnderwater"), 0, "maximum",
                             "running_movement_speed"))
        records.append(item(
            10, number, name, "s6", review=review, width=2, height=2,
            slot="gloves", drops=True, drop_level=drop_level,
            max_item_level=ITEM_LEVEL_CAP, durability=durability,
            classes=classes,
            requirements=[("level", lvl_req),
                          (stat("TotalStrengthRequirementValue"), str_req),
                          (stat("TotalAgilityRequirementValue"), agi_req)],
            power_ups=pups,
            options=["luck", "defense", "excellent_defense"]))

    for a in calls(text, "CreateBoots"):
        if len(a) != 19 or a[0] not in S6_ARMOR_FAMILIES:
            continue
        (number, name, drop_level, defense, walk_speed, durability,
         lvl_req, str_req, agi_req, ene_req, vit_req, lead_req,
         dw, dk, elf, mg, dl, summoner, rf) = a
        assert name.startswith(S6_ARMOR_FAMILIES[number]), name
        assert not (ene_req or vit_req or lead_req), name
        classes, review = s6_armor_classes_review(name, dw, dk, elf, mg, dl,
                                                  summoner, rf)
        pups = armor_common(defense)
        if walk_speed > 0:
            pups.append(power_up(stat("WalkSpeed"), walk_speed, "add_raw"))
        pups.append(power_up(stat("MovementSpeed"), 0, "maximum",
                             "running_movement_speed"))
        records.append(item(
            11, number, name, "s6", review=review, width=2, height=2,
            slot="boots", drops=True, drop_level=drop_level,
            max_item_level=ITEM_LEVEL_CAP, durability=durability,
            classes=classes,
            requirements=[("level", lvl_req),
                          (stat("TotalStrengthRequirementValue"), str_req),
                          (stat("TotalAgilityRequirementValue"), agi_req)],
            power_ups=pups,
            options=["luck", "defense", "excellent_defense"]))

    # 14 pieces: Storm Crow has no helm; the other two families have 5 pieces.
    assert len(records) == 14, sorted(r["name"] for r in records)
    return records


# ---------------------------------------------------------------------------
# wings (group 12) -- Version095d/Items/Wings.cs + s6 backports
# ---------------------------------------------------------------------------

def wing_power_ups(defense, absorb, dmg_increase, dmg_table, speed):
    pups = []
    if defense > 0:
        pups.append(power_up(stat("DefenseBase"), defense, "add_raw", "wing_defense"))
    if absorb > 0:
        pups.append(power_up(stat("DamageReceiveDecrement"), 1 - absorb / 100,
                             "multiplicate", "wing_absorb"))
    if dmg_increase > 0:
        pups.append(power_up(stat("AttackDamageIncrease"), 1 + dmg_increase / 100,
                             "multiplicate", dmg_table))
    pups.append(power_up(stat("CanFly"), 1, "add_raw"))
    pups.append(power_up(stat("MovementSpeed"), speed, "maximum"))
    pups.append(power_up(stat("MovementSpeedUnderwater"), speed, "maximum"))
    return pups


def build_wings():
    records = []

    # Version075/095d Wings.cs: identical three first wings (dmg/absorb 12/12,
    # drop level 100, durability 200, level requirement 180). Class flags per
    # the 095d initializer: DW and DK wings also flag MagicGladiator.
    first_wings = [
        # number, w, h, name, defense, dw, dk, elf
        (0, 3, 2, "Wings of Elf", 10, 0, 0, 1),
        (1, 5, 3, "Wings of Heaven", 10, 1, 0, 0),
        (2, 5, 2, "Wings of Satan", 20, 0, 1, 0),
    ]
    for number, width, height, name, defense, dw, dk, elf in first_wings:
        records.append(item(
            12, number, name, "075", width=width, height=height, slot="wings",
            drops=False, drop_level=100, max_item_level=ITEM_LEVEL_CAP,
            durability=200,
            classes=classes_from_levels(dw=dw, dk=dk, elf=elf,
                                        mg=1 if (dw == 1 or dk == 1) else 0),
            requirements=[("level", 180)],
            power_ups=wing_power_ups(defense, 12, 12, "wing_damage_first", 15),
            options=[common.slugify(name + " Options"), "luck"]))

    # VersionSeasonSix/Items/Wings.cs: curated 1.0-era 2nd wings.
    # S6 values adapted: max_item_level clamped 15 -> 11 (decision 2).
    second_wings = [
        # number, w, h, name, defense, classes, speed, review class note
        (3, 5, 3, "Wings of Spirits", 30, classes_from_levels(elf=2), 15,
         "muse_elf only (source class value 2)"),
        (4, 5, 3, "Wings of Soul", 30, classes_from_levels(dw=2), 15,
         "soul_master only (source class value 2)"),
        (5, 3, 3, "Wings of Dragon", 45, classes_from_levels(dk=2), 16,
         "blade_knight only (source class value 2); fast wing speed 16"),
        (6, 4, 2, "Wings of Darkness", 40, classes_from_levels(mg=1), 15,
         "magic_gladiator (source class value 1)"),
    ]
    for number, width, height, name, defense, classes, speed, class_note in second_wings:
        records.append(item(
            12, number, name, "s6",
            review="1.0-era 2nd wing backported from S6 dataset; "
                   + class_note + "; max_item_level clamped 15->11; "
                   "dmg +32%/abs 25%, +1%/level (wing_damage_second)",
            width=width, height=height, slot="wings",
            drops=False, drop_level=150, max_item_level=ITEM_LEVEL_CAP,
            durability=200,
            classes=classes,
            requirements=[("level", 215)],
            power_ups=wing_power_ups(defense, 25, 32, "wing_damage_second", speed),
            options=["2nd_wing_options", common.slugify(name + " Options"), "luck"]))

    # Loch's Feather rides along: it is defined in the same S6 Wings.cs and is
    # the 2nd-wings chaos-mix ingredient; without it the backport is unusable.
    records.append(item(
        13, 14, "Loch's Feather", "s6",
        review="1.0-era 2nd-wings mix ingredient from S6 Wings.cs; source sets "
               "drops_from_monsters=false and max_item_level=1",
        width=1, height=2, drops=False, drop_level=78, max_item_level=1,
        durability=1))

    # Cape of Lord (group overridden to 13 in the source): the 1.0-era Dark
    # Lord wing, defined in the same S6 Wings.cs. Backported because the
    # cape_of_lord chaos mix (chaos.py) creates it. The source treats capes
    # as hybrids: 2nd-wing-ish base values but the first-wings damage table.
    records.append(item(
        13, 30, "Cape of Lord", "s6",
        review="1.0-era Dark Lord wing backported from S6 Wings.cs "
               "(cape_of_lord chaos-mix result); max_item_level clamped "
               "15->11; dmg +20%/abs 10%, +2%/level (wing_damage_first); "
               "its S6 option definitions (Cape of Lord Options = 2nd-wing "
               "options + Command wing option, per-item random phys dmg) "
               "are S6-only option data and NOT backported -- only luck "
               "referenced, matching the 2nd-wing option gap in options_sets",
        width=2, height=3, slot="wings",
        drops=False, drop_level=180, max_item_level=ITEM_LEVEL_CAP,
        durability=200,
        classes=classes_from_levels(dl=1),
        requirements=[("level", 180)],
        power_ups=wing_power_ups(15, 10, 20, "wing_damage_first", 15),
        options=["luck"]))

    return records


# ---------------------------------------------------------------------------
# pets (group 13) -- Version095d/Items/Pets.cs (angel/imp/uniria also in 075)
# ---------------------------------------------------------------------------

def build_pets():
    pet_classes = classes_from_levels(dw=1, dk=1, elf=1, mg=1)

    def pet(number, name, version, level, drops, pups, skill=None, options=()):
        return item(13, number, name, version, width=1, height=1, slot="pet",
                    drops=drops, drop_level=level, durability=255,
                    skill=skill, classes=pet_classes,
                    requirements=[("level", level)],
                    power_ups=pups, options=options)

    return [
        pet(0, "Guardian Angel", "075", 23, True,
            [power_up(stat("DamageReceiveDecrement"), 0.8, "multiplicate"),
             power_up(stat("MaximumHealth"), 50, "add_raw")]),
        pet(1, "Imp", "075", 28, True,
            [power_up(stat("AttackDamageIncrease"), 1.3, "multiplicate")]),
        pet(2, "Horn of Uniria", "075", 25, True,
            [power_up(stat("MovementSpeed"), 15, "maximum"),
             power_up(stat("MovementSpeedUnderwater"), 15, "maximum")]),
        pet(3, "Horn of Dinorant", "095d", 110, False,
            [power_up(stat("IsDinorantEquipped"), 1, "add_raw"),
             power_up(stat("MovementSpeed"), 15, "maximum"),
             power_up(stat("MovementSpeedUnderwater"), 15, "maximum"),
             power_up(stat("DamageReceiveDecrement"), 0.9, "multiplicate"),
             power_up(stat("AttackDamageIncrease"), 1.15, "multiplicate")],
            skill=equip_skill(SKILL_NUMBER["FireBreath"]),
            options=["dinorant_options"]),
    ]


# ---------------------------------------------------------------------------
# jewelry (group 13 rings/pendants) -- Version075/095d Jewelery.cs
# ---------------------------------------------------------------------------

def build_jewelry():
    health_option = common.slugify("Health recover for jewelery")

    def jewel_item(number, slot, name, level, resistance_stat, excellent):
        return item(13, number, name, "075", width=1, height=1, slot=slot,
                    drops=True, drop_level=level, max_item_level=4,
                    durability=50,
                    classes=ALL_CLASSES,
                    requirements=[("level", level)],
                    power_ups=[power_up(stat(resistance_stat), 1, "maximum",
                                        "jewelry_resistance")],
                    options=[health_option, excellent])

    records = [
        # 095d adds the excellent option references; items themselves are 075
        jewel_item(8, "ring", "Ring of Ice", 20, "IceResistance", "excellent_defense"),
        jewel_item(9, "ring", "Ring of Poison", 17, "PoisonResistance", "excellent_defense"),
        jewel_item(12, "pendant", "Pendant of Lighting", 21, "LightningResistance",
                   "excellent_wizardry"),
        jewel_item(13, "pendant", "Pendant of Fire", 13, "FireResistance",
                   "excellent_physical"),
        # Transformation Ring: level selects the skin (bonus table), no options
        item(13, 10, "Transformation Ring", "075", width=1, height=1,
             slot="ring", drops=False, drop_level=0, max_item_level=5,
             durability=200,
             classes=ALL_CLASSES,
             requirements=[("level", 20)],
             power_ups=[power_up(stat("TransformationSkin"), 0, "add_raw",
                                 "transformation_ring_skins")]),
    ]

    # VersionSeasonSix/Items/Jewelery.cs: 1.0-era elemental rings/pendants,
    # backported because the s6 ancient jewelry sets (item_sets.json) use them
    # as pieces. Same mechanics as the 075 jewelry (durability 50, level
    # requirement = drop level, max_item_level 4, resistance 1+level).
    # Ring of Magic / Pendant of Ability carry an S6-only jewelry option
    # (+1% maximum mana/ability per option level) instead of health recover.
    mana_option = common.slugify("Jewelery option Maximum Mana")
    ability_option = common.slugify("Jewelery option Maximum Ability")

    def s6_jewel(number, slot, name, level, resistance_stat, options, note):
        return item(13, number, name, "s6",
                    review="1.0-era jewelry backported from the S6 dataset "
                           "(piece of the backported ancient jewelry sets); "
                           + note,
                    width=1, height=1, slot=slot, drops=True, drop_level=level,
                    max_item_level=4, durability=50,
                    classes=ALL_CLASSES,
                    requirements=[("level", level)],
                    power_ups=[power_up(stat(resistance_stat), 1, "maximum",
                                        "jewelry_resistance")]
                    if resistance_stat else [],
                    options=options)

    records += [
        s6_jewel(21, "ring", "Ring of Fire", 30, "FireResistance",
                 [health_option, "excellent_defense"], "fire resistance"),
        s6_jewel(22, "ring", "Ring of Earth", 38, "EarthResistance",
                 [health_option, "excellent_defense"], "earth resistance"),
        s6_jewel(23, "ring", "Ring of Wind", 44, "WindResistance",
                 [health_option, "excellent_defense"], "wind resistance"),
        s6_jewel(24, "ring", "Ring of Magic", 47, None,
                 [mana_option, "excellent_defense"],
                 "no resistance; S6-only +1%/level maximum_mana option"),
        s6_jewel(25, "pendant", "Pendant of Ice", 34, "IceResistance",
                 [health_option, "excellent_wizardry"], "ice resistance"),
        s6_jewel(26, "pendant", "Pendant of Wind", 42, "WindResistance",
                 [health_option, "excellent_physical"], "wind resistance"),
        s6_jewel(27, "pendant", "Pendant of Water", 46, "WaterResistance",
                 [health_option, "excellent_wizardry"], "water resistance"),
        s6_jewel(28, "pendant", "Pendant of Ability", 50, None,
                 [ability_option, "excellent_physical"],
                 "no resistance; S6-only +1%/level maximum_ability option"),
    ]
    return records


# ---------------------------------------------------------------------------
# orbs (group 12) and scrolls (group 15) -- parsed from both versions
# ---------------------------------------------------------------------------

ORB_CLASS_FLAGS = {
    "CharacterClasses.FairyElf": classes_from_levels(elf=1),
    "CharacterClasses.DarkKnight | CharacterClasses.MagicGladiator":
        classes_from_levels(dk=1, mg=1),
}


def build_orbs():
    records = []
    for version, path in (("075", SRC + "/Version075/Items/Orbs.cs"),
                          ("095d", SRC + "/Version095d/Items/Orbs.cs")):
        for a in calls(read(path), "CreateOrb"):
            (number, skill_expr, height, name, drop_level, lvl_req, ene_req,
             str_req, agi_req, money, class_expr) = a
            skill_no = SKILL_NUMBER[skill_expr.split(".")[-1]]
            records.append(item(
                12, number, name, version, width=1, height=height,
                drops=True, drop_level=drop_level,
                max_item_level=5 if number == 11 else 0,  # Orb of Summoning
                durability=1, value=money,
                skill=consume_skill(skill_no),
                classes=ORB_CLASS_FLAGS[class_expr],
                requirements=[("level", lvl_req),
                              (stat("TotalEnergy"), ene_req),
                              (stat("TotalStrength"), str_req),
                              (stat("TotalAgility"), agi_req)]))
    return records


def build_scrolls():
    records = []
    scroll_classes = classes_from_levels(dw=1)  # DW only in source
    for version, path in (("075", SRC + "/Version075/Items/Scrolls.cs"),
                          ("095d", SRC + "/Version095d/Items/Scrolls.cs")):
        for a in calls(read(path), "CreateScroll"):
            number, skill_no, name, drop_level, lvl_req, ene_req, money = a
            records.append(item(
                15, number, name, version, width=1, height=2,
                drops=True, drop_level=drop_level, durability=1, value=money,
                skill=consume_skill(skill_no),
                classes=scroll_classes,
                requirements=[("level", lvl_req),
                              (stat("TotalEnergyRequirementValue"), ene_req)]))
    return records


# ---------------------------------------------------------------------------
# jewels, potions/consumables, box of luck, event tickets
# ---------------------------------------------------------------------------

def build_jewels():
    def jewel(group, number, name, version, drop_level, value=0, max_drop=None,
              review=None):
        return item(group, number, name, version, width=1, height=1,
                    drops=False, drop_level=drop_level,
                    max_drop_level=max_drop, durability=1, value=value,
                    review=review)

    return [
        jewel(14, 13, "Jewel of Bless", "075", 25, value=150),
        jewel(14, 14, "Jewel of Soul", "075", 30, value=150),
        jewel(12, 15, "Jewel of Chaos", "075", 12, max_drop=66),
        jewel(14, 16, "Jewel of Life", "095d", 72),
        # Version097d/Items/Jewels.cs -- oldest pre-S3 dataset shipping it,
        # but source_version has no 097d value, so tagged s6 + review.
        jewel(14, 22, "Jewel of Creation", "s6", 72,
              review="0.97d-era jewel (Version097d dataset is the oldest "
                     "pre-S3 source; tagged s6 because source_version has "
                     "no 097d value); backported as the fruits chaos-mix "
                     "ingredient"),
    ]


def build_potions():
    # Version075/Items/Potions.cs, reused verbatim by 095d
    def consumable(number, name, drop_level, durability, value, height=1,
                   consume_effect=None):
        return item(14, number, name, "075", width=1, height=height,
                    drops=True, drop_level=drop_level, durability=durability,
                    value=value, consume_effect=consume_effect)

    records = [
        consumable(0, "Apple", 1, 3, 5),
        consumable(1, "Small Healing Potion", 10, 3, 10),
        consumable(2, "Medium Healing Potion", 25, 3, 20),
        consumable(3, "Large Healing Potion", 40, 3, 30),
        consumable(4, "Small Mana Potion", 10, 3, 10),
        consumable(5, "Medium Mana Potion", 25, 3, 20),
        consumable(6, "Large Mana Potion", 40, 3, 30),
        consumable(8, "Antidote", 10, 3, 10),
        consumable(9, "Ale", 15, 1, 30, height=2, consume_effect="alcohol"),
        consumable(10, "Town Portal Scroll", 30, 1, 30, height=2),
    ]

    # VersionSeasonSix/Items/Potions.cs CreateFruits: 1.0-era stat fruits,
    # backported because the fruits chaos mix (chaos.py) creates them.
    records.append(item(
        13, 15, "Fruits", "s6",
        review="1.0-era stat fruit backported from S6 dataset (fruits "
               "chaos-mix result); item level 0-4 encodes the fruit's stat "
               "kind; consume behavior (stat point add/remove) is a rule, "
               "not data",
        width=1, height=1, drops=False, max_item_level=4, durability=1))

    return records


def build_box_of_luck():
    # Version095d/Items/BoxOfLuck.cs: one drop table for source item level 0
    # (higher box kinds +1..+11 are only named in a source comment, not data).
    singles = [(0, 3), (0, 5), (0, 9), (0, 10), (0, 13),
               (4, 4), (4, 5), (4, 9), (4, 11), (4, 12),
               (5, 0), (5, 2), (5, 3), (5, 4),
               (12, 15), (14, 13), (14, 14)]
    armor_sets = [0, 2, 4, 5, 6, 7, 8, 10, 11, 12]
    drop_items = [common.item_ref(g, n) for g, n in singles]
    drop_items += [common.item_ref(group, number)
                   for number in armor_sets for group in range(7, 12)]
    box_drops = [
        {"source_item_level": 0, "chance": 0.5, "required_character_level": 0,
         "kind": "item_list", "items": drop_items, "level_range": [6, 6]},
        {"source_item_level": 0, "chance": 1.0, "required_character_level": 0,
         "kind": "money", "amount": 10000},
    ]
    return [item(14, 11, "Box of Luck", "095d", width=1, height=1,
                 drops=False, max_item_level=1, durability=1,
                 box_drops=box_drops)]


def build_event_tickets():
    # Version095d/Items/EventTicketItems.cs. The Devil's Eye/Key +1..+4
    # monster-level drop tables are global DropItemGroups -> drop_groups.json.
    def ticket(number, name):
        return item(14, number, name, "095d", width=1, height=1,
                    drops=False, max_item_level=4, durability=1)

    records = [ticket(17, "Devil's Eye"),
               ticket(18, "Devil's Key"),
               ticket(19, "Devil's Invitation")]

    # VersionSeasonSix/Items/EventTicketItems.cs: Blood Castle ticket items,
    # 1.0-era backport riding along with the drop groups (drops.py) and the
    # blood_castle_ticket mix (chaos.py); without them both are unusable.
    # Their +1..+8 monster-level drop tables are drop_groups.json records.
    bc_review = ("Blood Castle ticket item, approved 1.0-era backport from "
                 "S6 dataset; gates 7/8 of max_item_level are arguably "
                 "later-era (7: S1+, 8: S3)")
    records.append(item(
        13, 16, "Scroll of Archangel", "s6", review=bc_review,
        width=1, height=2, drops=False, max_item_level=8, durability=1))
    records.append(item(
        13, 17, "Blood Bone", "s6", review=bc_review,
        width=1, height=2, drops=False, max_item_level=8, durability=1))
    records.append(item(
        13, 18, "Invisibility Cloak", "s6",
        review="Blood Castle entry ticket (blood_castle_ticket mix result), "
               "approved 1.0-era backport from S6 dataset; level gates 7/8 "
               "as above",
        width=2, height=2, drops=False, max_item_level=8, durability=1))

    return records


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------

def main():
    records = []
    records += build_weapons()
    records += build_armors()
    records += build_s6_armors()
    records += build_wings()
    records += build_pets()
    records += build_jewelry()
    records += build_orbs()
    records += build_scrolls()
    records += build_jewels()
    records += build_potions()
    records += build_box_of_luck()
    records += build_event_tickets()

    ids = [(r["id"]["group"], r["id"]["number"]) for r in records]
    assert len(ids) == len(set(ids)), "duplicate item identity"
    records.sort(key=lambda r: (r["id"]["group"], r["id"]["number"]))

    tables = bonus_tables()
    referenced = {p["bonus_table"] for r in records for p in r["base_power_ups"]
                  if "bonus_table" in p}
    defined = {t["id"] for t in tables}
    assert referenced <= defined, f"undefined bonus tables: {referenced - defined}"

    items_path = common.write_datafile("item_definitions.json", records)
    tables_path = common.write_datafile("item_level_bonus_tables.json", tables)

    def by_version(recs):
        out = {}
        for r in recs:
            out[r["source_version"]] = out.get(r["source_version"], 0) + 1
        return out

    reviews = {("%d/%d" % (r["id"]["group"], r["id"]["number"])
                if "values_by_level" not in r else r["id"]): r["review"]
               for r in records + tables if "review" in r}

    coverage_path = common.coverage("items", {
        "files": {
            "item_definitions.json": {
                "records": len(records),
                "by_source_version": by_version(records),
            },
            "item_level_bonus_tables.json": {
                "records": len(tables),
                "by_source_version": by_version(tables),
            },
        },
        "review_count": len(reviews),
        "reviews": reviews,
        "gaps": [
            "Devil's Eye/Key +1..+4 monster-level drop tables (Version095d EventTicketItems.cs) are global drop groups -> drop_groups.json, not representable as box_drops",
            "Weapon of Archangel (13/19, S6 EventTicketItems.cs) is in-event Blood Castle content (saint statue drop); the event/minigame schema is not in this wave -- excluded, unlike the ticket items 13/16-18 which the backported drop groups and mix require",
            "S6-only event items NOT backported: Invisibility Cloak's S6 siblings Armor of Guardsman (13/29, Chaos Castle), Illusion Temple items (13/49-51), Imperial Guardian items (14/101-109) -- events are post-S3 or not in the backport list",
            "Box of Luck higher kinds (+1 Star of the Sacred Birth ... +11 Box of Kundun+4) are named in a 095d source comment but ship no drop data pre-S6; not backported",
            "2nd-wings chaos mix (and its use of Loch's Feather 13/14) -> chaos_mixes.json extractor",
            "Cape of Lord (13/30) is backported (the cape_of_lord chaos mix creates it) but its S6 option definitions (Cape of Lord Options = 2nd-wing options + Command wing option, per-item random phys-dmg options) are S6-only option data and are NOT -- the record references only 'luck'; see the 2nd-wing option gap in options_sets coverage",
            "dark lord scepters not backported in this wave; DL items are Cape of Lord 13/30 (chaos-mix result) and the Adamantine armor pieces (ancient-set dependency), otherwise dark_lord appears only in all-classes qualification lists",
            "summoner class is not in the baseline: the Red Wing pieces (7-11/40, ancient-set dependency) ship with empty classes lists -> unequippable; see their review flags",
            "Wings of Despair (12/42, summoner) and all 3rd wings/capes: post-S3 or excluded classes, skipped",
        ],
        "notes": [
            "merged baseline: record content mirrors the 095d dataset (excellent options, MG qualification, ammo damage table); source_version = oldest dataset shipping the record (075 armor/weapon data lines verified identical between versions)",
            "class expansion: value 1 -> base + evolved stage (evolution backport), value 2 -> evolved only per approved decision; OpenMU's DetermineClass helper would also admit the base class for value 2 -- divergence is intentional and flagged on the s6 wing records",
            "'all classes' items (jewelry, transformation ring) expanded to all 8 baseline slugs including dark_lord; 095d dataset itself only had dw/dk/elf/mg",
            "option slugs referenced (must exist in item_options.json): luck, physical_attack, wizardry_attack, defense, defense_rate, excellent_physical, excellent_wizardry, excellent_defense, health_recover_for_jewelery, jewelery_option_maximum_mana, jewelery_option_maximum_ability, dinorant_options, wings_of_elf_options, wings_of_heaven_options, wings_of_satan_options, 2nd_wing_options, wings_of_spirits_options, wings_of_soul_options, wings_of_dragon_options, wings_of_darkness_options",
            "set-group slugs referenced (must exist in item_sets.json): <prefix>_defense_rate_bonus, <prefix>_defense_bonus_level_10/11 for the 15 armor set prefixes (bronze..guardian); the s6 armor backports (storm crow/adamantine/red wing) carry no possible_set_groups -- BuildSets families cover numbers 0..14 only, their ancient set groups are referenced from the item_sets.json pieces side",
            "consume_effect 'alcohol' (Ale) must exist in magic_effects.json",
            "jewelry_resistance table kept in raw source units (base 1 + level, aggregate maximum); the n/255 fraction convention applies to monster resistances only",
            "bonus tables are dense 12-entry arrays; staff-rise/armor/shield/wing tables truncated from 16 source entries at cap 11, short source tables (ammunition 3, jewelry 5, transformation skins 6) padded with 0 = 'no bonus' per OpenMU sparse-table semantics",
            "skill references by number: weapon/shield skills 18-24 granted_while_equipped, orb/scroll skills taught_on_consume, dinorant fire breath 49 granted_while_equipped",
        ],
    })
    print(items_path)
    print(tables_path)
    print(coverage_path)
    print("items:", len(records), by_version(records))
    print("tables:", len(tables), by_version(tables))


if __name__ == "__main__":
    main()
