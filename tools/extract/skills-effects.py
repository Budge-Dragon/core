#!/usr/bin/env python3
"""Extract the pre-S3 skill roster to data/skills.json (v2 schema).

v2 shape (locked by the R2 Rust serde types): each record is a flat Skill —
number / name / source_version / attack_damage / damage_type / element? /
inflicts? / range / shape (kind-tagged) / cost{mana,ability} /
learn{level,energy,command} / classes (ClassSet list) / review?. Provenance
(source_version + optional review) rides as top-level keys.

Numbers are the authentic Skill.txt-era values, transcribed from the OpenMU
initializers named below (identical to v1; only the emit shape changed):

  Version095d/SkillsInitializer.cs   -> 075 / 095d roster (skills already in
                                        Version075 tagged "075", the five 095d
                                        additions tagged "095d")
  VersionSeasonSix/...SkillsInitializer -> curated 0.97/1.0 backports, tagged
                                        "s6", each carrying a review line

What v1's sim model is GONE in v2 (became Rust, not data):
  * AreaSkillSettings (frustum/circle geometry, deferred_hits, per-tile and
    between-hit delays, hits_per_target, hits_per_attack_range,
    hit_chance_per_distance, projectile_count, effect_range) -> the closed
    AreaPattern tag; per-pattern tile math + flagged geometry constants live
    in services::area (W-SRC re-sources the invented values).
  * SkillTarget / TargetRestriction / moves_to_target / moves_target ->
    folded into SkillShape (buff variants carry their own targeting; Lunge
    is the one variant for the five DK weapon skills' knock-forward fact).
  * effect (EffectId) + magic_effects.json -> typed inflicts: Ailment on
    hits and Buff carried inside the buff shape variants. magic_effects.json,
    its record type, and every PowerUp/Aggregate/Operator/ScaledBy datum are
    deleted; magnitudes/durations/slots became services constants in Rust.
  * generic_monster_skill (number 150) -> deleted; a contentless placeholder
    OpenMU invented to patch its own dangling attack_skill=150 refs. Monster
    attacks are modeled natively in monsters_spawns (MonsterAttack::plain /
    skill{skill}); the phantom referencers ship as plain with a review note.
  * damage_scaling / skip_elemental_modifier / implicit_target_range /
    hits_per_attack / the id slug -> gone (services rules or dead columns).

Teleport (6) is corrected to damage_type "none" (the 075 initializer's
wizardry tag was residue). Cometfall (13) resolves to an area skill despite
095d tagging it a direct hit (it attaches area settings; the S6 dataset
agrees). Nova is two records: Area{nova} (40) + NovaCharge (58).
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import coverage, without_name, write_datafile, write_names

# ---------------------------------------------------------------- shapes

def direct_hit():
    return {"kind": "direct_hit"}


def lunge():
    return {"kind": "lunge"}


def area(pattern):
    return {"kind": "area", "pattern": pattern}


def buff_self(buff):
    return {"kind": "buff_self", "buff": buff}


def buff_player(buff):
    return {"kind": "buff_player", "buff": buff}


def buff_party_member(buff):
    return {"kind": "buff_party_member", "buff": buff}


def buff_party(buff):
    return {"kind": "buff_party", "buff": buff}


def heal():
    return {"kind": "heal"}


def summon(monster):
    return {"kind": "summon", "monster": monster}


def teleport():
    return {"kind": "teleport"}


def nova_charge():
    return {"kind": "nova_charge"}


def recall_party():
    return {"kind": "recall_party"}


# ---------------------------------------------------------------- builder

def skill(number, name, source_version, shape, classes,
          attack_damage=0, damage_type="none", element=None, inflicts=None,
          rng=0, mana=0, ability=0, level=0, energy=0, command=0, review=None):
    r = {
        "number": number,
        "name": name,
        "source_version": source_version,
        "attack_damage": attack_damage,
        "damage_type": damage_type,
        "element": element,
        "inflicts": inflicts,
        "range": rng,
        "shape": shape,
        "cost": {"mana": mana, "ability": ability},
        "learn": {"level": level, "energy": energy, "command": command},
        "classes": list(classes),
    }
    if review:
        r["review"] = review
    return r


DW_MG = ("dark_wizard", "magic_gladiator")
DK_MG = ("dark_knight", "magic_gladiator")
ELF = ("fairy_elf",)

SKILLS = [
    # ---------- Version095d baseline (075 / 095d values) ----------
    skill(1, "Poison", "075", direct_hit(), DW_MG, attack_damage=12,
          damage_type="wizardry", element="poison", inflicts="poisoned",
          rng=6, mana=42, energy=140),
    skill(2, "Meteorite", "075", direct_hit(), DW_MG, attack_damage=21,
          damage_type="wizardry", element="earth", rng=6, mana=12,
          energy=104),
    skill(3, "Lightning", "075", direct_hit(), DW_MG, attack_damage=17,
          damage_type="wizardry", element="lightning", rng=6, mana=15,
          energy=72),
    skill(4, "Fire Ball", "075", direct_hit(), DW_MG, attack_damage=8,
          damage_type="wizardry", element="fire", rng=6, mana=3, energy=40),
    skill(5, "Flame", "075", area("flame"), DW_MG, attack_damage=25,
          damage_type="wizardry", element="fire", rng=6, mana=50,
          energy=160),
    skill(6, "Teleport", "075", teleport(), ("dark_wizard",),
          rng=6, mana=30, energy=88),
    skill(7, "Ice", "075", direct_hit(), DW_MG, attack_damage=10,
          damage_type="wizardry", element="ice", inflicts="iced",
          rng=6, mana=38, energy=120),
    skill(8, "Twister", "075", area("twister"), DW_MG, attack_damage=35,
          damage_type="wizardry", element="wind", rng=6, mana=60,
          energy=180),
    skill(9, "Evil Spirit", "075", area("evil_spirit"), DW_MG,
          attack_damage=45, damage_type="wizardry", rng=6, mana=90,
          energy=220),
    skill(10, "Hellfire", "075", area("hellfire"), DW_MG, attack_damage=120,
          damage_type="wizardry", element="fire", mana=160, energy=260),
    skill(11, "Power Wave", "075", direct_hit(), DW_MG, attack_damage=14,
          damage_type="wizardry", rng=6, mana=5, energy=56),
    skill(12, "Aqua Beam", "075", area("aqua_beam"), DW_MG, attack_damage=80,
          damage_type="wizardry", element="water", rng=6, mana=140,
          energy=345),
    skill(13, "Cometfall", "095d", area("cometfall"), DW_MG,
          attack_damage=70, damage_type="wizardry", element="lightning",
          rng=3, mana=150, energy=436,
          review="095d source marks it a plain direct hit yet attaches "
                 "area-of-effect settings; encoded as an area skill (AoE "
                 "intent; the S6 dataset also treats it as an area skill)"),
    skill(14, "Inferno", "095d", area("inferno"), DW_MG, attack_damage=100,
          damage_type="wizardry", element="fire", mana=200, energy=578),
    skill(17, "Energy Ball", "075", direct_hit(), DW_MG, attack_damage=3,
          damage_type="wizardry", rng=6, mana=1),
    skill(18, "Defense", "075", buff_self("defense"), DK_MG, mana=30),
    skill(19, "Falling Slash", "075", lunge(), DK_MG,
          damage_type="physical", rng=3, mana=9),
    skill(20, "Lunge", "075", lunge(), DK_MG,
          damage_type="physical", rng=2, mana=9),
    skill(21, "Uppercut", "075", lunge(), DK_MG,
          damage_type="physical", rng=2, mana=8),
    skill(22, "Cyclone", "075", lunge(), DK_MG,
          damage_type="physical", rng=2, mana=9),
    skill(23, "Slash", "075", lunge(), DK_MG,
          damage_type="physical", rng=2, mana=10),
    skill(24, "Triple Shot", "075", area("triple_shot"), ELF,
          damage_type="physical", rng=6, mana=5),
    skill(26, "Heal", "075", heal(), ELF, rng=6, mana=20, energy=52),
    skill(27, "Greater Defense", "075", buff_player("greater_defense"), ELF,
          rng=6, mana=30, energy=72),
    skill(28, "Greater Damage", "075", buff_player("greater_damage"), ELF,
          rng=6, mana=40, energy=92),
    skill(30, "Summon Goblin", "075", summon(26), ELF, mana=40, energy=90),
    skill(31, "Summon Stone Golem", "075", summon(32), ELF, mana=70,
          energy=170),
    skill(32, "Summon Assassin", "075", summon(21), ELF, mana=110,
          energy=190),
    skill(33, "Summon Elite Yeti", "075", summon(20), ELF, mana=160,
          energy=230),
    skill(34, "Summon Dark Knight", "075", summon(10), ELF, mana=200,
          energy=250),
    skill(35, "Summon Bali", "075", summon(150), ELF, mana=250, energy=260),
    skill(41, "Twisting Slash", "095d", area("twisting_slash"), DK_MG,
          damage_type="physical", element="wind", rng=2, mana=10),
    skill(47, "Impale", "095d", direct_hit(), DK_MG, attack_damage=15,
          damage_type="physical", rng=3, mana=8, level=28),
    skill(49, "Fire Breath", "095d", direct_hit(), DK_MG, attack_damage=30,
          damage_type="physical", rng=3, mana=9, level=110),
    skill(50, "Flame of Evil (Monster)", "075", direct_hit(), (),
          attack_damage=120, mana=160, level=60, energy=100),

    # ---------- curated 1.0-era backports (VersionSeasonSix values) --------
    skill(16, "Soul Barrier", "s6", buff_party_member("soul_barrier"),
          ("soul_master",), rng=6, mana=70, ability=22, energy=408,
          review="retail 0.97/1.0 Soul Master skill; values from the S6 "
                 "initializer; S6 allows casting on party members - retail "
                 "may be self-only"),
    skill(39, "Ice Storm", "s6", area("ice_storm"), ("soul_master",),
          attack_damage=80, damage_type="wizardry", element="ice",
          inflicts="iced", rng=6, mana=100, ability=5, energy=849,
          review="retail 1.0-era Soul Master AoE; absent from 075/095d, "
                 "values from S6"),
    skill(40, "Nova", "s6", area("nova"), ("soul_master",),
          damage_type="wizardry", element="fire", rng=6, mana=15, level=100,
          energy=1052,
          review="retail 1.0 Soul Master skill; mana 15 = 180 per full "
                 "12-stage charge; per-stage bonus damage resolves in the "
                 "Nova area routine"),
    skill(58, "Nova (Start)", "s6", nova_charge(), ("soul_master",),
          ability=45, level=100, energy=1052,
          review="charge-phase companion of the Nova backport (skill 40); "
                 "not on the curated list but Nova is unusable without it"),
    skill(42, "Rageful Blow", "s6", area("rageful_blow"), ("blade_knight",),
          attack_damage=60, damage_type="physical", element="earth",
          rng=3, mana=25, ability=20, level=170,
          review="retail 0.97/1.0 Blade Knight skill; absent from 075/095d, "
                 "values from S6"),
    skill(43, "Death Stab", "s6", area("death_stab"), ("blade_knight",),
          attack_damage=70, damage_type="physical", element="wind",
          rng=2, mana=15, ability=12, level=160,
          review="retail 0.97/1.0 Blade Knight skill; absent from 075/095d, "
                 "values from S6"),
    skill(48, "Swell Life", "s6", buff_party("swell_life"),
          ("dark_knight", "blade_knight"), mana=22, ability=24, level=120,
          review="retail 0.97/1.0 knight party buff (Greater Fortitude); "
                 "absent from 075/095d, values from S6"),
    skill(51, "Ice Arrow", "s6", direct_hit(), ("muse_elf",),
          attack_damage=105, damage_type="physical", element="ice",
          inflicts="frozen", rng=8, mana=10, ability=12,
          review="retail 1.0 Muse Elf skill; absent from 075/095d, values "
                 "from S6; S6 skill_multiplier rebalance relationship not "
                 "extracted (see gaps)"),
    skill(52, "Penetration", "s6", area("penetration"),
          ("fairy_elf", "muse_elf"), attack_damage=70,
          damage_type="physical", element="wind", rng=6, mana=7, ability=9,
          level=130,
          review="retail 1.0 elf skill; absent from 075/095d, values from "
                 "S6; S6 skill_multiplier rebalance relationship not "
                 "extracted (see gaps)"),
    skill(55, "Fire Slash", "s6", area("fire_slash"), ("magic_gladiator",),
          attack_damage=80, damage_type="physical", element="fire",
          inflicts="defense_reduction", rng=2, mana=15, ability=20,
          review="retail 1.0-era Magic Gladiator skill; absent from "
                 "075/095d, values from S6"),
    skill(56, "Power Slash", "s6", area("power_slash"), ("magic_gladiator",),
          damage_type="physical", rng=5, mana=15,
          review="retail 1.0-era Magic Gladiator skill; absent from "
                 "075/095d, values from S6"),
    skill(61, "Fire Burst", "s6", area("fire_burst"), ("dark_lord",),
          attack_damage=100, damage_type="physical", rng=6, mana=25,
          energy=79,
          review="Dark Lord backport (DL is 0.97/1.0 content, only in the "
                 "S6 dataset)"),
    skill(62, "Earthshake", "s6", area("earthshake"), ("dark_lord",),
          attack_damage=150, damage_type="physical", element="lightning",
          rng=10, ability=50,
          review="Dark Lord backport; S6 horse_level*10 damage term dropped "
                 "(dark horse pet excluded pre-S3, see gaps)"),
    skill(63, "Summon", "s6", recall_party(), ("dark_lord",),
          mana=70, ability=30, energy=153, command=400,
          review="Dark Lord backport; summons party members (not a monster "
                 "summon), party-summon behavior is a rules concern"),
    skill(64, "Increase Critical Damage", "s6",
          buff_party("critical_damage_increase"), ("dark_lord",),
          mana=50, ability=50, energy=102, command=300,
          review="Dark Lord backport (DL is 0.97/1.0 content, only in the "
                 "S6 dataset)"),
    skill(77, "Infinity Arrow", "s6", buff_self("infinite_arrow"),
          ("muse_elf",), rng=6, mana=50, ability=10, level=220,
          review="retail 1.0 Muse Elf buff; absent from 075/095d, values "
                 "from S6"),
]

GAPS = [
    "blade_knight skill combo: the combo definition (completion window; step "
    "1 slash/cyclone/lunge/falling_slash/uppercut, step 2 twisting_slash/"
    "rageful_blow/death_stab, final twisting_slash/rageful_blow/death_stab) "
    "is attached to the character class in the source and is a class/rules "
    "concern, not a skill record - not extracted",
    "earthshake bonus damage: the S6 horse_level*10 term is dropped - the "
    "dark-horse trainable pet is excluded pre-S3; the strength/10 + "
    "command/5 terms live in services::skill_damage",
    "ice_arrow / penetration: the S6 per-skill final-damage rebalance "
    "(2.0 x skill_multiplier) is S6 damage-tuning, not a pre-S3 fact - not "
    "extracted",
    "dl summon (skill 63): summons party members; encoded as shape "
    "recall_party, the party-recall execution itself is a services/rules "
    "concern (W-CMB)",
    "evolved-class qualification: 075/095d records keep the literal 095d "
    "base-class masks; whether soul_master/blade_knight/muse_elf inherit "
    "base-class skills is a class/rules concern. s6 backports carry the S6 "
    "masks filtered to pre-S3 classes",
    "nova per-stage damage, and the Hellfire/Inferno/Nova caster-centered "
    "radii, are OpenMU engine-default resolved values with no authentic "
    "source - named open review items in services::area/skill_damage "
    "(W-SRC), no number invented into the data",
]

NOTES = [
    "records tagged 075 carry the approved 095d baseline values; 075/095d "
    "deltas (evil_spirit range, triple_shot projectile fan, summon energy "
    "requirements, MG class masks) are the same facts v1 recorded, now "
    "emitted only as range/element/damage/cost/learn columns",
    "magic_effects.json is deleted - the closed pre-S3 effect roster became "
    "the Buff and Ailment Rust enums; every effect magnitude, duration, "
    "stacking slot, and tick rule became a bespoke services constant/function",
    "buff skills carry their Buff inside the shape variant (buff_self / "
    "buff_player / buff_party_member / buff_party); ailment-inflicting hits "
    "carry inflicts: Ailment (1->poisoned, 7->iced, 39->iced, 51->frozen, "
    "55->defense_reduction). heal is instantaneous - no effect identity",
    "generic_monster_skill (150) is dropped: it was OpenMU's placeholder for "
    "its own dangling monster attack_skill=150 refs; monsters_spawns models "
    "monster attacks natively and the phantom referencers ship as plain",
    "AreaSkillSettings sim-knobs (geometry, deferred_hits, per-tile / "
    "between-hit delays, hit_chance_per_distance, projectile_count, "
    "effect_range, hits_per_* budgets) are dropped from the data: the closed "
    "AreaPattern tag carries the authentic 'this is an area skill of family "
    "X' fact, and per-pattern tile math + flagged geometry constants live in "
    "services::area (invented values re-sourced under W-SRC)",
    "teleport (6) damage_type corrected to none (075 wizardry tag was "
    "residue); cometfall (13) resolves to area despite the 095d direct-hit "
    "tag; nova is two records - area{nova} (40) plus nova_charge (58)",
    "classes serialize as the ClassSet list; skill 50 (Flame of Evil) has an "
    "all-false / empty class set - authentic monster-learnable-by-none fact",
    "s6 leadership learn requirements are renamed to the authentic Command "
    "stat: skill 63 command 400, skill 64 command 300 (v1 'leadership' dead)",
]

# ---------------------------------------------------------------- validation

CLASSES = {"dark_wizard", "dark_knight", "fairy_elf", "magic_gladiator",
           "dark_lord", "soul_master", "blade_knight", "muse_elf"}
ELEMENTS = {"ice", "poison", "lightning", "fire", "earth", "wind", "water"}
AILMENTS = {"poisoned", "iced", "frozen", "defense_reduction"}
BUFFS = {"defense", "greater_damage", "greater_defense", "soul_barrier",
         "swell_life", "critical_damage_increase", "infinite_arrow",
         "alcohol"}
DAMAGE_TYPES = {"none", "physical", "wizardry"}
AREA_PATTERNS = {"flame", "twister", "evil_spirit", "hellfire", "aqua_beam",
                 "cometfall", "inferno", "triple_shot", "ice_storm", "nova",
                 "twisting_slash", "rageful_blow", "death_stab",
                 "penetration", "fire_slash", "power_slash", "fire_burst",
                 "earthshake"}
BUFF_KINDS = {"buff_self", "buff_player", "buff_party_member", "buff_party"}
SHAPE_KINDS = ({"direct_hit", "lunge", "area", "heal", "summon", "teleport",
                "nova_charge", "recall_party"} | BUFF_KINDS)
U16 = 0xFFFF


def check(skills):
    """Loud cross-checks before writing; throwaway."""
    numbers = [s["number"] for s in skills]
    assert len(set(numbers)) == len(numbers), "duplicate skill number"
    assert 150 not in numbers, "generic_monster_skill (150) must be dropped"
    for s in skills:
        sid = s["number"]
        assert s["source_version"] in ("075", "095d", "s6"), sid
        assert s["source_version"] != "s6" or s.get("review"), sid
        assert s["damage_type"] in DAMAGE_TYPES, sid
        assert s["element"] is None or s["element"] in ELEMENTS, sid
        assert s["inflicts"] is None or s["inflicts"] in AILMENTS, sid
        assert 0 <= s["attack_damage"] <= U16, sid
        assert 0 <= s["range"] <= 255, sid
        for field in ("mana", "ability"):
            assert 0 <= s["cost"][field] <= U16, (sid, field)
        for field in ("level", "energy", "command"):
            assert 0 <= s["learn"][field] <= U16, (sid, field)
        for cls in s["classes"]:
            assert cls in CLASSES, (sid, cls)
        assert len(set(s["classes"])) == len(s["classes"]), sid
        shape = s["shape"]
        kind = shape["kind"]
        assert kind in SHAPE_KINDS, (sid, kind)
        if kind == "area":
            assert shape["pattern"] in AREA_PATTERNS, (sid, shape["pattern"])
        elif kind in BUFF_KINDS:
            assert shape["buff"] in BUFFS, (sid, shape["buff"])
        elif kind == "summon":
            assert isinstance(shape["monster"], int) and shape["monster"] > 0, sid


def by_version(records):
    out = {}
    for r in records:
        out[r["source_version"]] = out.get(r["source_version"], 0) + 1
    return out


def main():
    skills = sorted(SKILLS, key=lambda s: s["number"])
    check(skills)

    # Display names -> host-owned sidecar keyed by number; the core file carries
    # only the number and rules.
    write_names("skills.json", {"records": [
        {"number": s["number"], "name": s["name"]} for s in skills]})
    write_datafile("skills.json", [without_name(s) for s in skills])

    reviews = {f"skills/{r['number']}": r["review"]
               for r in skills if "review" in r}
    coverage("skills-effects", {
        "skills": {"records": len(skills),
                   "by_source_version": by_version(skills)},
        "review_count": len(reviews),
        "reviews": reviews,
        "gaps": GAPS,
        "notes": NOTES,
    })
    print(f"skills: {len(skills)}  by_source_version: {by_version(skills)}  "
          f"reviews: {len(reviews)}")


if __name__ == "__main__":
    main()
