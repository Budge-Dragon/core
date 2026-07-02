"""Extract data/item_definitions.json (v2 schema).

Sources (OpenMU clone at /tmp/openmu-ref):
- Version095d/Items/{Weapons,Armors,Wings,Pets,Orbs,Scrolls,Jewels,Jewelery,
  BoxOfLuck,EventTicketItems}.cs (+ the Version075 initializers they reuse or
  subclass, used for source_version tagging)
- Version075/Items/Potions.cs (invoked directly by the 095d initializer)
- VersionSeasonSix/Items/{Wings,Armors,Jewelery,Potions,EventTicketItems}.cs
  (curated 1.0-era backports: 2nd wings + Loch's Feather + Cape of Lord, s6
  ancient-set armor/jewelry pieces, stat fruits, Blood Castle tickets)
- Version097d/Items/Jewels.cs (Jewel of Creation, tagged s6 + review)

The numeric facts (damage, defense, drop levels, durability, requirements,
prices) are scraped from the C# CreateXxx(...) data lines; the shape emitted is
the locked v2 `ItemDefinition`: shared authentic columns + a kind-tagged
`ItemKind` flattened into the record. Every killed OpenMU mechanism (PowerUp /
Aggregate vocabulary, stat slugs, bonus-table indirection, possible_options /
possible_set_groups slug lists, box_drops, equip-flag pseudo-stats, the `slot`
field) is gone -- those became Rust rules/services. This extractor emits only
the game's own numeric identities and the authentic per-item facts.
"""

import re

import common

SRC = "/tmp/openmu-ref/src/Persistence/Initialization"


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

ITEM_LEVEL_CAP = 11

# durability-3 potions carry OpenMU's stack-size modeling: an invented value
# preserved verbatim but flagged (classic pre-S3 potions were single items).
POTION_STACK_REVIEW = ("durability 3 is OpenMU stack-size modeling; classic "
                       "pre-S3 potions were single items")


# ---------------------------------------------------------------------------
# class qualification expansion (unchanged from v1)
# ---------------------------------------------------------------------------

CLASS_ORDER = ["dark_wizard", "soul_master", "dark_knight", "blade_knight",
               "fairy_elf", "muse_elf", "magic_gladiator", "dark_lord"]


def classes_from_levels(dw=0, dk=0, elf=0, mg=0, dl=0):
    """OpenMU class-level ints -> snake_case class list (ClassSet wire form).

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


ALL_CLASSES = list(CLASS_ORDER)  # jewelry / transformation ring: every class


# ---------------------------------------------------------------------------
# record + shared-shape builders
# ---------------------------------------------------------------------------

def item(group, number, name, version, *, width, height, drops=False,
         drop_level=0, max_item_level=0, durability, value=0, kind,
         review=None):
    """Assemble one ItemDefinition: shared columns + flattened ItemKind."""
    rec = {"id": {"group": int(group), "number": int(number)},
           "name": name,
           "source_version": version}
    if review:
        rec["review"] = review
    rec["width"] = width
    rec["height"] = height
    rec["drops_from_monsters"] = drops
    rec["drop_level"] = drop_level
    rec["max_item_level"] = max_item_level
    rec["durability"] = durability
    rec["price"] = ({"kind": "fixed", "zen": value} if value > 0
                    else {"kind": "formula"})
    rec.update(kind)  # flatten kind tag + variant fields inline
    return rec


def wear(level=0, strength=0, agility=0, vitality=0, energy=0, command=0):
    """WearRequirements: raw Item.txt columns (0 = no requirement)."""
    return {"level": level, "strength": strength, "agility": agility,
            "vitality": vitality, "energy": energy, "command": command}


def learn(level=0, strength=0, agility=0, energy=0):
    """LearnRequirements: absolute consumption minima (no vitality column)."""
    return {"level": level, "strength": strength, "agility": agility,
            "energy": energy}


def add_skill(kind, skill_no):
    """Attach an optional equipped-skill SkillNumber (omitted when absent)."""
    if skill_no and skill_no > 0:
        kind["skill"] = skill_no


# ---------------------------------------------------------------------------
# weapons (groups 0-5): weapon / bow / crossbow / staff / arrows / bolts
# ---------------------------------------------------------------------------

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
        version = "075" if (group, number) in in_075 else "095d"
        classes = classes_from_levels(
            dw=wizard, dk=knight, elf=elf,
            mg=1 if (wizard == 1 or knight == 1 or mg == 1) else 0)
        worn = wear(level=lvl_req, strength=str_req, agility=agi_req,
                    vitality=vit_req, energy=ene_req)

        if group == 5:
            kind = {"kind": "staff", "min_damage": min_dmg,
                    "max_damage": max_dmg, "attack_speed": speed,
                    "magic_power": magic_power}
        elif group == 4:
            kind = {"kind": "bow" if number <= 6 else "crossbow",
                    "min_damage": min_dmg, "max_damage": max_dmg,
                    "attack_speed": speed}
        else:  # groups 0-3: melee. Two-handed weapons occupy 2 inventory cols.
            kind = {"kind": "weapon",
                    "handling": "two_handed" if width == 2 else "one_handed",
                    "min_damage": min_dmg, "max_damage": max_dmg,
                    "attack_speed": speed}
        add_skill(kind, skill_no)
        kind["classes"] = classes
        kind["wear"] = worn

        records.append(item(
            group, number, name, version, width=width, height=height,
            drops=drops, drop_level=drop_level, max_item_level=ITEM_LEVEL_CAP,
            durability=durability, kind=kind))

    for a in calls(text_095d, "CreateAmmunition"):
        (group, number, slot, width, height, drops, name, drop_level,
         durability, wizard, knight, elf) = a
        version = "075" if (group, number) in in_075 else "095d"
        kind = {"kind": "arrows" if number == 15 else "bolts",
                "classes": classes_from_levels(dw=wizard, dk=knight, elf=elf)}
        records.append(item(
            group, number, name, version, width=width, height=height,
            drops=drops, drop_level=drop_level, max_item_level=0,
            durability=durability, kind=kind))
    return records


# ---------------------------------------------------------------------------
# armors + shields (groups 6-11)
# ---------------------------------------------------------------------------

# CreateArmor slot column (2/3/4) -> group -> kind tag.
ARMOR_KIND = {7: "helm", 8: "body_armor", 9: "pants"}


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
        kind = {"kind": "shield", "defense": defense,
                "defense_rate": defense_rate}
        add_skill(kind, skill_no)
        kind["classes"] = classes_from_levels(dw=dw, dk=dk, elf=elf)
        kind["wear"] = wear(strength=str_req, agility=agi_req)
        records.append(item(
            6, number, name, "075", width=width, height=height, drops=True,
            drop_level=drop_level, max_item_level=ITEM_LEVEL_CAP,
            durability=durability, kind=kind))

    for a in calls(text_095d, "CreateArmor"):
        (number, slot, width, height, name, drop_level, defense, durability,
         str_req, agi_req, dw, dk, elf) = a
        group = slot + 5
        # short CreateArmor overload: MG auto-qualifies on non-helm DW/DK gear
        mg = 1 if group != 7 and (dw == 1 or dk == 1) else 0
        kind = {"kind": ARMOR_KIND[group], "defense": defense,
                "classes": classes_from_levels(dw=dw, dk=dk, elf=elf, mg=mg),
                "wear": wear(strength=str_req, agility=agi_req)}
        records.append(item(
            group, number, name, "075", width=width, height=height, drops=True,
            drop_level=drop_level, max_item_level=ITEM_LEVEL_CAP,
            durability=durability, kind=kind))

    for a in calls(text_095d, "CreateGloves"):
        (number, name, drop_level, defense, attack_speed, durability,
         str_req, agi_req, dw, dk, elf) = a
        kind = {"kind": "gloves", "defense": defense,
                "attack_speed": attack_speed,
                "classes": classes_from_levels(
                    dw=dw, dk=dk, elf=elf, mg=1 if (dw == 1 or dk == 1) else 0),
                "wear": wear(strength=str_req, agility=agi_req)}
        records.append(item(
            10, number, name, "075", width=2, height=2, drops=True,
            drop_level=drop_level, max_item_level=ITEM_LEVEL_CAP,
            durability=durability, kind=kind))

    for a in calls(text_095d, "CreateBoots"):
        (number, slot, width, height, name, drop_level, defense, walk_speed,
         durability, str_req, agi_req, dw, dk, elf) = a
        kind = {"kind": "boots", "defense": defense,
                "classes": classes_from_levels(
                    dw=dw, dk=dk, elf=elf, mg=1 if (dw == 1 or dk == 1) else 0),
                "wear": wear(strength=str_req, agility=agi_req)}
        records.append(item(
            11, number, name, "075", width=2, height=2, drops=True,
            drop_level=drop_level, max_item_level=ITEM_LEVEL_CAP,
            durability=durability, kind=kind))

    return records


# ---------------------------------------------------------------------------
# s6 armor backports -- ancient-set pieces (VersionSeasonSix/Items/Armors.cs)
# ---------------------------------------------------------------------------

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
    requirement columns and class columns)."""
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
        group = slot + 5
        kind = {"kind": ARMOR_KIND[group], "defense": defense,
                "classes": classes,
                "wear": wear(level=lvl_req, strength=str_req, agility=agi_req)}
        records.append(item(
            group, number, name, "s6", review=review, width=width,
            height=height, drops=True, drop_level=drop_level,
            max_item_level=ITEM_LEVEL_CAP, durability=durability, kind=kind))

    for a in calls(text, "CreateGloves"):
        if len(a) != 15 or a[0] not in S6_ARMOR_FAMILIES:
            continue
        (number, name, drop_level, defense, attack_speed, durability,
         lvl_req, str_req, agi_req, dw, dk, elf, mg, dl, summoner) = a
        assert name.startswith(S6_ARMOR_FAMILIES[number]), name
        classes, review = s6_armor_classes_review(name, dw, dk, elf, mg, dl,
                                                  summoner, 0)
        kind = {"kind": "gloves", "defense": defense,
                "attack_speed": attack_speed, "classes": classes,
                "wear": wear(level=lvl_req, strength=str_req, agility=agi_req)}
        records.append(item(
            10, number, name, "s6", review=review, width=2, height=2,
            drops=True, drop_level=drop_level, max_item_level=ITEM_LEVEL_CAP,
            durability=durability, kind=kind))

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
        kind = {"kind": "boots", "defense": defense, "classes": classes,
                "wear": wear(level=lvl_req, strength=str_req, agility=agi_req)}
        records.append(item(
            11, number, name, "s6", review=review, width=2, height=2,
            drops=True, drop_level=drop_level, max_item_level=ITEM_LEVEL_CAP,
            durability=durability, kind=kind))

    # 14 pieces: Storm Crow has no helm; the other two families have 5 pieces.
    assert len(records) == 14, sorted(r["name"] for r in records)
    return records


# ---------------------------------------------------------------------------
# wings (group 12 numbers 0-6 + Cape of Lord 13/30)
# ---------------------------------------------------------------------------

def build_wings():
    records = []

    # Version075/095d Wings.cs first wings (JoL options from BuildOptions).
    first_wings = [
        # number, w, h, name, defense, jol_options, classes
        (0, 3, 2, "Wings of Elf", 10, ["health_recovery_pct"],
         classes_from_levels(elf=1)),
        (1, 5, 3, "Wings of Heaven", 10, ["wizardry_damage"],
         classes_from_levels(dw=1, mg=1)),
        (2, 5, 2, "Wings of Satan", 20, ["physical_damage"],
         classes_from_levels(dk=1, mg=1)),
    ]
    for number, width, height, name, defense, jol, classes in first_wings:
        kind = {"kind": "wings", "tier": "first", "defense": defense,
                "absorb_percent": 12, "damage_percent": 12,
                "jol_options": jol, "classes": classes,
                "wear": wear(level=180)}
        records.append(item(
            12, number, name, "075", width=width, height=height, drops=False,
            drop_level=100, max_item_level=ITEM_LEVEL_CAP, durability=200,
            kind=kind))

    # VersionSeasonSix/Items/Wings.cs curated 1.0-era 2nd wings.
    second_wings = [
        # number, w, h, name, defense, jol_options, classes, class_note
        (3, 5, 3, "Wings of Spirits", 30,
         ["health_recovery_pct", "physical_damage"], classes_from_levels(elf=2),
         "muse_elf only (source class value 2)"),
        (4, 5, 3, "Wings of Soul", 30,
         ["health_recovery_pct", "wizardry_damage"], classes_from_levels(dw=2),
         "soul_master only (source class value 2)"),
        (5, 3, 3, "Wings of Dragon", 45,
         ["health_recovery_pct", "physical_damage"], classes_from_levels(dk=2),
         "blade_knight only (source class value 2); fast wing speed 16"),
        (6, 4, 2, "Wings of Darkness", 40,
         ["wizardry_damage", "physical_damage"], classes_from_levels(mg=1),
         "magic_gladiator (source class value 1)"),
    ]
    for number, width, height, name, defense, jol, classes, note in second_wings:
        kind = {"kind": "wings", "tier": "second", "defense": defense,
                "absorb_percent": 25, "damage_percent": 32,
                "jol_options": jol, "classes": classes,
                "wear": wear(level=215)}
        records.append(item(
            12, number, name, "s6",
            review="1.0-era 2nd wing backported from S6 dataset; " + note
                   + "; max_item_level clamped 15->11; "
                   "dmg +32%/abs 25%, +1%/level (wing_damage_second)",
            width=width, height=height, drops=False, drop_level=150,
            max_item_level=ITEM_LEVEL_CAP, durability=200, kind=kind))

    # Cape of Lord (group overridden to 13 in the source): 1.0-era Dark Lord
    # wing. First-wings damage table (+20/abs10, +2%/level). Its S6 option
    # definitions (Cape of Lord Options) are NOT backported -> jol_options [].
    cape_kind = {"kind": "wings", "tier": "first", "defense": 15,
                 "absorb_percent": 10, "damage_percent": 20,
                 "jol_options": [], "classes": classes_from_levels(dl=1),
                 "wear": wear(level=180)}
    records.append(item(
        13, 30, "Cape of Lord", "s6",
        review="1.0-era Dark Lord wing backported from S6 Wings.cs "
               "(cape_of_lord chaos-mix result); max_item_level clamped "
               "15->11; dmg +20%/abs 10%, +2%/level (wing_damage_first); "
               "its S6 option definitions (Cape of Lord Options = 2nd-wing "
               "options + Command wing option, per-item random phys dmg) "
               "are S6-only option data and NOT backported -- only luck "
               "referenced, matching the 2nd-wing option gap in options_sets",
        width=2, height=3, drops=False, drop_level=180,
        max_item_level=ITEM_LEVEL_CAP, durability=200, kind=cape_kind))

    return records


# ---------------------------------------------------------------------------
# pets (group 13 numbers 0-3)
# ---------------------------------------------------------------------------

def build_pets():
    pet_classes = classes_from_levels(dw=1, dk=1, elf=1, mg=1)  # seven classes

    def pet(number, name, version, level, drops, ride, bonuses, skill=None):
        kind = {"kind": "pet", "ride": ride, "bonuses": bonuses}
        add_skill(kind, skill)
        kind["classes"] = pet_classes
        kind["wear"] = wear(level=level)
        return item(13, number, name, version, width=1, height=1, drops=drops,
                    drop_level=level, max_item_level=0, durability=255,
                    kind=kind)

    return [
        # DamageReceiveDecrement 0.8 -> -20% incoming; MaximumHealth +50.
        pet(0, "Guardian Angel", "075", 23, True, "not_rideable",
            [{"kind": "incoming_damage_pct", "percent": 20},
             {"kind": "max_health", "amount": 50}]),
        # AttackDamageIncrease 1.3 -> +30% damage.
        pet(1, "Imp", "075", 28, True, "not_rideable",
            [{"kind": "damage_pct", "percent": 30}]),
        # Horn of Uniria: a ground mount; its only power-up was movement speed
        # (deleted) -> no combat bonuses.
        pet(2, "Horn of Uniria", "075", 25, True, "ground_mount", []),
        # Horn of Dinorant: flying mount; -10% incoming, +15% damage, FireBreath.
        pet(3, "Horn of Dinorant", "095d", 110, False, "flying_mount",
            [{"kind": "incoming_damage_pct", "percent": 10},
             {"kind": "damage_pct", "percent": 15}],
            skill=SKILL_NUMBER["FireBreath"]),
    ]


# ---------------------------------------------------------------------------
# jewelry: rings + pendants + transformation ring (group 13)
# ---------------------------------------------------------------------------

# v1 excellent option slug -> ExcellentCategory object (pendants roll weapon).
EXCELLENT = {
    "wizardry": {"set": "weapon", "damage": "wizardry"},
    "physical": {"set": "weapon", "damage": "physical"},
}


def build_jewelry():
    records = []

    def ring(number, name, version, level, resistance, option, review=None):
        kind = {"kind": "ring"}
        if resistance:
            kind["resistance"] = resistance
        kind["option"] = option
        kind["classes"] = ALL_CLASSES
        kind["wear"] = wear(level=level)
        return item(13, number, name, version, review=review, width=1, height=1,
                    drops=True, drop_level=level, max_item_level=4,
                    durability=50, kind=kind)

    def pendant(number, name, version, level, resistance, option, excellent,
                review=None):
        kind = {"kind": "pendant"}
        if resistance:
            kind["resistance"] = resistance
        kind["option"] = option
        kind["excellent"] = EXCELLENT[excellent]
        kind["classes"] = ALL_CLASSES
        kind["wear"] = wear(level=level)
        return item(13, number, name, version, review=review, width=1, height=1,
                    drops=True, drop_level=level, max_item_level=4,
                    durability=50, kind=kind)

    def s6_review(note):
        return ("1.0-era jewelry backported from the S6 dataset (piece of the "
                "backported ancient jewelry sets); " + note)

    records += [
        # 075 elemental jewelry (095d adds the excellent option references).
        ring(8, "Ring of Ice", "075", 20, "ice", "health_recovery_pct"),
        ring(9, "Ring of Poison", "075", 17, "poison", "health_recovery_pct"),
        pendant(12, "Pendant of Lighting", "075", 21, "lightning",
                "health_recovery_pct", "wizardry"),
        pendant(13, "Pendant of Fire", "075", 13, "fire",
                "health_recovery_pct", "physical"),
        # Transformation Ring: item level 0..5 selects the monster skin.
        item(13, 10, "Transformation Ring", "075", width=1, height=1,
             drops=False, drop_level=0, max_item_level=5, durability=200,
             kind={"kind": "transformation_ring",
                   "skins": [2, 7, 14, 8, 9, 41],
                   "classes": ALL_CLASSES, "wear": wear(level=20)}),
        # VersionSeasonSix/Items/Jewelery.cs 1.0-era jewelry (ancient-set pieces).
        ring(21, "Ring of Fire", "s6", 30, "fire", "health_recovery_pct",
             s6_review("fire resistance")),
        ring(22, "Ring of Earth", "s6", 38, "earth", "health_recovery_pct",
             s6_review("earth resistance")),
        ring(23, "Ring of Wind", "s6", 44, "wind", "health_recovery_pct",
             s6_review("wind resistance")),
        ring(24, "Ring of Magic", "s6", 47, None, "max_mana_pct",
             s6_review("no resistance; S6-only +1%/level maximum_mana option")),
        pendant(25, "Pendant of Ice", "s6", 34, "ice", "health_recovery_pct",
                "wizardry", s6_review("ice resistance")),
        pendant(26, "Pendant of Wind", "s6", 42, "wind", "health_recovery_pct",
                "physical", s6_review("wind resistance")),
        pendant(27, "Pendant of Water", "s6", 46, "water",
                "health_recovery_pct", "wizardry", s6_review("water resistance")),
        pendant(28, "Pendant of Ability", "s6", 50, None, "max_ability_pct",
                "physical",
                s6_review("no resistance; S6-only +1%/level maximum_ability "
                          "option")),
    ]
    return records


# ---------------------------------------------------------------------------
# orbs (group 12) and scrolls (group 15) -- teach a skill on consumption
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
            teaches = SKILL_NUMBER[skill_expr.split(".")[-1]]
            kind = {"kind": "orb", "teaches": teaches,
                    "learn": learn(level=lvl_req, strength=str_req,
                                   agility=agi_req, energy=ene_req),
                    "classes": ORB_CLASS_FLAGS[class_expr]}
            records.append(item(
                12, number, name, version, width=1, height=height, drops=True,
                drop_level=drop_level,
                max_item_level=5 if number == 11 else 0,  # Orb of Summoning
                durability=1, value=money, kind=kind))
    return records


def build_scrolls():
    records = []
    scroll_classes = classes_from_levels(dw=1)  # DW line only in source
    for version, path in (("075", SRC + "/Version075/Items/Scrolls.cs"),
                          ("095d", SRC + "/Version095d/Items/Scrolls.cs")):
        for a in calls(read(path), "CreateScroll"):
            number, skill_no, name, drop_level, lvl_req, ene_req, money = a
            kind = {"kind": "skill_scroll", "teaches": skill_no,
                    "learn": learn(level=lvl_req, energy=ene_req),
                    "classes": scroll_classes}
            records.append(item(
                15, number, name, version, width=1, height=2, drops=True,
                drop_level=drop_level, max_item_level=0, durability=1,
                value=money, kind=kind))
    return records


# ---------------------------------------------------------------------------
# jewels, consumables, box of luck, event tickets / mix materials, fruit
# ---------------------------------------------------------------------------

def build_jewels():
    def jewel(group, number, name, version, drop_level, jewel_kind, value=0,
              review=None):
        return item(group, number, name, version, width=1, height=1,
                    drops=False, drop_level=drop_level, max_item_level=0,
                    durability=1, value=value, review=review,
                    kind={"kind": "jewel", "jewel": jewel_kind})

    return [
        jewel(14, 13, "Jewel of Bless", "075", 25, "bless", value=150),
        jewel(14, 14, "Jewel of Soul", "075", 30, "soul", value=150),
        jewel(12, 15, "Jewel of Chaos", "075", 12, "chaos"),
        jewel(14, 16, "Jewel of Life", "095d", 72, "life"),
        # Version097d/Items/Jewels.cs -- oldest pre-S3 dataset shipping it, but
        # source_version has no 097d value, so tagged s6 + review.
        jewel(14, 22, "Jewel of Creation", "s6", 72, "creation",
              review="0.97d-era jewel (Version097d dataset is the oldest "
                     "pre-S3 source; tagged s6 because source_version has "
                     "no 097d value); backported as the fruits chaos-mix "
                     "ingredient"),
    ]


def build_consumables():
    # Version075/Items/Potions.cs, reused verbatim by 095d. Effect kind carries
    # the strength tier; magnitudes are a services rule.
    potions = [
        # number, name, drop_level, durability, value, height, effect
        (0, "Apple", 1, 3, 5, 1, {"kind": "healing", "tier": "apple"}),
        (1, "Small Healing Potion", 10, 3, 10, 1,
         {"kind": "healing", "tier": "small"}),
        (2, "Medium Healing Potion", 25, 3, 20, 1,
         {"kind": "healing", "tier": "medium"}),
        (3, "Large Healing Potion", 40, 3, 30, 1,
         {"kind": "healing", "tier": "large"}),
        (4, "Small Mana Potion", 10, 3, 10, 1,
         {"kind": "mana", "tier": "small"}),
        (5, "Medium Mana Potion", 25, 3, 20, 1,
         {"kind": "mana", "tier": "medium"}),
        (6, "Large Mana Potion", 40, 3, 30, 1,
         {"kind": "mana", "tier": "large"}),
        (8, "Antidote", 10, 3, 10, 1, {"kind": "antidote"}),
        (9, "Ale", 15, 1, 30, 2, {"kind": "alcohol"}),
        (10, "Town Portal Scroll", 30, 1, 30, 2, {"kind": "town_portal"}),
    ]
    records = []
    for number, name, drop_level, durability, value, height, effect in potions:
        review = POTION_STACK_REVIEW if durability == 3 else None
        records.append(item(
            14, number, name, "075", review=review, width=1, height=height,
            drops=True, drop_level=drop_level, max_item_level=0,
            durability=durability, value=value,
            kind={"kind": "consumable", "effect": effect}))
    return records


def build_box_of_luck():
    # Version095d/Items/BoxOfLuck.cs. Contents live in drops (box_drops.json);
    # items contributes only the bare lucky_box kind tag.
    return [item(14, 11, "Box of Luck", "095d", width=1, height=1, drops=False,
                 drop_level=0, max_item_level=1, durability=1,
                 kind={"kind": "lucky_box"})]


def build_tickets_and_materials():
    """Event-entry tickets, inert chaos-machine ingredients, stat fruit."""
    records = []

    def mix_material(group, number, name, version, *, width, height,
                     drop_level=0, max_item_level, review=None):
        return item(group, number, name, version, review=review, width=width,
                    height=height, drops=False, drop_level=drop_level,
                    max_item_level=max_item_level, durability=1,
                    kind={"kind": "mix_material"})

    def event_ticket(group, number, name, version, *, width, height,
                     event, max_item_level, review=None):
        return item(group, number, name, version, review=review, width=width,
                    height=height, drops=False, drop_level=0,
                    max_item_level=max_item_level, durability=1,
                    kind={"kind": "event_ticket", "event": event})

    # Version095d/Items/EventTicketItems.cs (Devil Square).
    records.append(mix_material(14, 17, "Devil's Eye", "095d",
                                width=1, height=1, max_item_level=4))
    records.append(mix_material(14, 18, "Devil's Key", "095d",
                                width=1, height=1, max_item_level=4))
    records.append(event_ticket(14, 19, "Devil's Invitation", "095d",
                                 width=1, height=1, event="devil_square",
                                 max_item_level=4))

    # VersionSeasonSix Blood Castle ticket items + entry ticket.
    bc_review = ("Blood Castle ticket item, approved 1.0-era backport from "
                 "S6 dataset; gates 7/8 of max_item_level are arguably "
                 "later-era (7: S1+, 8: S3)")
    records.append(mix_material(13, 16, "Scroll of Archangel", "s6",
                                width=1, height=2, max_item_level=8,
                                review=bc_review))
    records.append(mix_material(13, 17, "Blood Bone", "s6",
                                width=1, height=2, max_item_level=8,
                                review=bc_review))
    records.append(event_ticket(
        13, 18, "Invisibility Cloak", "s6", width=2, height=2,
        event="blood_castle", max_item_level=8,
        review="Blood Castle entry ticket (blood_castle_ticket mix result), "
               "approved 1.0-era backport from S6 dataset; level gates 7/8 "
               "as above"))

    # Loch's Feather: 2nd-wings mix ingredient (S6 Wings.cs).
    records.append(mix_material(
        13, 14, "Loch's Feather", "s6", width=1, height=2, drop_level=78,
        max_item_level=1,
        review="1.0-era 2nd-wings mix ingredient from S6 Wings.cs; source sets "
               "drops_from_monsters=false and max_item_level=1"))

    # Stat fruit (S6 Potions.cs CreateFruits): item level 0-4 encodes the stat.
    records.append(item(
        13, 15, "Fruits", "s6",
        review="1.0-era stat fruit backported from S6 dataset (fruits "
               "chaos-mix result); item level 0-4 encodes the fruit's stat "
               "kind; consume behavior (stat point add/remove) is a rule, "
               "not data",
        width=1, height=1, drops=False, drop_level=0, max_item_level=4,
        durability=1, kind={"kind": "stat_fruit"}))

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
    records += build_consumables()
    records += build_box_of_luck()
    records += build_tickets_and_materials()

    ids = [(r["id"]["group"], r["id"]["number"]) for r in records]
    assert len(ids) == len(set(ids)), "duplicate item identity"
    records.sort(key=lambda r: (r["id"]["group"], r["id"]["number"]))

    items_path = common.write_datafile("item_definitions.json", records)

    def by_version(recs):
        out = {}
        for r in recs:
            out[r["source_version"]] = out.get(r["source_version"], 0) + 1
        return out

    def by_kind(recs):
        out = {}
        for r in recs:
            out[r["kind"]] = out.get(r["kind"], 0) + 1
        return out

    reviews = {"%d/%d" % (r["id"]["group"], r["id"]["number"]): r["review"]
               for r in records if "review" in r}

    coverage_path = common.coverage("items", {
        "files": {
            "item_definitions.json": {
                "records": len(records),
                "by_source_version": by_version(records),
                "by_kind": by_kind(records),
            },
        },
        "review_count": len(reviews),
        "reviews": reviews,
        "gaps": [
            "Devil's Eye/Key +1..+4 monster-level drop tables (Version095d EventTicketItems.cs) are global drop groups -> drops domain, not carried on the item record",
            "Weapon of Archangel (13/19, S6 EventTicketItems.cs) is in-event Blood Castle content (saint statue drop); the event/minigame schema is not in this wave -- excluded, unlike the ticket items 13/16-18 which the backported drop groups and mix require",
            "S6-only event items NOT backported: Invisibility Cloak's S6 siblings Armor of Guardsman (13/29, Chaos Castle), Illusion Temple items (13/49-51), Imperial Guardian items (14/101-109) -- events are post-S3 or not in the backport list",
            "Box of Luck higher kinds (+1 Star of the Sacred Birth ... +11 Box of Kundun+4) are named in a 095d source comment but ship no drop data pre-S6; not backported",
            "2nd-wings chaos mix (and its use of Loch's Feather 13/14) -> chaos_mixes.json extractor",
            "Cape of Lord (13/30) is backported (the cape_of_lord chaos mix creates it) but its S6 option definitions (Cape of Lord Options = 2nd-wing options + Command wing option, per-item random phys-dmg options) are S6-only option data and are NOT -- jol_options is empty; see the 2nd-wing option gap in options_sets coverage",
            "dark lord scepters not backported in this wave; DL items are Cape of Lord 13/30 (chaos-mix result) and the Adamantine armor pieces (ancient-set dependency), otherwise dark_lord appears only in all-classes qualification lists",
            "summoner class is not in the baseline: the Red Wing pieces (7-11/40, ancient-set dependency) ship with empty classes lists -> unequippable; see their review flags",
            "Wings of Despair (12/42, summoner) and all 3rd wings/capes: post-S3 or excluded classes, skipped",
        ],
        "notes": [
            "v2 shape: each record is an ItemDefinition -- shared authentic columns + a kind-tagged ItemKind flattened inline. No stat slugs, no PowerUp/Aggregate vocabulary, no bonus-table indirection, no possible_options/possible_set_groups slug lists, no box_drops, no equip-flag pseudo-stats, no slot field: all became Rust rules/services",
            "item_level_bonus_tables.json is DROPPED (the +level curves are services const tables keyed by the EnhanceLevel enum)",
            "merged baseline: record content mirrors the 095d dataset (MG qualification, ammo kinds); source_version = oldest dataset shipping the record (075 armor/weapon data lines verified identical between versions)",
            "class expansion (ClassSet wire = snake_case class list): value 1 -> base + evolved stage (evolution backport), value 2 -> evolved only per approved decision; OpenMU's DetermineClass helper would also admit the base class for value 2 -- divergence flagged on the s6 wing records",
            "'all classes' items (jewelry, transformation ring) expanded to all 8 baseline slugs including dark_lord; 095d dataset itself only had dw/dk/elf/mg",
            "kind is data (set by extraction), never derived from group: group 12 = wings(0-6)+orbs(7-11)+jewel(15); group 13 = pets+jewelry+wings(cape)+tickets+materials+fruit; group 14 = potions+jewels+box+tickets; group 15 = scrolls",
            "weapon handling: two_handed when the melee weapon occupies 2 inventory columns (width==2, groups 0-3), else one_handed; bows/crossbows/staffs carry no handling",
            "group 4 split: bow = numbers 0-6, crossbow = 8-16 (CreateWeapon), bolts = 4/7, arrows = 4/15 (CreateAmmunition)",
            "wing jol_options are the BuildOptions normal (Jewel-of-Life) option kinds as NormalOption: Elf health_recovery_pct, Heaven wizardry_damage, Satan physical_damage, Spirits [health_recovery_pct, physical_damage], Soul [health_recovery_pct, wizardry_damage], Dragon [health_recovery_pct, physical_damage], Darkness [wizardry_damage, physical_damage]; Cape of Lord [] (S6 option data not backported)",
            "pendant excellent is the ExcellentCategory object {set:weapon, damage:physical|wizardry} (Fire/Wind/Ability -> physical, Lighting/Ice/Water -> wizardry); pets serialize CombatBonus inline (absorb -> incoming_damage_pct, attack increase -> damage_pct, health -> max_health); movement-speed power-ups deleted -> Horn of Uniria carries no bonuses (ground_mount) and the mount fact rides PetRide",
            "requirements split: equipment carries WearRequirements (raw Item.txt columns, scaled at equip by services), orbs/scrolls carry LearnRequirements (absolute consumption minima, no vitality column)",
            "price is kind-tagged: source value>0 -> {fixed, zen}, 0 -> {formula}; equipment/jewels(Chaos/Life/Creation)/box/tickets/fruit are formula, orbs/scrolls/potions/(Bless/Soul) are fixed",
            "potion durability=3 is OpenMU stack-size modeling (invented value): the 8 durability-3 consumables (Apple, healing x3, mana x3, Antidote) carry the review flag; Ale/Town Portal (durability 1) do not; stacking itself is a W-ENT inventory rule",
            "transformation ring skins [2, 7, 14, 8, 9, 41] are MonsterNumbers (Atlas-proven); weapon/shield/pet skills and orb/scroll teaches are SkillNumbers (Atlas-proven)",
        ],
    })

    print(items_path)
    print(coverage_path)
    print("items:", len(records), by_version(records))
    print("kinds:", by_kind(records))


if __name__ == "__main__":
    main()
