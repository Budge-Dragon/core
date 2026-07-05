#!/usr/bin/env python3
"""Extract the eleven 0.97-era merchant shelf catalogs -> data/npc_shops.json.

Source: Version075/MerchantStores.cs (the 0.97-era store data; 0.95d/0.97d add
zero merchant changes), transcribed from the ItemHelper CALLS, never the source
comments (the slot-50 "Mace" comment lies — the code creates Scepters #1).
ItemHelper semantics (Items/ItemHelper.cs): CreatePotion/CreateItem carry the
stack size in durability; CreateScroll/CreateOrb/CreateLearnable fix durability
1; CreateSummonOrb is Orbs #11 with the summon level as item level;
CreateEquippableItem (weapons/set items/shields) takes full definition
durability and links luck / a leveled normal option / skill flags.

One record per merchant NPC number, provenance envelope (source_version "075"),
entries carrying: shelf slot (the classic row*8+col byte), ItemRef, level
(EnhanceLevel, 0..11), and a kind-tagged stock (the W-SHOP ShelfStock wire):
  gear   - wearable: luck/skill rolls + optional pre-applied normal option
  stack  - stackable consumable pack (potions/apple/antidote x1|x3)
  quiver - ammunition: one purchase = one full 255 quiver, NO stack field (K2)
  single - one durability-1 piece (skill scrolls, orbs, Ale, Town Portal
           Scroll — the two durability-1 consumables are singles per spec §9.1)

Two deliberate data fixes over OpenMU, review-noted on the entries:
  D14 - Hanzo: OpenMU creates Gladius AND Falchion at slot 73 (Falchion
        unreachable); Falchion moves to the nearest free anchor fitting its
        1x3 footprint.
  D15 - Hanzo: OpenMU creates Morning Star (Scepters #1) at BOTH +2 and +3;
        the +2 entry becomes the actual Mace (Scepters #0) the stated stock
        list intends.
Kept verbatim, review-noted: the Vine mixed levels (K4) and the two
option-level-2 shields (K6).

The extractor validates the full 8x15 grid per merchant — footprints from the
items generator's own records (items.build_all) — plus stock/kind consistency;
any violation exits nonzero.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import coverage, item_ref, write_datafile

import items

# The grid contract is OURS (spec L6): OpenMU has no server-side bound; 8
# columns is slot arithmetic, 15 rows the classic client window.
COLUMNS = 8
ROWS = 15
MAX_ENHANCE_LEVEL = 11

# OpenMU ItemGroups enum values (Items/ItemGroups.cs), used by the store calls.
SWORDS, AXES, SCEPTERS, SPEARS, BOWS, STAFF, SHIELDS = 0, 1, 2, 3, 4, 5, 6
HELM, ARMOR, PANTS, GLOVES, BOOTS = 7, 8, 9, 10, 11
ORBS, MISC2, SCROLLS = 12, 14, 15

BOLT = item_ref(BOWS, 7)
ARROW = item_ref(BOWS, 15)
TOWN_PORTAL = item_ref(MISC2, 10)
ALE = item_ref(MISC2, 9)

# The normal (Jewel-of-Life) option a store item pre-applies is the target
# definition's own single Option-type option (ItemHelper.CreateEquippableItem
# First(OptionType == Option); Version075 Weapons.cs:305-313 physical/wizardry,
# ArmorInitializerBase.cs:185/405 defense_rate/defense), keyed by our kind tag.
OPTION_BY_KIND = {
    "weapon": "physical_damage",
    "bow": "physical_damage",
    "crossbow": "physical_damage",
    "staff": "wizardry_damage",
    "shield": "defense_rate",
    "helm": "defense",
    "body_armor": "defense",
    "pants": "defense",
    "gloves": "defense",
    "boots": "defense",
}

GEAR_KINDS = set(OPTION_BY_KIND)
AMMO_KINDS = {"arrows", "bolts"}

# (group, number) -> the generated ItemDefinition record (footprint, kind,
# durability) — the same source data/item_definitions.json is written from.
ITEM = {(r["id"]["group"], r["id"]["number"]): r for r in items.build_all()}


def fail(message):
    print("shops.py: " + message, file=sys.stderr)
    sys.exit(1)


def definition(ref, context):
    rec = ITEM.get((ref["group"], ref["number"]))
    if rec is None:
        fail("%s: unknown item %d/%d" % (context, ref["group"], ref["number"]))
    return rec


# ---------------------------------------------------------------------------
# entry builders (one per ShelfStock family)
# ---------------------------------------------------------------------------

def entry(slot, ref, level, stock, review=None, **stock_fields):
    e = {"slot": slot, "item": ref, "level": level, "stock": stock}
    e.update(stock_fields)
    if review:
        e["review"] = review
    return e


def gear(slot, group, number, level, option_level, luck, skill, review=None):
    """A wearable shelf entry mirroring CreateEquippableItem's facts."""
    ref = item_ref(group, number)
    kind = definition(ref, "gear @%d" % slot)["kind"]
    fields = {"luck": "lucky" if luck else "plain",
              "skill": "with_skill" if skill else "no_skill"}
    if option_level > 0:
        fields["option"] = {"option": OPTION_BY_KIND[kind],
                            "level": option_level}
    return entry(slot, ref, level, "gear", review=review, **fields)


def stack(slot, number, pieces):
    """A potion/apple/antidote pack (group 14, always +0 in the era data)."""
    return entry(slot, item_ref(MISC2, number), 0, "stack", pieces=pieces)


def single(slot, ref, level=0, review=None):
    """One durability-1 piece: scroll, orb, Ale, Town Portal Scroll."""
    return entry(slot, ref, level, "single", review=review)


def quiver(slot, ref):
    """A full 255-quiver ammo entry — no stack field (K2)."""
    return entry(slot, ref, 0, "quiver")


def scroll(slot, number):
    return single(slot, item_ref(SCROLLS, number))


def orb(slot, number):
    return single(slot, item_ref(ORBS, number))


def summon_orb(slot, level):
    return single(slot, item_ref(ORBS, 11), level=level)


def armor_set(anchors, set_number, level, review_by_group=None):
    """Five +opt1+Luck set pieces; `anchors` maps armor group -> shelf slot.
    `review_by_group` rides K4 notes on specific pieces; a (level, review)
    tuple in `anchors`' value position overrides the set level (Vine)."""
    out = []
    for group, slot in anchors.items():
        piece_level = level
        if isinstance(slot, tuple):
            slot, piece_level = slot
        review = (review_by_group or {}).get(group)
        out.append(gear(slot, group, set_number, piece_level, 1, True, False,
                        review=review))
    return out


def potion_range_x1_x3():
    """The full potion range x1 (slots 0-7) and x3 (slots 8-15), all +0 —
    shared verbatim by Amy, Elf Lala, and Izabel."""
    numbers = [0, 1, 2, 3, 4, 5, 6, 8]  # Apple, 3x Healing, 3x Mana, Antidote
    return ([stack(slot, number, 1) for slot, number in enumerate(numbers)]
            + [stack(slot + 8, number, 3)
               for slot, number in enumerate(numbers)])


# ---------------------------------------------------------------------------
# the eleven stores (Version075/MerchantStores.cs, code beats comments)
# ---------------------------------------------------------------------------

K4_REVIEW = ("kept verbatim (spec K4): the Vine set ships mixed levels — +0 "
             "helm/armor/pants, +3 gloves/boots (Version075/MerchantStores.cs"
             ":245-249); deliberate quirk or transcription error is "
             "undecidable from source (consult open question 8)")

K6_REVIEW = ("kept verbatim (spec K6): one of exactly two era shields "
             "carrying normal option level 2 — Legendary Shield at Izabel, "
             "Elven Shield at Eo (Version075/MerchantStores.cs:315,356); "
             "every other store option is level 1")

D15_REVIEW = ("our data fix over OpenMU (spec D15): OpenMU creates Scepters "
              "#1 (Morning Star) at BOTH +2 (slot 50, comment claims 'Mace') "
              "and +3 (slot 54) — Version075/MerchantStores.cs:133,137; the "
              "stated stock list is the rule, so this entry ships the actual "
              "Mace (Scepters #0) at +2, no skill per the original comment "
              "intent (the comment carries no +S and the call passes "
              "skill=false)")

D14_REVIEW = ("our data fix over OpenMU (spec D14): OpenMU creates Gladius "
              "AND Falchion both at slot 73 (Version075/MerchantStores.cs"
              ":141-142), leaving the Falchion unreachable behind the "
              "first-match lookup; moved to slot 76 — the nearest free "
              "anchor (row 9, col 4), the first row-major free cell after "
              "the occupied 72-75 sword run, fitting the 1x3 footprint on "
              "rows 9-11")


def elf_lala_242():
    shelf = potion_range_x1_x3()
    shelf += [orb(16, 8), orb(17, 9), orb(18, 10)]
    shelf += [single(21, TOWN_PORTAL), quiver(22, BOLT), quiver(23, ARROW)]
    shelf += [summon_orb(24 + level, level) for level in range(5)]
    shelf += armor_set(
        {HELM: (32, 0), ARMOR: (34, 0), PANTS: (36, 0),
         GLOVES: (38, 3), BOOTS: (48, 3)},
        set_number=10, level=0,
        review_by_group={GLOVES: K4_REVIEW, BOOTS: K4_REVIEW})  # Vine
    shelf += armor_set({HELM: 50, ARMOR: 52, PANTS: 54, GLOVES: 64, BOOTS: 66},
                       set_number=11, level=2)  # Silk
    shelf += armor_set({HELM: 68, ARMOR: 70, PANTS: 80, GLOVES: 82, BOOTS: 84},
                       set_number=12, level=3)  # Wind
    return shelf


def eo_243():
    shelf = armor_set({HELM: 0, ARMOR: 2, PANTS: 4, GLOVES: 6, BOOTS: 16},
                      set_number=13, level=3)  # Spirit
    shelf += armor_set({HELM: 18, ARMOR: 20, PANTS: 22, GLOVES: 32, BOOTS: 34},
                       set_number=14, level=3)  # Guardian
    bows = [(36, 8, 1), (38, 9, 3), (48, 0, 0), (50, 1, 0), (52, 2, 2),
            (54, 3, 3), (72, 11, 3), (74, 4, 3), (76, 10, 3)]
    shelf += [gear(slot, BOWS, number, level, 1, True, True)
              for slot, number, level in bows]
    shelf.append(gear(78, SHIELDS, 3, 3, 2, True, False,
                      review=K6_REVIEW))  # Elven Shield
    return shelf


def barmaid():
    return [single(0, ALE), single(1, TOWN_PORTAL)]


def izabel_245():
    shelf = potion_range_x1_x3()
    shelf += armor_set({HELM: 16, ARMOR: 18, PANTS: 20, GLOVES: 22, BOOTS: 32},
                       set_number=3, level=3)  # Legendary
    shelf += [single(34, TOWN_PORTAL), quiver(35, BOLT), quiver(36, ARROW),
              scroll(37, 4), scroll(38, 7)]  # Flame, Twister
    shelf += [gear(48, STAFF, 4, 3, 1, True, False),   # Gorgon Staff
              gear(50, STAFF, 5, 3, 1, True, False),   # Legendary Staff
              gear(52, SHIELDS, 14, 3, 2, True, False,
                   review=K6_REVIEW)]                  # Legendary Shield
    return shelf


def zienna_246():
    shelf = armor_set({HELM: 0, ARMOR: 2, PANTS: 4, GLOVES: 6, BOOTS: 16},
                      set_number=1, level=3)  # Dragon
    weapons = [(20, SWORDS, 9), (22, SWORDS, 11), (32, SWORDS, 13),
               (33, SWORDS, 14), (26, SWORDS, 15), (44, SWORDS, 12),
               (46, BOWS, 12), (56, SPEARS, 9), (50, SPEARS, 8),
               (68, BOWS, 5), (70, BOWS, 13)]
    shelf += [gear(slot, group, number, 3, 1, True, True)
              for slot, group, number in weapons]
    return shelf


def wandering_merchant():
    """Martin and Harold: the same store builder, distinct NPC identities."""
    shelf = armor_set({HELM: 0, ARMOR: 16, PANTS: 40, BOOTS: 56, GLOVES: 72},
                      set_number=5, level=0)  # Leather
    shelf += armor_set({HELM: 2, ARMOR: 18, PANTS: 34, BOOTS: 50, GLOVES: 66},
                       set_number=0, level=2)  # Bronze
    shelf += armor_set({HELM: 4, ARMOR: 20, PANTS: 36, BOOTS: 52, GLOVES: 68},
                       set_number=6, level=3)  # Scale
    shelf += armor_set({HELM: 6, ARMOR: 22, PANTS: 38, BOOTS: 54, GLOVES: 70},
                       set_number=8, level=3)  # Brass
    shelf += armor_set({HELM: 88, ARMOR: 104, PANTS: 82, BOOTS: 98, GLOVES: 84},
                       set_number=9, level=3)  # Plate
    return shelf


def hanzo_251():
    shields = [(0, 0, 0, False), (2, 4, 1, True), (4, 1, 2, False),
               (6, 2, 3, False), (16, 6, 3, True), (18, 10, 3, True),
               (20, 9, 3, True), (22, 7, 3, True), (32, 5, 3, True),
               (34, 8, 3, True), (36, 11, 3, True), (38, 12, 3, True)]
    shelf = [gear(slot, SHIELDS, number, level, 1, True, skill)
             for slot, number, level, skill in shields]
    weapons = [
        (48, SWORDS, 1, 0, False, None),      # Short Sword
        (49, AXES, 1, 1, False, None),        # Hand Axe
        (50, SCEPTERS, 0, 2, False, D15_REVIEW),  # Mace (fix over Scepters #1)
        (51, SWORDS, 2, 2, False, None),      # Rapier
        (52, AXES, 2, 2, False, None),        # Double Axe
        (53, SWORDS, 4, 3, True, None),       # Sword of Assassin
        (54, SCEPTERS, 1, 3, True, None),     # Morning Star
        (55, AXES, 3, 3, True, None),         # Tomahawk
        (72, SWORDS, 0, 2, False, None),      # Kris
        (73, SWORDS, 6, 3, True, None),       # Gladius
        (76, SWORDS, 7, 3, True, D14_REVIEW),  # Falchion (fix over slot 73)
        (74, SWORDS, 8, 3, False, None),      # Serpent Sword
        (75, SWORDS, 5, 3, True, None),       # Blade
    ]
    shelf += [gear(slot, group, number, level, 1, True, skill, review=review)
              for slot, group, number, level, skill, review in weapons]
    return shelf


def amy_253():
    return potion_range_x1_x3() + [quiver(16, BOLT), quiver(17, ARROW),
                                   single(18, TOWN_PORTAL)]


def pasi_254():
    scrolls = [(0, 3), (1, 10), (2, 2), (3, 1), (4, 5), (5, 6), (6, 0)]
    shelf = [scroll(slot, number) for slot, number in scrolls]
    shelf += armor_set({HELM: 16, ARMOR: 32, PANTS: 48, BOOTS: 64, GLOVES: 80},
                       set_number=2, level=0)  # Pad
    shelf += armor_set({HELM: 18, ARMOR: 34, PANTS: 50, BOOTS: 66, GLOVES: 82},
                       set_number=4, level=2)  # Bone
    shelf += armor_set({HELM: 20, ARMOR: 36, PANTS: 60, BOOTS: 76, GLOVES: 92},
                       set_number=7, level=3)  # Sphinx
    staffs = [(22, 0, 0), (46, 1, 2), (70, 2, 3), (94, 3, 3)]
    shelf += [gear(slot, STAFF, number, level, 1, True, False)
              for slot, number, level in staffs]
    return shelf


STORES = [
    (242, elf_lala_242()),
    (243, eo_243()),
    (244, barmaid()),          # Caren the Barmaid
    (245, izabel_245()),
    (246, zienna_246()),
    (248, wandering_merchant()),  # Martin
    (250, wandering_merchant()),  # Harold
    (251, hanzo_251()),
    (253, amy_253()),
    (254, pasi_254()),
    (255, barmaid()),          # Lumen the Barmaid
]


# ---------------------------------------------------------------------------
# validation: the 8x15 grid geometry + stock/kind consistency
# ---------------------------------------------------------------------------

def validate_entry(npc, e):
    context = "npc %d slot %d" % (npc, e["slot"])
    rec = definition(e["item"], context)
    kind, durability = rec["kind"], rec["durability"]
    if not 0 <= e["level"] <= MAX_ENHANCE_LEVEL:
        fail("%s: level %d outside EnhanceLevel 0..%d"
             % (context, e["level"], MAX_ENHANCE_LEVEL))
    stock = e["stock"]
    if stock == "gear":
        if kind not in GEAR_KINDS:
            fail("%s: gear stock on non-wearable kind %r" % (context, kind))
        option = e.get("option")
        if option and option["option"] != OPTION_BY_KIND[kind]:
            fail("%s: option %r does not match kind %r"
                 % (context, option["option"], kind))
    elif stock == "stack":
        if kind != "consumable" or durability <= 1:
            fail("%s: stack stock needs a stackable consumable, got %r "
                 "durability %d" % (context, kind, durability))
        if not 1 <= e["pieces"] <= durability:
            fail("%s: pieces %d outside 1..%d (the stack cap)"
                 % (context, e["pieces"], durability))
    elif stock == "quiver":
        if kind not in AMMO_KINDS:
            fail("%s: quiver stock on non-ammo kind %r" % (context, kind))
    elif stock == "single":
        if durability != 1:
            fail("%s: single stock on durability-%d definition"
                 % (context, durability))
    else:
        fail("%s: unknown stock %r" % (context, stock))


def validate_grid(npc, shelf):
    occupied = {}  # (row, col) -> anchor slot
    for e in shelf:
        slot = e["slot"]
        rec = definition(e["item"], "npc %d slot %d" % (npc, slot))
        row, col = divmod(slot, COLUMNS)
        width, height = rec["width"], rec["height"]
        if col + width > COLUMNS or row + height > ROWS:
            fail("npc %d slot %d: %dx%d footprint exceeds the %dx%d grid"
                 % (npc, slot, width, height, COLUMNS, ROWS))
        for r in range(row, row + height):
            for c in range(col, col + width):
                if (r, c) in occupied:
                    fail("npc %d: slot %d overlaps slot %d at row %d col %d"
                         % (npc, slot, occupied[(r, c)], r, c))
                occupied[(r, c)] = slot


def validate(records):
    npcs = [r["npc"] for r in records]
    assert npcs == sorted(npcs) and len(npcs) == len(set(npcs)), npcs
    for r in records:
        for e in r["shelf"]:
            validate_entry(r["npc"], e)
        validate_grid(r["npc"], r["shelf"])


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------

def main():
    records = []
    for npc, shelf in STORES:
        shelf = sorted(shelf, key=lambda e: e["slot"])
        records.append({"npc": npc, "source_version": "075", "shelf": shelf})

    assert len(records) == 11, len(records)
    validate(records)
    path = write_datafile("npc_shops.json", records)

    entry_count = sum(len(r["shelf"]) for r in records)
    by_stock = {}
    reviews = {}
    for r in records:
        for e in r["shelf"]:
            by_stock[e["stock"]] = by_stock.get(e["stock"], 0) + 1
            if "review" in e:
                key = "npc %d slot %d" % (r["npc"], e["slot"])
                reviews[key] = e["review"]

    coverage("shops", {
        "records": len(records),
        "entries": entry_count,
        "entries_by_npc": {str(r["npc"]): len(r["shelf"]) for r in records},
        "by_stock": by_stock,
        "review_count": len(reviews),
        "reviews": reviews,
        "data_fixes": {
            "hanzo_falchion_slot": "D14: Falchion (Swords #7) moved from the "
                "duplicated slot 73 to slot 76 (nearest free anchor fitting "
                "its 1x3 footprint); OpenMU's duplicate makes it unreachable",
            "hanzo_mace": "D15: the slot-50 +2 entry ships the actual Mace "
                "(Scepters #0, no skill) instead of OpenMU's second Morning "
                "Star (Scepters #1); the source comment lies",
        },
        "kept_quirks": {
            "vine_mixed_levels": "K4: Vine +0 helm/armor/pants with +3 "
                "gloves/boots, verbatim",
            "option_2_shields": "K6: exactly two shields at option level 2 "
                "(Legendary Shield at Izabel, Elven Shield at Eo), verbatim",
            "ammo_quivers": "K2: ammo entries carry NO stack field; one "
                "purchase = one full 255 quiver (definition durability)",
        },
        "notes": [
            "grid contract 8x15 is OURS (spec L6): OpenMU has no server-side "
            "bound; the extractor proves every footprint in-bounds and "
            "non-overlapping per merchant (footprints from items.build_all, "
            "the same source item_definitions.json is written from)",
            "stock families emitted: gear (wearables: luck/skill rolls + "
            "optional pre-applied normal option), stack (potions/apple/"
            "antidote x1|x3), quiver (ammo, no stack field), single "
            "(skill scrolls, orbs incl. summon orbs +0..+4, Ale, Town "
            "Portal Scroll) — all four ShelfStock variants are reachable",
            "Ale and Town Portal Scroll are durability-1 consumables emitted "
            "as single (spec §9.1 'Ale and Town Portal Scroll single'), not "
            "stack: the merge-eligible stack family stays exactly the "
            "potion/apple/antidote packs (spec §4.1.4)",
            "the pre-applied normal option kind is derived from the "
            "definition's own option family (physical_damage weapons/bows, "
            "wizardry_damage staffs, defense_rate shields, defense armor) — "
            "ItemHelper picks First(OptionType == Option) of the definition",
            "248 Martin and 250 Harold share one store builder with distinct "
            "NPC identities (consult E.1.5); their records are identical by "
            "construction",
            "no names sidecar: shop records carry no display name; merchant "
            "names live in data/names/monster_definitions.json",
        ],
    })

    print(path)
    print("shops: %d merchants, %d entries, by_stock=%s"
          % (len(records), entry_count, by_stock))


if __name__ == "__main__":
    main()
