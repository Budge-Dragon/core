#!/usr/bin/env python3
"""Extract pre-S3 skills + magic effects (spec sections 7 and 8).

Outputs:
  data/skills.json               spec section 7 records
  data/magic_effects.json        spec section 8 records
  data/_coverage/skills-effects.json

Baseline: Version095d/SkillsInitializer.cs (35 skills; skills already present
in Version075 are tagged "075", the five 095d additions "095d") plus the five
095d magic-effect initializers and the on-demand elemental effects (Iced,
Poisoned). Curated 1.0-era backports come from VersionSeasonSix (tagged "s6",
each with a review line): SoulBarrier, IceStorm, Nova(+NovaStart), RagefulBlow,
DeathStab, SwellLife, IceArrow, Penetration, FireSlash, PowerSlash, FireBurst,
Earthshake, DL Summon, IncreaseCriticalDamage, InfinityArrow and their effects
(SoulBarrier, SwellLife, CriticalDamageIncrease, InfiniteArrow, Freeze,
DefenseReduction), plus the Generic Monster Skill (150) that 075/095d monster
initializers reference but only the S6 skills initializer defines.

All values below are hand-transcribed from the initializers named above;
durations are converted seconds -> integer milliseconds. Effect client numbers
are the canonical MagicEffectNumber values (the 095d 'effect number := skill
number' rewrite is a client-protocol concern, see coverage notes).
"""

import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import DATA_DIR, coverage, load_stat_map, write_datafile

STAT_MAP = load_stat_map()


def st(openmu_name):
    """OpenMU stat property name -> mu-core slug (fails loud on unknowns)."""
    return STAT_MAP[openmu_name]


# ---------------------------------------------------------------- power-ups

def rel(stat, operand, operator="multiply"):
    return {"stat": st(stat), "operator": operator, "operand": operand}


def power_up(stat, value, aggregate="add_raw", scaled_by=None):
    p = {"stat": st(stat), "value": value, "aggregate": aggregate}
    if scaled_by:
        p["scaled_by"] = scaled_by
    return p


def duration(seconds, scaled_by_ms=None, max_seconds=None):
    """Effect duration; source seconds -> ms (scaling operands also in ms)."""
    d = {"constant_ms": int(seconds * 1000)}
    if scaled_by_ms:
        d["scaled_by"] = scaled_by_ms
    d["max_ms"] = int(max_seconds * 1000) if max_seconds is not None else None
    return d


# ---------------------------------------------------------------- area shapes

def frustum(start_width, end_width, distance):
    return {"kind": "frustum", "start_width": start_width,
            "end_width": end_width, "distance": distance}


def circle(diameter):
    return {"kind": "circle", "diameter": diameter}


NO_GEOMETRY = {"kind": "none"}


def area(geometry=NO_GEOMETRY, deferred=False, per_tile_ms=0, between_ms=0,
         per_target=(1, 1), per_attack=(0, 0), chance=1.0, projectiles=1,
         effect_range=0):
    """AreaSkillSettings with the initializer's default values filled in."""
    return {
        "geometry": geometry,
        "deferred_hits": deferred,
        "delay_per_tile_ms": per_tile_ms,
        "delay_between_hits_ms": between_ms,
        "hits_per_target": list(per_target),
        "hits_per_attack_range": list(per_attack),
        "hit_chance_per_distance": chance,
        "projectile_count": projectiles,
        "effect_range": effect_range,
    }


# ---------------------------------------------------------------- skills

def skill(number, slug, name, source_version, behavior, classes,
          damage=0, damage_type="none", target="explicit", restriction="none",
          rng=0, implicit_range=0, hits=1, moves_to_target=False,
          moves_target=False, element=None, skip_elemental=False, effect=None,
          damage_scaling=None, level=0, leadership=0, energy=0,
          mana=0, ability=0, review=None):
    r = {"number": number, "id": slug, "name": name,
         "source_version": source_version}
    if review:
        r["review"] = review
    r.update({
        "attack_damage": damage,
        "damage_type": damage_type,
        "behavior": behavior,
        "target": target,
        "target_restriction": restriction,
        "range": rng,
        "implicit_target_range": implicit_range,
        "hits_per_attack": hits,
        "moves_to_target": moves_to_target,
        "moves_target": moves_target,
        "element": element,
        "skip_elemental_modifier": skip_elemental,
        "effect": effect,
    })
    if damage_scaling:
        r["damage_scaling"] = damage_scaling
    reqs = []
    if level:
        reqs.append({"stat": st("Level"), "value": level})
    if leadership:
        reqs.append({"stat": st("TotalLeadership"), "value": leadership})
    if energy:
        reqs.append({"stat": st("TotalEnergy"), "value": energy})
    consume = []
    if mana:
        consume.append({"stat": st("CurrentMana"), "value": mana})
    if ability:
        consume.append({"stat": st("CurrentAbility"), "value": ability})
    r["requirements"] = reqs
    r["consume"] = consume
    r["classes"] = list(classes)
    return r


DIRECT = {"kind": "direct_hit"}
BUFF = {"kind": "buff"}
REGEN = {"kind": "regeneration"}
OTHER = {"kind": "other"}


def area_auto(a):
    return {"kind": "area_automatic", "area": a}


def summon(monster):
    return {"kind": "summon", "monster": monster}


DW_MG = ("dark_wizard", "magic_gladiator")
DK_MG = ("dark_knight", "magic_gladiator")
ELF = ("fairy_elf",)

SKILLS = [
    # ------------- Version095d baseline (values from 095d/SkillsInitializer)
    skill(1, "poison", "Poison", "075", DIRECT, DW_MG, damage=12,
          damage_type="wizardry", rng=6, mana=42, energy=140,
          element="poison", effect="poisoned"),
    skill(2, "meteorite", "Meteorite", "075", DIRECT, DW_MG, damage=21,
          damage_type="wizardry", rng=6, mana=12, energy=104, element="earth"),
    skill(3, "lightning", "Lightning", "075", DIRECT, DW_MG, damage=17,
          damage_type="wizardry", rng=6, mana=15, energy=72,
          element="lightning"),
    skill(4, "fire_ball", "Fire Ball", "075", DIRECT, DW_MG, damage=8,
          damage_type="wizardry", rng=6, mana=3, energy=40, element="fire"),
    skill(5, "flame", "Flame", "075",
          area_auto(area(circle(2.0), deferred=True, between_ms=500,
                         per_target=(0, 2), chance=0.5)),
          DW_MG, damage=25, damage_type="wizardry", rng=6, mana=50,
          energy=160, element="fire"),
    skill(6, "teleport", "Teleport", "075", OTHER, ("dark_wizard",),
          damage_type="wizardry", rng=6, mana=30, energy=88),
    skill(7, "ice", "Ice", "075", DIRECT, DW_MG, damage=10,
          damage_type="wizardry", rng=6, mana=38, energy=120,
          element="ice", effect="iced"),
    skill(8, "twister", "Twister", "075",
          area_auto(area(frustum(1.5, 1.5, 4.0), deferred=True,
                         per_tile_ms=300, between_ms=1000, per_target=(0, 2),
                         chance=0.7)),
          DW_MG, damage=35, damage_type="wizardry", rng=6, mana=60,
          energy=180, element="wind"),
    skill(9, "evil_spirit", "Evil Spirit", "075",
          area_auto(area(deferred=True, per_tile_ms=100, between_ms=1000,
                         per_target=(0, 2), chance=0.7)),
          DW_MG, damage=45, damage_type="wizardry", rng=6, mana=90,
          energy=220),
    skill(10, "hellfire", "Hellfire", "075", area_auto(area()), DW_MG,
          damage=120, damage_type="wizardry", mana=160, energy=260,
          element="fire"),
    skill(11, "power_wave", "Power Wave", "075", DIRECT, DW_MG, damage=14,
          damage_type="wizardry", rng=6, mana=5, energy=56),
    skill(12, "aqua_beam", "Aqua Beam", "075",
          area_auto(area(frustum(1.5, 1.5, 8.0))), DW_MG, damage=80,
          damage_type="wizardry", rng=6, mana=140, energy=345,
          element="water"),
    skill(13, "cometfall", "Cometfall", "095d",
          area_auto(area(circle(2.0))), DW_MG, damage=70,
          damage_type="wizardry", rng=3, mana=150, energy=436,
          element="lightning",
          review="095d source marks it a plain direct hit yet attaches "
                 "target-area settings; encoded as area_automatic (AoE "
                 "intent; the S6 dataset marks it an automatic-hits area "
                 "skill)"),
    skill(14, "inferno", "Inferno", "095d", area_auto(area()), DW_MG,
          damage=100, damage_type="wizardry", mana=200, energy=578,
          element="fire"),
    skill(17, "energy_ball", "Energy Ball", "075", DIRECT, DW_MG, damage=3,
          damage_type="wizardry", rng=6, mana=1),
    skill(18, "defense", "Defense", "075", BUFF, DK_MG, restriction="self",
          mana=30, effect="shield_defense"),
    skill(19, "falling_slash", "Falling Slash", "075", DIRECT, DK_MG,
          damage_type="physical", rng=3, mana=9, moves_to_target=True,
          moves_target=True),
    skill(20, "lunge", "Lunge", "075", DIRECT, DK_MG, damage_type="physical",
          rng=2, mana=9, moves_to_target=True, moves_target=True),
    skill(21, "uppercut", "Uppercut", "075", DIRECT, DK_MG,
          damage_type="physical", rng=2, mana=8, moves_to_target=True,
          moves_target=True),
    skill(22, "cyclone", "Cyclone", "075", DIRECT, DK_MG,
          damage_type="physical", rng=2, mana=9, moves_to_target=True,
          moves_target=True),
    skill(23, "slash", "Slash", "075", DIRECT, DK_MG, damage_type="physical",
          rng=2, mana=10, moves_to_target=True, moves_target=True),
    skill(24, "triple_shot", "Triple Shot", "075",
          area_auto(area(frustum(1.0, 4.5, 7.0), deferred=True,
                         per_tile_ms=50, per_target=(1, 3), per_attack=(0, 3),
                         projectiles=3)),
          ELF, damage_type="physical", rng=6, mana=5),
    skill(26, "heal", "Heal", "075", REGEN, ELF, restriction="player", rng=6,
          mana=20, energy=52, effect="heal"),
    skill(27, "greater_defense", "Greater Defense", "075", BUFF, ELF,
          restriction="player", rng=6, mana=30, energy=72,
          effect="greater_defense"),
    skill(28, "greater_damage", "Greater Damage", "075", BUFF, ELF,
          restriction="player", rng=6, mana=40, energy=92,
          effect="greater_damage"),
    skill(30, "summon_goblin", "Summon Goblin", "075", summon(26), ELF,
          mana=40, energy=90),
    skill(31, "summon_stone_golem", "Summon Stone Golem", "075", summon(32),
          ELF, mana=70, energy=170),
    skill(32, "summon_assassin", "Summon Assassin", "075", summon(21), ELF,
          mana=110, energy=190),
    skill(33, "summon_elite_yeti", "Summon Elite Yeti", "075", summon(20),
          ELF, mana=160, energy=230),
    skill(34, "summon_dark_knight", "Summon Dark Knight", "075", summon(10),
          ELF, mana=200, energy=250),
    skill(35, "summon_bali", "Summon Bali", "075", summon(150), ELF,
          mana=250, energy=260),
    skill(41, "twisting_slash", "Twisting Slash", "095d", area_auto(area()),
          DK_MG, damage_type="physical", rng=2, mana=10, element="wind"),
    skill(47, "impale", "Impale", "095d", DIRECT, DK_MG, damage=15,
          damage_type="physical", rng=3, mana=8, level=28),
    skill(49, "fire_breath", "Fire Breath", "095d", DIRECT, DK_MG, damage=30,
          damage_type="physical", rng=3, mana=9, level=110),
    skill(50, "flame_of_evil_monster", "Flame of Evil (Monster)", "075",
          DIRECT, (), damage=120, mana=160, level=60, energy=100),

    # ------------- curated 1.0-era backports (VersionSeasonSix values)
    skill(16, "soul_barrier", "Soul Barrier", "s6", BUFF, ("soul_master",),
          restriction="party", rng=6, mana=70, ability=22, energy=408,
          effect="soul_barrier",
          review="retail 0.97/1.0 Soul Master skill; absent from OpenMU "
                 "075/095d datasets, values from the S6 initializer"),
    skill(39, "ice_storm", "Ice Storm", "s6",
          area_auto(area(circle(3.0), deferred=True)), ("soul_master",),
          damage=80, damage_type="wizardry", rng=6, mana=100, ability=5,
          energy=849, element="ice", effect="iced",
          review="retail 1.0-era Soul Master AoE; absent from 075/095d, "
                 "values from S6"),
    skill(40, "nova", "Nova", "s6", DIRECT, ("soul_master",),
          damage_type="wizardry", rng=6, mana=15, level=100, energy=1052,
          element="fire",
          damage_scaling=[rel("TotalStrength", 0.5),
                          rel("NovaStageDamage", 1.0)],
          review="retail 1.0 Soul Master skill; mana 15 = 180 per full "
                 "12-stage charge; stage damage feeds nova_stage_damage"),
    skill(58, "nova_start", "Nova (Start)", "s6", OTHER, ("soul_master",),
          ability=45, level=100, energy=1052,
          review="charge-phase companion of the Nova backport (skill 40); "
                 "not on the curated list but Nova is unusable without it"),
    skill(42, "rageful_blow", "Rageful Blow", "s6", area_auto(area()),
          ("blade_knight",), damage=60, damage_type="physical", rng=3,
          mana=25, ability=20, level=170, element="earth",
          review="retail 0.97/1.0 Blade Knight skill; absent from 075/095d, "
                 "values from S6"),
    skill(43, "death_stab", "Death Stab", "s6", DIRECT, ("blade_knight",),
          damage=70, damage_type="physical",
          target="explicit_with_implicit_in_range", implicit_range=1, rng=2,
          mana=15, ability=12, level=160, element="wind",
          review="retail 0.97/1.0 Blade Knight skill; absent from 075/095d, "
                 "values from S6"),
    skill(48, "swell_life", "Swell Life", "s6", BUFF,
          ("dark_knight", "blade_knight"), target="implicit_party", mana=22,
          ability=24, level=120, effect="swell_life",
          review="retail 0.97/1.0 knight party buff (Greater Fortitude); "
                 "absent from 075/095d, values from S6"),
    skill(51, "ice_arrow", "Ice Arrow", "s6", DIRECT, ("muse_elf",),
          damage=105, damage_type="physical", rng=8, mana=10, ability=12,
          element="ice", effect="freeze",
          review="retail 1.0 Muse Elf skill; absent from 075/095d, values "
                 "from S6; S6 skill_multiplier rebalance relationship not "
                 "extracted (see gaps)"),
    skill(52, "penetration", "Penetration", "s6",
          area_auto(area(frustum(1.1, 1.2, 8.0), deferred=True,
                         per_tile_ms=50)),
          ("fairy_elf", "muse_elf"), damage=70, damage_type="physical",
          rng=6, mana=7, ability=9, level=130, element="wind",
          review="retail 1.0 elf skill; absent from 075/095d, values from "
                 "S6; S6 skill_multiplier rebalance relationship not "
                 "extracted (see gaps)"),
    skill(55, "fire_slash", "Fire Slash", "s6",
          area_auto(area(frustum(1.5, 2.0, 2.0))), ("magic_gladiator",),
          damage=80, damage_type="physical", rng=2, mana=15, ability=20,
          element="fire", effect="defense_reduction",
          review="retail 1.0-era Magic Gladiator skill; absent from "
                 "075/095d, values from S6"),
    skill(56, "power_slash", "Power Slash", "s6",
          area_auto(area(frustum(1.0, 6.0, 6.0))), ("magic_gladiator",),
          damage_type="physical", rng=5, mana=15,
          review="retail 1.0-era Magic Gladiator skill; absent from "
                 "075/095d, values from S6"),
    skill(61, "fire_burst", "Fire Burst", "s6", DIRECT, ("dark_lord",),
          damage=100, damage_type="physical",
          target="explicit_with_implicit_in_range", implicit_range=1, rng=6,
          mana=25, energy=79,
          review="Dark Lord backport (DL is 0.97/1.0 content, only in the "
                 "S6 dataset)"),
    skill(62, "earthshake", "Earthshake", "s6",
          area_auto(area(circle(10.0), per_attack=(9, 15))), ("dark_lord",),
          damage=150, damage_type="physical", rng=10, ability=50,
          element="lightning", skip_elemental=True,
          damage_scaling=[rel("TotalStrength", 0.1),
                          rel("TotalLeadership", 0.2)],
          review="Dark Lord backport; S6 horse_level*10 damage term dropped "
                 "(dark horse pet excluded pre-S3, see gaps)"),
    skill(63, "summon", "Summon", "s6", OTHER, ("dark_lord",), mana=70,
          ability=30, energy=153, leadership=400,
          review="Dark Lord backport; summons party members (not a monster "
                 "summon), party-summon behavior is a rules concern"),
    skill(64, "increase_critical_damage", "Increase Critical Damage", "s6",
          BUFF, ("dark_lord",), target="implicit_party", mana=50, ability=50,
          energy=102, leadership=300, effect="critical_damage_increase",
          review="Dark Lord backport (DL is 0.97/1.0 content, only in the "
                 "S6 dataset)"),
    skill(77, "infinity_arrow", "Infinity Arrow", "s6", BUFF, ("muse_elf",),
          restriction="self", rng=6, mana=50, ability=10, level=220,
          effect="infinite_arrow",
          review="retail 1.0 Muse Elf buff; absent from 075/095d, values "
                 "from S6"),
    skill(150, "generic_monster_skill", "Generic Monster Skill", "s6", OTHER,
          (), rng=5,
          review="monster-only attack skill: 075/095d monster initializers "
                 "(Death Gorgon/Balrog/Hydra and 095d bosses) set "
                 "attack_skill 150, but only the S6 skills initializer "
                 "defines it - upstream 075/095d lookup silently resolves "
                 "to null (latent omission); backported so those "
                 "attack_skill references resolve"),
]

# ---------------------------------------------------------------- effects

def effect(slug, number, source_version, sub_type, stop_by_death, power_ups,
           dur=None, review=None):
    r = {"id": slug, "number": number, "source_version": source_version}
    if review:
        r["review"] = review
    r["sub_type"] = sub_type
    r["stop_by_death"] = stop_by_death
    if dur:
        r["duration"] = dur
    r["power_ups"] = power_ups
    return r


ENERGY_MS = lambda seconds_per_point: [  # noqa: E731 - tiny local shorthand
    rel("TotalEnergy", seconds_per_point * 1000.0)]

MAGIC_EFFECTS = [
    # ------------- 075/095d initializers
    effect("heal", -2, "075", 0, False,
           [power_up("CurrentHealth", 5.0,
                     scaled_by=[rel("TotalEnergy", 1 / 5)])]),
    effect("greater_damage", 1, "075", 0, True,
           [power_up("GreaterDamageBonus", 3.0,
                     scaled_by=[rel("TotalEnergy", 1 / 7)])],
           dur=duration(60)),
    effect("greater_defense", 2, "075", 0, True,
           [power_up("DefenseFinal", 2.0, aggregate="add_final",
                     scaled_by=[rel("TotalEnergy", 1 / 8)])],
           dur=duration(60)),
    effect("poisoned", 55, "075", 253, True,
           [power_up("IsPoisoned", 1.0)], dur=duration(20)),
    effect("iced", 56, "075", 254, True,
           [power_up("IsIced", 1.0),
            power_up("MovementSpeedFactor", 0.5, aggregate="multiplicate")],
           dur=duration(10)),
    effect("shield_defense", 200, "075", 0, True,
           [power_up("DamageReceiveDecrement", 0.5,
                     aggregate="multiplicate")],
           dur=duration(4)),
    effect("alcohol", 201, "075", 54, False,
           [power_up("AttackSpeedAny", 20.0)], dur=duration(80)),

    # ------------- 1.0-era backports (S6 initializers)
    effect("soul_barrier", 4, "s6", 0, True,
           [power_up("SoulBarrierReceiveDecrement", 0.1,
                     scaled_by=[rel("TotalEnergy", 1 / 20000),
                                rel("TotalAgility", 1 / 5000)]),
            power_up("SoulBarrierManaTollPerHit", 0.0,
                     scaled_by=[rel("MaximumMana", 0.02)])],
           dur=duration(60, scaled_by_ms=ENERGY_MS(1 / 40)),
           review="effect of the soul_barrier skill backport (retail "
                  "0.97/1.0); S6 initializer values"),
    effect("critical_damage_increase", 5, "s6", 17, True,
           [power_up("CriticalDamageBonus", 0.0,
                     scaled_by=[rel("TotalEnergy", 1 / 30),
                                rel("TotalLeadership", 1 / 25)])],
           dur=duration(60, scaled_by_ms=ENERGY_MS(1 / 10), max_seconds=180),
           review="effect of the Dark Lord increase_critical_damage "
                  "backport; S6 initializer values"),
    effect("infinite_arrow", 6, "s6", 0, True,
           [power_up("AmmunitionConsumptionRate", 0.0,
                     aggregate="multiplicate")],
           dur=duration(600),
           review="effect of the infinity_arrow backport (retail 1.0); S6 "
                  "zero-value master-skill placeholder power-up dropped "
                  "(see gaps)"),
    effect("swell_life", 8, "s6", 0, True,
           [power_up("MaximumHealth", 1.12, aggregate="multiplicate",
                     scaled_by=[rel("TotalEnergy", 1.0005,
                                    operator="exponentiate_by_attribute"),
                                rel("TotalVitality", 1.0001,
                                    operator="exponentiate_by_attribute")])],
           dur=duration(60, scaled_by_ms=ENERGY_MS(1 / 5)),
           review="effect of the swell_life backport (retail 0.97/1.0 "
                  "Greater Fortitude); S6 initializer values"),
    effect("freeze", 57, "s6", 254, True,
           [power_up("IsFrozen", 1.0)], dur=duration(5),
           review="created on demand by the ice_arrow backport (retail "
                  "1.0); shares sub_type 254 with iced, so freeze replaces "
                  "iced (source behavior)"),
    effect("defense_reduction", 58, "s6", 0, True,
           [power_up("DefenseDecrement", 0.9, aggregate="multiplicate")],
           dur=duration(10),
           review="effect of the fire_slash backport (retail 1.0-era); S6 "
                  "initializer values"),
]

GAPS = [
    "blade_knight skill combo: the combo definition (3000 ms completion "
    "window; step 1 slash/cyclone/lunge/falling_slash/uppercut, step 2 "
    "twisting_slash/rageful_blow/death_stab/+S2 strike_of_destruction, "
    "final twisting_slash/rageful_blow/death_stab) is attached to the "
    "character class in the source and has no slot in spec sections 7/8 - "
    "class concern, not extracted",
    "earthshake damage_scaling: the S6 horse_level*10 term is dropped - "
    "dark horse trainable pet is excluded pre-S3 and horse_level is not in "
    "stats.json",
    "infinite_arrow: S6 zero-value placeholder power-up on "
    "attack_damage_increase (reserved for the S4 master skill) dropped",
    "ice_arrow/penetration: S6 per-skill final-damage relationships (2.0 x "
    "skill_multiplier) not extracted - S6 damage-rebalance data, not pre-S3",
    "dl summon (skill 63): summons party members; encoded as behavior kind "
    "'other', the party-summon behavior itself is a rules concern",
    "evolved-class qualification: 075/095d records keep the literal 095d "
    "class masks (base classes); whether soul_master/blade_knight/muse_elf "
    "inherit base-class skills is a class/rules concern. s6 backports carry "
    "second-class slugs from the S6 masks filtered to pre-S3 classes",
    "the 095d dataset rewrites buff-effect client numbers to the owning "
    "skill number (client-protocol convention); canonical effect numbers "
    "kept per spec section 8 example",
]

NOTES = [
    "records tagged 075 carry 095d values per the approved 095d baseline; "
    "differing 075 values: evil_spirit range 7 (095d: 6), triple_shot was "
    "area-explicit-hits without settings (095d: 3-projectile frustum), "
    "summon energy requirements were 30/60/90/130/170/210 (095d: "
    "90/170/190/230/250/260), MG absent from all class masks",
    "summon skills 30-35 reference monster numbers 26/32/21/20/10/150; "
    "Bali #150 is created by the 095d skills initializer but the monster "
    "record is owned by the monsters extractor (dependency)",
    "effect durations and delays converted seconds -> integer milliseconds; "
    "duration scaled_by operands are ms per stat point",
    "no cooldown field (approved decision 8); no AG costs exist in the "
    "095d baseline - ability consumption appears only on s6 backports",
    "generic_monster_skill (150) is a monster-only skill with no classes; "
    "referenced by monster attack_skill in 075/095d map initializers but "
    "defined only by the S6 skills initializer, hence source_version s6",
]


def check(skills, effects):
    """Cross-checks before writing; throwaway but loud."""
    stat_ids = {r["id"] for r in json.load(
        open(os.path.join(DATA_DIR, "stats.json")))["records"]}
    effect_ids = {e["id"] for e in effects}
    assert len({s["number"] for s in skills}) == len(skills), "dup skill number"
    assert len({s["id"] for s in skills}) == len(skills), "dup skill id"
    assert len({e["id"] for e in effects}) == len(effects), "dup effect id"
    for s in skills:
        assert s["source_version"] in ("075", "095d", "s6"), s["id"]
        assert s["source_version"] != "s6" or s.get("review"), s["id"]
        if s["effect"] is not None:
            assert s["effect"] in effect_ids, (s["id"], s["effect"])
        for req in s["requirements"] + s["consume"]:
            assert req["stat"] in stat_ids, (s["id"], req["stat"])
        for ds in s.get("damage_scaling", ()):
            assert ds["stat"] in stat_ids, (s["id"], ds["stat"])
    for e in effects:
        assert e["source_version"] != "s6" or e.get("review"), e["id"]
        for p in e["power_ups"]:
            assert p["stat"] in stat_ids, (e["id"], p["stat"])
            for sb in p.get("scaled_by", ()):
                assert sb["stat"] in stat_ids, (e["id"], sb["stat"])
        for sb in e.get("duration", {}).get("scaled_by", ()):
            assert sb["stat"] in stat_ids, (e["id"], sb["stat"])


def by_version(records):
    out = {}
    for r in records:
        out[r["source_version"]] = out.get(r["source_version"], 0) + 1
    return out


def main():
    skills = sorted(SKILLS, key=lambda s: s["number"])
    effects = sorted(MAGIC_EFFECTS, key=lambda e: e["number"])
    check(skills, effects)

    write_datafile("skills.json", skills)
    write_datafile("magic_effects.json", effects)

    reviews = {f"skills/{r['id']}": r["review"]
               for r in skills if "review" in r}
    reviews.update({f"magic_effects/{r['id']}": r["review"]
                    for r in effects if "review" in r})
    coverage("skills-effects", {
        "skills": {"records": len(skills),
                   "by_source_version": by_version(skills)},
        "magic_effects": {"records": len(effects),
                          "by_source_version": by_version(effects)},
        "review_count": len(reviews),
        "reviews": reviews,
        "gaps": GAPS,
        "notes": NOTES,
    })
    print(f"skills: {len(skills)}  magic_effects: {len(effects)}  "
          f"reviews: {len(reviews)}")


if __name__ == "__main__":
    main()
