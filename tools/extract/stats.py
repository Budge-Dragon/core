#!/usr/bin/env python3
"""Extract the pre-S3 stat catalog from OpenMU's Stats.cs.

Outputs:
  data/stats.json                  spec section 1 records
  tools/extract/stat_map.json      OpenMU property name (or inline attribute
                                   designation) -> mu-core slug
  data/_coverage/stats.json        counts, review list, named gaps

Curation policy: include the pre-S3 catalogue from
docs/reference/openmu-facts/0_*.md plus the approved 1.0-era backport needs
(leadership/DL, combo, soul barrier, nova, ancient option targets); exclude
shield/SD, master tree, Summoner, Rage Fighter, sockets/harmony, trainable
pets. Every Stats.cs property must be either included or listed in EXCLUDED
with a reason -- unknown upstream additions make the script fail.

source_version is computed from where the pre-S3 initializers wire the stat
("075" set = Version075 plus the shared initializers it uses, "095d" set =
Version095d plus MG/excellent-options). Stats wired by neither set must have
a MANUAL entry (s6 backports, engine flags), so nothing is decided silently.
"""

import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import coverage, slugify, write_datafile, STAT_MAP_PATH

OPENMU = "/tmp/openmu-ref/src"
STATS_CS = os.path.join(OPENMU, "GameLogic/Attributes/Stats.cs")
INIT = os.path.join(OPENMU, "Persistence/Initialization")

# Files that make up each pre-S3 dataset (shared bases included only where the
# 075/095d initializers actually inherit or call them).
V075_PATHS = [
    "Version075",
    "GameConfigurationInitializerBase.cs",
    "CharacterClasses/CharacterClassInitialization.cs",
    "CharacterClasses/CharacterClassHelper.cs",
    "CharacterClasses/ClassDarkWizard.cs",
    "CharacterClasses/ClassDarkKnight.cs",
    "CharacterClasses/ClassFairyElf.cs",
    "Items/ArmorInitializerBase.cs",
    "Skills/SkillsInitializerBase.cs",
    "Skills/DefenseEffectInitializer.cs",
    "Skills/GreaterDamageEffectInitializer.cs",
    "Skills/GreaterDefenseEffectInitializer.cs",
    "Skills/HealEffectInitializer.cs",
    "Skills/AlcoholEffectInitializer.cs",
]
V095D_PATHS = [
    "Version095d",
    "CharacterClasses/ClassMagicGladiator.cs",
    "Items/ExcellentOptions.cs",
]

# PropertyName -> scope.  base = persisted per-character stat, derived =
# computed/aggregated, resource = current/maximum pair, flag = 0/1 state,
# intermediate = formula scratch attribute.
INCLUDE = {
    # base
    "BaseStrength": "base", "BaseAgility": "base", "BaseVitality": "base",
    "BaseEnergy": "base", "BaseLeadership": "base",
    "Level": "base", "PointsPerLevelUp": "base", "AmmunitionAmount": "base",
    # resources
    "CurrentHealth": "resource", "MaximumHealth": "resource",
    "CurrentMana": "resource", "MaximumMana": "resource",
    "CurrentAbility": "resource", "MaximumAbility": "resource",
    # flags
    "IsInSafezone": "flag", "IsShieldEquipped": "flag",
    "IsBowEquipped": "flag", "IsCrossBowEquipped": "flag",
    "IsOneHandedSwordEquipped": "flag",
    "IsOneHandedStaffEquipped": "flag", "IsTwoHandedStaffEquipped": "flag",
    "IsTwoHandedWeaponEquipped": "flag", "IsScepterEquipped": "flag",
    "AreTwoWeaponsEquipped": "flag", "HasDoubleWield": "flag",
    "IsIced": "flag", "IsFrozen": "flag", "IsPoisoned": "flag",
    "IsStunned": "flag", "IsDinorantEquipped": "flag", "CanFly": "flag",
    "IsInvisible": "flag", "IsSkillComboAvailable": "flag",
    "GainHeroStatusQuestCompleted": "flag", "IsUnderwater": "flag",
    # intermediates (formula helpers)
    "TotalStrengthAndAgility": "intermediate",
    "EquippedWeaponCount": "intermediate",
    "DoubleWieldWeaponCount": "intermediate",
    "MeleeAttackMode": "intermediate", "ArcheryAttackMode": "intermediate",
    # totals & progression
    "TotalStrength": "derived", "TotalAgility": "derived",
    "TotalVitality": "derived", "TotalEnergy": "derived",
    "TotalLeadership": "derived", "TotalLevel": "derived",
    "TotalStrengthRequirementValue": "derived",
    "TotalAgilityRequirementValue": "derived",
    "TotalVitalityRequirementValue": "derived",
    "TotalEnergyRequirementValue": "derived",
    "TotalLeadershipRequirementValue": "derived",
    "ExperienceRate": "derived",
    "RandomExperienceMinMultiplier": "derived",
    "RandomExperienceMaxMultiplier": "derived",
    "MoneyAmountRate": "derived",
    # hit rates
    "AttackRatePvm": "derived", "AttackRatePvp": "derived",
    "DefenseRatePvm": "derived", "DefenseRatePvp": "derived",
    # offense
    "MinimumPhysBaseDmgByWeapon": "derived", "MaximumPhysBaseDmgByWeapon": "derived",
    "MinPhysBaseDmgByRightWeapon": "derived", "MaxPhysBaseDmgByRightWeapon": "derived",
    "MinimumPhysBaseDmg": "derived", "MaximumPhysBaseDmg": "derived",
    "PhysicalBaseDmg": "derived",
    "MinimumWizBaseDmg": "derived", "MaximumWizBaseDmg": "derived",
    "WizardryBaseDmg": "derived", "StaffRise": "derived", "ScepterRise": "derived",
    "BaseDamageBonus": "derived", "BaseMinDamageBonus": "derived",
    "BaseMaxDamageBonus": "derived", "FinalDamageBonus": "derived",
    "SkillMultiplier": "derived", "SkillDamageBonus": "derived",
    "CriticalDamageBonus": "derived", "CriticalDamageChance": "derived",
    "ExcellentDamageBonus": "derived", "ExcellentDamageChance": "derived",
    "DoubleDamageChance": "derived", "DefenseIgnoreChance": "derived",
    "StunChance": "derived", "GreaterDamageBonus": "derived",
    "ComboBonus": "derived", "BonusDamageWithScepter": "derived",
    # speed
    "AttackSpeedAny": "derived", "AttackSpeed": "derived",
    "AttackSpeedByWeapon": "derived", "MagicSpeed": "derived",
    "WalkSpeed": "derived", "MovementSpeed": "derived",
    "MovementSpeedUnderwater": "derived",
    "MovementSpeedFactor": "derived",
    # damage multipliers
    "WizardryBaseDmgIncrease": "derived", "PhysicalBaseDmgIncrease": "derived",
    "AttackDamageIncrease": "derived", "WizardryAttackDamageIncrease": "derived",
    "TwoHandedWeaponDamageIncrease": "derived",
    # elf modes & ammunition
    "MeleeMinDmg": "derived", "MeleeMaxDmg": "derived",
    "ArcheryMinDmg": "derived", "ArcheryMaxDmg": "derived",
    "AmmunitionConsumptionRate": "derived", "AmmunitionDamageBonus": "derived",
    # defense
    "DefenseBase": "derived", "DefenseFinal": "derived",
    "DefensePvm": "derived", "DefensePvp": "derived", "DefenseShield": "derived",
    "DamageReceiveDecrement": "derived", "ArmorDamageDecrease": "derived",
    "DamageReflection": "derived", "DefenseIncreaseWithEquippedShield": "derived",
    "DefenseDecrement": "derived",
    "SoulBarrierReceiveDecrement": "derived", "SoulBarrierManaTollPerHit": "derived",
    # elements
    "IceResistance": "derived", "FireResistance": "derived",
    "WaterResistance": "derived", "EarthResistance": "derived",
    "WindResistance": "derived", "PoisonResistance": "derived",
    "LightningResistance": "derived",
    "IceDamageBonus": "derived", "FireDamageBonus": "derived",
    "WaterDamageBonus": "derived", "EarthDamageBonus": "derived",
    "WindDamageBonus": "derived", "PoisonDamageBonus": "derived",
    "LightningDamageBonus": "derived",
    # recovery
    "HealthRecoveryMultiplier": "derived", "ManaRecoveryMultiplier": "derived",
    "AbilityRecoveryMultiplier": "derived",
    "HealthRecoveryAbsolute": "derived", "ManaRecoveryAbsolute": "derived",
    "AbilityRecoveryAbsolute": "derived",
    "HealthAfterMonsterKillMultiplier": "derived",
    "ManaAfterMonsterKillMultiplier": "derived",
    "PoisonDamageMultiplier": "derived",
    # misc
    "ItemDurationIncrease": "derived", "MaximumGuildSize": "derived",
    "TransformationSkin": "derived", "NovaStageDamage": "derived",
}

# The spec references requirement stats without the "Value" suffix (section 3).
SLUG_OVERRIDES = {
    "TotalStrengthRequirementValue": "total_strength_requirement",
    "TotalAgilityRequirementValue": "total_agility_requirement",
    "TotalVitalityRequirementValue": "total_vitality_requirement",
    "TotalEnergyRequirementValue": "total_energy_requirement",
    "TotalLeadershipRequirementValue": "total_leadership_requirement",
}

# Stats not wired by the 075/095d initializer sets: version + mandatory review.
MANUAL = {
    "BaseLeadership": ("s6", "dark lord backport (~1.0): leadership base stat"),
    "TotalLeadership": ("s6", "dark lord backport (~1.0): leadership total"),
    "TotalLeadershipRequirementValue": ("s6", "dark lord backport: leadership item requirement"),
    "ScepterRise": ("s6", "dark lord backport: scepter rise item stat"),
    "IsScepterEquipped": ("s6", "dark lord backport: scepter equip flag"),
    "BonusDamageWithScepter": ("s6", "dark lord backport: scepter damage bonus (non-MST part)"),
    "SoulBarrierReceiveDecrement": ("s6", "soul barrier backport (1.0-era soul master skill)"),
    "SoulBarrierManaTollPerHit": ("s6", "soul barrier backport (1.0-era soul master skill)"),
    "NovaStageDamage": ("s6", "nova backport (0.97/1.0-era skill)"),
    "IsSkillComboAvailable": ("s6", "dk combo backport (~0.98-1.0); unlocked by the hero-status quest"),
    "GainHeroStatusQuestCompleted": ("s6", "1.0-era hero-status quest reward flag; legacy quests deferred"),
    "IsStunned": ("s6", "stun state; pre-S3 source is the earthshake backport, OpenMU wires stun only in S6"),
    "StunChance": ("s6", "stun chance; pre-S3 source is the earthshake backport, OpenMU wires stun only in S6"),
    "FinalDamageBonus": ("s6", "ancient set option target (ancient sets pending decision 5)"),
    "SkillDamageBonus": ("s6", "ancient set option target (pending decision 5); other sources are post-S3"),
    "ExcellentDamageBonus": ("s6", "ancient set option target (pending decision 5)"),
    "CriticalDamageBonus": ("s6", "fed by DL critical-damage-increase skill backport and ancient sets"),
    "DoubleDamageChance": ("s6", "ancient set option target (pending decision 5); other source is sockets"),
    "DefenseIgnoreChance": ("s6", "ancient set option target (pending decision 5); also fed by the backported 2nd-wing option"),
    "TwoHandedWeaponDamageIncrease": ("s6", "ancient option target (pending decision 5)"),
    "IceDamageBonus": ("s6", "ancient jewelry damage bonus (S1-era jewelry, pending decision 5)"),
    "FireDamageBonus": ("s6", "ancient jewelry damage bonus (S1-era jewelry, pending decision 5)"),
    "WaterDamageBonus": ("s6", "ancient jewelry damage bonus (S1-era jewelry, pending decision 5)"),
    "EarthDamageBonus": ("s6", "ancient jewelry damage bonus (S1-era jewelry, pending decision 5)"),
    "WindDamageBonus": ("s6", "ancient jewelry damage bonus (S1-era jewelry, pending decision 5)"),
    "PoisonDamageBonus": ("s6", "ancient jewelry damage bonus (S1-era jewelry, pending decision 5)"),
    "LightningDamageBonus": ("s6", "ancient jewelry damage bonus (S1-era jewelry, pending decision 5)"),
    "IsInvisible": ("075", "engine-state flag (GM hide); not wired by pre-S3 data"),
    "HealthRecoveryAbsolute": ("075", "engine regeneration term (current += mult*max + absolute); no pre-S3 data feeds it"),
    "ManaRecoveryAbsolute": ("075", "engine regeneration term (current += mult*max + absolute); no pre-S3 data feeds it"),
    "IsUnderwater": ("075", "spec section 11 atlans power-up; OpenMU applies underwater state at runtime only"),
}

# Auto-versioned but era-doubtful: extra review notes.
REVIEW_EXTRA = {
    "ComboBonus": "OpenMU ships the DK combo formula in its 075 dataset; historically combo arrived ~0.98-1.0",
}

# Everything else in Stats.cs must be here, with a reason (coverage gaps).
_MST = "master tree (S4+)"
_SHIELD = "shield/SD system (classic PvP instead)"
_SUMMONER = "summoner class (S3)"
_RF = "rage fighter class (S5)"
_PETS = "trainable pets excluded wholesale (dark raven/horse, fenrir)"
EXCLUDED = {
    "MasterPointsPerLevelUp": _MST, "MasterLevel": _MST, "MasterExperienceRate": _MST,
    "MinWizardryAndCurseDmgBonus": _MST, "WizardryAndCurseBaseDmgBonus": _MST,
    "BowStrBonusDamage": _MST, "CrossBowStrBonusDamage": _MST,
    "CrossBowMasteryBonusDamage": _MST, "OneHandedSwordBonusDamage": _MST,
    "WeaponMasteryAttackSpeed": _MST, "TwoHandedSwordStrBonusDamage": _MST,
    "TwoHandedSwordMasteryBonusDamage": _MST, "MaceBonusDamage": _MST,
    "SpearBonusDamage": _MST, "MasterSkillPhysBonusDmg": _MST,
    "OneHandedStaffBonusBaseDamage": _MST, "TwoHandedStaffBonusBaseDamage": _MST,
    "TwoHandedStaffMasteryBonusDamage": _MST, "BookBonusBaseDamage": _MST,
    "StickBonusBaseDamage": _MST, "StickMasteryBonusDamage": _MST,
    "ExplosionBonusDmg": _MST, "RequiemBonusDmg": _MST, "PollutionBonusDmg": _MST,
    "ScepterStrBonusDamage": _MST, "ScepterMasteryBonusDamage": _MST,
    "ScepterPetBonusDamage": _MST, "GloveWeaponBonusDamage": _MST,
    "BonusDefenseWithShield": _MST, "BonusDefenseRateWithShield": _MST,
    "BonusDamageWithScepterCmdDiv": _MST, "BonusDefenseWithHorse": _MST,
    "RavenBonusDamage": _MST, "PollutionMoveTargetChance": _MST,
    "CurrentShield": _SHIELD, "MaximumShield": _SHIELD, "MaximumShieldTemp": _SHIELD,
    "ShieldRecoveryMultiplier": _SHIELD, "ShieldRecoveryAbsolute": _SHIELD,
    "ShieldRecoveryEverywhere": _SHIELD, "ShieldAfterMonsterKillMultiplier": _SHIELD,
    "ShieldAfterMonsterKillAbsolute": _SHIELD, "ShieldBypassChance": _SHIELD,
    "ShieldDecreaseRateIncrease": _SHIELD, "ShieldRateIncrease": _SHIELD,
    "ShieldItemDefenseIncrease": "water socket option (S4); classes agent must drop the common relationship that reads it",
    "BookRise": _SUMMONER, "MinimumCurseBaseDmg": _SUMMONER,
    "MaximumCurseBaseDmg": _SUMMONER, "CurseBaseDmg": _SUMMONER,
    "CurseAttackDamageIncrease": _SUMMONER, "IsStickEquipped": _SUMMONER,
    "IsBookEquipped": _SUMMONER, "BerserkerManaMultiplier": _SUMMONER,
    "BerserkerHealthDecrement": _SUMMONER, "BerserkerCurseMultiplier": _SUMMONER,
    "BerserkerWizardryMultiplier": _SUMMONER, "BerserkerProficiencyMultiplier": _SUMMONER,
    "BerserkerMinPhysDmgBonus": _SUMMONER, "BerserkerMaxPhysDmgBonus": _SUMMONER,
    "BerserkerMinWizDmgBonus": _SUMMONER, "BerserkerMaxWizDmgBonus": _SUMMONER,
    "BerserkerMinCurseDmgBonus": _SUMMONER, "BerserkerMaxCurseDmgBonus": _SUMMONER,
    "WeaknessPhysDmgDecrement": _SUMMONER + " / RF killing blow",
    "InnovationDefDecrement": _SUMMONER,
    "IsAsleep": _SUMMONER,
    "IsBleeding": _SUMMONER + " (explosion/requiem books)",
    "BleedingDamageMultiplier": _SUMMONER + " (explosion/requiem books)",
    "SummonedMonsterHealthIncrease": _SUMMONER, "SummonedMonsterDefenseIncrease": _SUMMONER,
    "VitalitySkillMultiplier": _RF, "IsGloveWeaponEquipped": _RF,
    "FenrirBaseDmg": _PETS, "HorseLevel": _PETS, "RavenLevel": _PETS,
    "RavenMinimumDamage": _PETS, "RavenMaximumDamage": _PETS,
    "RavenAttackRate": _PETS, "RavenAttackSpeed": _PETS,
    "RavenCriticalDamageChance": _PETS, "RavenExcDamageChance": _PETS,
    "RavenAttackDamageIncrease": _PETS,
    "DamageReceiveHorseDecrement": _PETS + "; DL class backport drops horse relationships",
    "IsHorseEquipped": _PETS + "; DL class backport drops horse relationships",
    "PetDurationIncrease": _PETS + " (DL const value dropped)",
    "IsPetSkeletonEquipped": "S6 transformation pet",
    "MoonstonePendantEquipped": "kanturu event (S3)",
    "IsMuHelperActive": "mu helper (S9)",
    "IsVip": "vip accounts (custom/post-S3)",
    "PointsPerReset": "reset system (custom server feature)",
    "Resets": "reset system (custom server feature)",
    "BonusExperienceRate": "fed only by S6 transformation rings/pets; exp rate knobs live in game_constants",
    "MaximumAllianceSize": "guild alliances, S6-only value (decision 5 open)",
    "FullyRecoverManaAfterHitChance": "3rd wing option (post-S3)",
    "FullyRecoverHealthAfterHitChance": "3rd wing option (post-S3)",
    "FullyReflectDamageAfterHitChance": "3rd wing option (post-S3)",
    "SkillBaseMultiplier": "S6 AreaSkillSettings skill-attribute quartet",
    "SkillBaseDamageBonus": "S6 AreaSkillSettings skill-attribute quartet",
    "SkillFinalMultiplier": "S6 AreaSkillSettings skill-attribute quartet",
    "SkillFinalDamageBonus": "S6 AreaSkillSettings skill-attribute quartet",
    "SkillLevel": "master-skill level attribute; skills do not level pre-S3",
    "SkillExtraManaCost": "infinite arrow effect (S2+ muse elf), not in curated backports",
    "HealthLossAfterHit": "wing HP toll; wired by no OpenMU initializer (dead data)",
    "NearbyPartyMemberCount": "wired only by S6 skill settings",
    "ManaUsageReduction": "wired only by S6 skill settings",
    "AbilityUsageReduction": "wired only by socket options (S4)",
    "RequiredStrengthReduction": "harmony options only (S4)",
    "RequiredAgilityReduction": "harmony options only (S4)",
    "RequiredEnergyReduction": "harmony options only (S4)",
    "RequiredVitalityReduction": "harmony options only (S4)",
    "RequiredLeadershipReduction": "harmony options only (S4)",
    "FinalDamageIncreasePvp": "wired only by guardian/380 options (post-S3), not by 2nd wings",
    "IsTwoHandedSwordEquipped": "per-weapon-class flag set only by S6 weapon data (feeds MST buckets)",
    "IsMaceEquipped": "per-weapon-class flag set only by S6 weapon data (feeds MST buckets)",
    "IsSpearEquipped": "per-weapon-class flag set only by S6 weapon data (feeds MST buckets)",
    "AbilityAfterMonsterKillMultiplier": "after-kill regeneration slot wired by no dataset (dead data)",
    "AbilityAfterMonsterKillAbsolute": "after-kill regeneration slot wired by no dataset (dead data)",
    "HealthAfterMonsterKillAbsolute": "after-kill regeneration slot wired by no dataset (dead data)",
    "ManaAfterMonsterKillAbsolute": "after-kill regeneration slot wired by no dataset (dead data)",
}

# Inline (anonymous) attributes created by the CharacterClasses initializers.
# key = designation string in the C# source, verified against the files.
INLINE = [
    ("Temp Half weapon attack speed", "CharacterClasses/CharacterClassInitialization.cs", "075", None),
    ("Temp Defense Bonus multiplier with Shield", "CharacterClasses/CharacterClassInitialization.cs", "075", None),
    ("Temp Double Wield multiplier", "CharacterClasses/CharacterClassInitialization.cs", "075", None),
    ("Ammunition damage increase", "CharacterClasses/ClassFairyElf.cs", "075", None),
    ("TotalEnergy minus 15", "CharacterClasses/ClassDarkLord.cs", "s6",
     "dark lord backport (~1.0): mana formula intermediate"),
]
# Inline attrs deliberately not carried over (they belong to excluded systems).
INLINE_EXCLUDED = {
    "Temp Innovation defense decrement": _SUMMONER,
    "Stats defense": _SUMMONER,
    "Stats min wiz and curse base dmg": _SUMMONER,
    "Stats max wiz and curse base dmg": _SUMMONER,
    "Min Berserker health decrement": _SUMMONER,
    "Final Berserker health decrement": _SUMMONER,
}

PROP_RE = re.compile(r"public static AttributeDefinition (\w+) \{ get; \}")
MAX_RE = re.compile(r"MaximumValue = ([0-9.]+)f?\s*,")


def parse_stats_cs():
    """-> {property_name: max_value_or_None}, in source order."""
    text = open(STATS_CS, encoding="utf-8").read()
    props = {}
    matches = list(PROP_RE.finditer(text))
    for i, m in enumerate(matches):
        end = matches[i + 1].start() if i + 1 < len(matches) else len(text)
        # only look inside this property's initializer block
        block = text[m.start():end]
        maxm = MAX_RE.search(block)
        props[m.group(1)] = float(maxm.group(1)) if maxm else None
    return props


def read_tree(paths):
    chunks = []
    for rel in paths:
        full = os.path.join(INIT, rel)
        if os.path.isdir(full):
            for root, _dirs, files in os.walk(full):
                for name in files:
                    if name.endswith(".cs"):
                        chunks.append(open(os.path.join(root, name), encoding="utf-8").read())
        else:
            chunks.append(open(full, encoding="utf-8").read())
    return "\n".join(chunks)


def main():
    props = parse_stats_cs()

    unknown = [p for p in props if p not in INCLUDE and p not in EXCLUDED]
    if unknown:
        sys.exit("Stats.cs properties not curated (add to INCLUDE or EXCLUDED): %s" % ", ".join(unknown))
    missing = [p for p in list(INCLUDE) + list(EXCLUDED) if p not in props]
    if missing:
        sys.exit("curated names not found in Stats.cs: %s" % ", ".join(missing))

    blob_075 = read_tree(V075_PATHS)
    blob_095d = read_tree(V095D_PATHS)

    def version_of(name):
        if name in MANUAL:
            return MANUAL[name]
        if re.search(r"Stats\.%s\b" % name, blob_075):
            return "075", REVIEW_EXTRA.get(name)
        if re.search(r"Stats\.%s\b" % name, blob_095d):
            return "095d", REVIEW_EXTRA.get(name)
        return None

    records, stat_map, unresolved = [], {}, []
    for name in props:  # keep Stats.cs source order
        if name in EXCLUDED:
            continue
        resolved = version_of(name)
        if resolved is None:
            unresolved.append(name)
            continue
        version, review = resolved
        slug = SLUG_OVERRIDES.get(name, slugify(name))
        rec = {"id": slug}
        if props[name] is not None:
            rec["max_value"] = props[name]
        rec["scope"] = INCLUDE[name]
        rec["source_version"] = version
        if review:
            rec["review"] = review
        records.append(rec)
        stat_map[name] = slug

    if unresolved:
        sys.exit("stats wired by neither pre-S3 set and lacking a MANUAL entry: %s" % ", ".join(unresolved))

    for designation, rel_file, version, review in INLINE:
        source = open(os.path.join(INIT, rel_file), encoding="utf-8").read()
        if '"%s"' % designation not in source:
            sys.exit("inline attribute %r not found in %s" % (designation, rel_file))
        slug = slugify(designation)
        rec = {"id": slug, "scope": "intermediate", "source_version": version}
        if review:
            rec["review"] = review
        records.append(rec)
        stat_map[designation] = slug

    out_path = write_datafile("stats.json", records)
    import json
    with open(STAT_MAP_PATH, "w", encoding="utf-8") as f:
        json.dump(stat_map, f, indent=2, ensure_ascii=False)
        f.write("\n")

    by_version = {}
    for rec in records:
        by_version[rec["source_version"]] = by_version.get(rec["source_version"], 0) + 1
    reviews = {r["id"]: r["review"] for r in records if "review" in r}
    gaps = {slugify(SLUG_OVERRIDES.get(n, n)): reason for n, reason in sorted(EXCLUDED.items())}
    gaps.update({slugify(d): reason for d, reason in INLINE_EXCLUDED.items()})
    cov = {
        "records": len(records),
        "by_source_version": by_version,
        "review_count": len(reviews),
        "reviews": reviews,
        "gaps": gaps,
    }
    cov_path = coverage("stats", cov)

    print("wrote %s (%d records: %s)" % (out_path, len(records), by_version))
    print("wrote %s (%d entries)" % (STAT_MAP_PATH, len(stat_map)))
    print("wrote %s (%d gaps, %d reviews)" % (cov_path, len(gaps), len(reviews)))


if __name__ == "__main__":
    main()
