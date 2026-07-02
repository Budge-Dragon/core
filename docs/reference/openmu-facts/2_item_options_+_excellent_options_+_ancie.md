# item options + excellent options + ancient sets

## Counts
- ItemOptionTypes catalog: 14 option types total (Season 6 uses all 14)
- Version075 option types enabled: 2 (Option, Luck)
- Version095d option types enabled: 3 (Option, Luck, Excellent)
- Excellent option groups: 3 active (Defense, Physical, Wizardry) x 6 options each = 18; Curse group defined but disabled (S14+)
- Normal option definitions (all versions): 5 (Luck, Defense, PhysicalAttack, WizardryAttack, DefenseRate) + jewelry health option; S6 adds 2 more (Phys+Wiz combined, Curse)
- VersionSeasonSix ancient sets: 36 sets (numbers 1..36), 2 per armor set family, discriminators 1 and 2
- Ancient bonus option levels: 2 (level 1 = +5, level 2 = +10)
- Version075 wings with wing options: 3; Version095d wings: 3; SeasonSix wings/capes: 15
- ItemOptionDefinitionNumbers: 38 catalog numbers

## Entities

### item option type (kind of option)
- A closed set of option kinds defined in src/DataModel/Configuration/Items/ItemOptionTypes.cs; each has: Name (string), Description (string), IsVisible (bool = whether other players see it, true for Excellent, AncientOption, and the fenrir/horse pet types).
- Full list (14): Excellent, Wing, Luck, Option (= normal option), HarmonyOption, AncientOption, AncientBonus, GuardianOption, SocketOption, SocketBonusOption, BlueFenrir, BlackFenrir, GoldFenrir, DarkHorse.
- Luck semantics: 'Luck (Critical Damage Chance 5%)' — fixed CriticalDamageChance +0.05, no levels; also (game-logic elsewhere) affects jewel success rates.
- Option semantics: the '+4/+8/...' normal item option, increased by Jewel of Life up to option level 4.
- Skill is NOT an option type: it is ItemDefinition.Skill (a skill reference) plus a per-item-instance bool HasSkill (src/DataModel/Entities/Item.cs).
- GuardianOption description: 'added by the chaos machine with a jewel of guardian on level 380 items' (Season 3+).
- Wing type is used only for 2nd/3rd-wing bonus ('excellent wing') options and cape command option — in the SeasonSix dataset only.

### item option definition (option group attachable to items)
- File: src/DataModel/Configuration/Items/ItemOptionDefinition.cs.
- Name: string (e.g. 'Luck', 'Defense Option', 'Excellent Defense Options', '<wing name> Options', '<set name> (Ancient Set)').
- AddsRandomly: bool — whether this group can roll onto a dropped item.
- AddChance: float 0..1 — roll chance when AddsRandomly (normal/luck = 0.25, excellent = 0.001, dinorant = 0.3, S6 wing options = 0.1).
- MaximumOptionsPerItem: int — max options from this group on one item (1 for normal/luck/harmony, 2 for excellent).
- PossibleOptions: collection of increasable item options (see below).
- Attachment: ItemDefinition.PossibleItemOptions is a collection of ItemOptionDefinition references (ItemDefinition.cs line 141). Weapons get Luck + Physical-damage-option (magicPower==0) or Wizardry-damage-option (staffs); armor gets Luck + Defense-option; shields get Luck + DefenseRate-option (no defense option); jewelry gets health-recover option and (095d+) excellent groups; wings get their own per-wing option definition + Luck.

### item option (increasable item option)
- Files: ItemOption.cs + IncreasableItemOption.cs.
- Number: int — index inside the group; combined with the option type it is the client wire encoding (excellent options numbered 1..6, ancient set options 1..n, socket options 0..n).
- SubOptionType: int — sub-discriminator; only used for socket options (element fire/water/ice/wind/lightning/earth). Post-S3.
- OptionType: reference to an item option type (above).
- PowerUpDefinition: target attribute (stat reference) + boost = constant value (float) + aggregate type enum {AddRaw, Multiplicate, AddFinal, Maximum} and/or related values (attribute relationship: target += inputAttribute * operand, e.g. exc 'dmg +level/20').
- LevelType: enum {OptionLevel (default; option leveled by jewels, e.g. Jewel of Life), ItemLevel (option value follows the item's +level; used only by S6 2nd-wing '+50~125' options)}.
- Weight: byte — statistical weight for random rollout; only used by harmony options (post-S3): 40/30/20/10 tiers, 50 for harmony defense.
- LevelDependentOptions: collection of option-of-level entries; if empty the flat PowerUpDefinition applies.

### option of level (ItemOptionOfLevel)
- File: ItemOptionOfLevel.cs.
- Level: int — the option level this row applies to (normal options 1..4; ancient bonus 1..2; harmony 1..n).
- RequiredItemLevel: int — minimum item (+) level for the row to be active; only harmony uses it (option stays on item but is inactive if item level < required).
- PowerUpDefinition: same shape as above (attribute + value + aggregate type).
- Application rule (game logic): effective level = item.Level when LevelType==ItemLevel else stored option level; pick LevelDependentOptions row with that Level, fall back to the flat PowerUpDefinition.

### item instance option link (ItemOptionLink)
- File: src/DataModel/Entities/ItemOptionLink.cs — how a concrete item stores its rolled options.
- ItemOption: reference into Definition.PossibleItemOptions[..].PossibleOptions.
- Level: int — current option level (normal option 1..4; ancient bonus 1..2).
- Index: int — ordering slot, needed only for socket options (post-S3).
- Related item-instance fields: HasSkill bool, SocketCount int (post-S3), ItemSetGroups: collection of item-of-item-set references (ancient membership of this concrete item).

### normal (standard) options — concrete values
- Defined in GameConfigurationInitializerBase.cs for all versions: Luck def (number 0x01): CriticalDamageChance +0.05, AddChance 0.25, max 1/item, no levels.
- Defense option (0x02): DefenseBase +4 per option level; level rows 2..4 hold value = level*4 (4/8/12/16).
- Physical attack option (0x03): PhysicalBaseDmg +4 per level (4/8/12/16).
- Wizardry attack option (0x04): WizardryBaseDmg +4 per level (4/8/12/16).
- Defense rate option (0x06, shields): DefenseRatePvm +5 per level (5/10/15/20).
- All: AddChance 0.25, AddsRandomly true, MaximumOptionsPerItem 1, option-level range 1..4 (MaximumOptionLevel = 4); dropped items capped at option level 3 (GameConfiguration.MaximumItemOptionLevelDrop = 3); level raised by Jewel of Life.
- Jewelry health-recover option (0x10): HealthRecoveryMultiplier +0.01 per level, max option level 3 (Jewelery initializer overrides MaximumOptionLevel=3), applies to rings/pendants with withHealthOption.
- Season-6-only additional normal options: PhysicalAndWizardryAttack (0x07, BaseDamageBonus, for MG magic swords) and CurseAttack (0x05, CurseBaseDmg, Summoner) — created only in the S6 GameConfigurationInitializer.
- Wing options (075/095d): each wing has its own single-option definition of type Option, AddChance 0.25, max 1: Wings of Elf = HealthRecoveryMultiplier +0.01/level; Wings of Heaven = WizardryBaseDmg +4/level; Wings of Satan = PhysicalBaseDmg +4/level; option level 1..4 via Jewel of Life.

### excellent options — concrete values (src/Persistence/Initialization/Items/ExcellentOptions.cs, shared by 095d and S6)
- Three groups, each: AddChance 0.001, AddsRandomly true, MaximumOptionsPerItem 2, option numbers 1..6, no level progression (flat values).
- Excellent Defense Options (def number 0x12), attached to armor/shields/rings: 1 = MoneyAmountRate x1.4 (Zen +40%); 2 = DefenseRatePvm x1.1 (+10%); 3 = DamageReflection +0.05; 4 = ArmorDamageDecrease +0.04 (DD 4%); 5 = MaximumMana x1.04; 6 = MaximumHealth x1.04.
- Excellent Physical Attack Options (0x13), attached to physical weapons/pendants: 1 = ManaAfterMonsterKillMultiplier +1/8 (mana/8 per kill); 2 = HealthAfterMonsterKillMultiplier +1/8 (HP/8 per kill); 3 = AttackSpeedAny +7; 4 = PhysicalBaseDmgIncrease x1.02 (+2%); 5 = PhysicalBaseDmg += TotalLevel/20 (relationship: level/20); 6 = ExcellentDamageChance +0.1.
- Excellent Wizardry Attack Options (0x14), attached to staffs/wizard pendants: same six with WizardryBaseDmgIncrease x1.02 and WizardryBaseDmg += level/20.
- Curse attack group (0x15) exists in code but is disabled/obsolete: no curse excellent options until Season 14; excellent curse books use wizardry options. (Correct for pre-S3.)
- Drop mechanics: dedicated drop item group SpecialItemType.Excellent, chance 0.0001 (0.01%); item picked as if monster level reduced by GameConfiguration.ExcellentItemDropLevelDelta = 25; first excellent option is always added, each further slot up to MaximumOptionsPerItem(2) rolls with AddChance, no duplicate options.
- Excellent (and ancient) items also get IMPLICIT base bonuses (hardcoded formulas in GameLogic/ItemPowerUpFactory.cs, integer math): armor: DefenseBase += (baseDefense*12/dropLevel) + (dropLevel/5) + 4; shield: DefenseRatePvm += (baseRate*25/dropLevel) + 5; physical weapon: min&max dmg += (minDmg*25/dropLevel) + 5; staff: StaffRise += ((rise*2*25/dropLevel)+5)/2; where dropLevel = item definition drop level.
- Effective drop level formula (ItemExtensions.CalculateDropLevel): dropLevel = base + 30 if ancient, else +25 if excellent; plus 3 * itemLevel.

### ancient set (ItemSetGroup + ItemOfItemSet) — structure
- ItemSetGroup fields: Name; AlwaysApplies bool (set options apply without instance membership — used for generic set bonuses, false for ancients); CountDistinct bool (true for ancients — same item worn twice counts once; false enables e.g. double-wield same-sword bonus); MinimumItemCount int (2 for all ancients); SetLevel int (minimum +level of all pieces, 0 for ancients — used for non-ancient 'defense bonus at +9' style sets); Options -> one ItemOptionDefinition whose PossibleOptions are the set bonuses ordered by Number; Items -> collection of ItemOfItemSet.
- ItemOfItemSet fields: AncientSetDiscriminator int (0 = not ancient; ancients use 1 or 2 — protocol supports max two ancient sets per item definition, e.g. Warrior Leather = 1, Anonymous Leather = 2); ItemDefinition reference (group+number identify the piece); BonusOption reference (the per-piece ancient '+5/+10 stat' bonus).
- Set option application rule (ItemPowerUpFactory.GetSetPowerUps): count equipped pieces of the group (distinct by definition if CountDistinct, only pieces with level >= SetLevel); if count == total set size, ALL options apply; else if count >= MinimumItemCount, the first (count-1) options ordered by Number apply. So an ancient set with k of n pieces (k>=2) grants options 1..k-1; full set grants everything including the final 'full set' bonuses.
- Ancient bonus (AncientBonus type, def number 0x30): per-piece IncreasableItemOption keyed by stat; LevelDependentOptions: Level 1 = +5, Level 2 = +10 of that stat; on drop the level is chosen randomly from {1,2}; AddsRandomly false, MaximumOptionsPerItem 1; stored on the item as an option link with Level 1 or 2.
- Ancient membership on a concrete item = entry in Item.ItemSetGroups referencing the ItemOfItemSet.
- Ancient implicit bonuses (hardcoded, ItemPowerUpFactory, on top of the excellent implicit bonuses which ancients also get): armor extra DefenseBase += 2 + ((baseDefense+additionalDefense)*3/ancientDropLevel) + (ancientDropLevel/30); shield extra DefenseShield(ability/shield defense) += 2 + ((baseDefense+itemLevel)*20/ancientDropLevel); weapon min&max dmg += 5 + (ancientDropLevel/40); staff rise += (2 + ancientDropLevel/60)/2; ancientDropLevel = base drop level + 30.

### ancient sets — dataset (VersionSeasonSix/Items/AncientSets.cs; ancients are Season 1+, in scope)
- 36 sets, set numbers 1..36, each MinimumItemCount 2, CountDistinct true; set options are of type AncientOption (def number 0x29), numbered 1..n in listed order; per-piece bonus stat + discriminator (1 or 2) listed per item.
- 1 Warrior (Leather, disc 1, 7 pcs: boots/gloves/helm/pants/armor +Vit bonus, Morning Star +Str, Ring of Ice +Agi): opts = Str+10, SkillDmg+10, MaxAbility(AG)+20, AbilityRecovery+5, DefenseBase+20(AddFinal), Agi+10, CritDmgChance+5%, ExcDmgChance+5%, Str+25.
- 2 Anonymous (Leather, disc 2, 4 pcs: boots/helm/pants +Vit, Small Shield disc1 +Vit): MaxHP+50, Agi+50, DefenseIncreaseWithShield+25%, FinalDamageBonus(BaseDmg)+30.
- 3 Hyperion (Bronze, disc 1, 3 pcs boots/pants/armor +Vit): Energy+15, Agi+15, SkillDmg+20, MaxMana+30.
- 4 Mist (Bronze, disc 2 except gloves&helm disc1, 3 pcs gloves/pants/helm +Vit): Vit+20, SkillDmg+30, DoubleDmgChance+10%, Agi+20.
- 5 Eplete (Scale, disc 1, 5 pcs pants/armor/helm +Vit, Plate Shield +Vit, Pendant of Lightning +Ene): SkillDmg+15, AttackRatePvm+50, WizDmg x1.05, MaxHP+50, MaxAbility+30, CritDmgChance+10%, ExcDmgChance+10%.
- 6 Berserker (Scale, disc 2 for pants/armor/helm, disc 1 gloves/boots, 5 pcs +Vit): MaxPhysDmg +10/+20/+30/+40, SkillDmg+50, Str+40.
- 7 Garuda (Brass, disc 1, 5 pcs pants/armor/gloves/boots +Vit, Pendant of Fire +Str): MaxAbility+30, DoubleDmgChance+5%, Ene+15, MaxHP+50, SkillDmg+25, WizDmg x1.15.
- 8 Cloud (Brass, disc 2 pants / disc 1 helm, 2 pcs +Vit): CritDmgChance+20%, CritDmgBonus+50.
- 9 Kantata (Plate, disc 1, 5 pcs boots/gloves/armor +Vit, Ring of Wind +Agi, Ring of Poison +Vit): Ene+15, Vit+30, WizDmg x1.10, Str+15, SkillDmg+25, ExcDmgChance+10 (note: file has 10.0f not 0.10 — likely data bug), ExcDmgBonus+20.
- 10 Rave (Plate, disc 1 helm/pants disc 2 armor, 3 pcs +Vit): SkillDmg+20, DoubleDmgChance+10%, TwoHandedWeaponDmg+30%, DefenseIgnoreChance+5%.
- 11 Hyon (Dragon, disc 1, 4 pcs: Lightning Sword +Str, helm/boots/gloves +Vit): DefenseBase+25(AddFinal), DoubleDmgChance+10%, SkillDmg+20, CritDmgChance+15%, ExcDmgChance+15%, CritDmgBonus+20, ExcDmgBonus+20.
- 12 Vicious (Dragon, disc 2, 4 pcs: Ring of Earth +Str, helm/pants/armor +Vit): SkillDmg+15, FinalDmgBonus+15, DoubleDmgChance+10%, MinPhysDmg+20, MaxPhysDmg+30, DefenseIgnoreChance+5%.
- 13 Apollo (Pad, disc 1, 7 pcs: Skull Staff +Ene, helm/armor/pants/gloves +Vit, Pendant of Ice +Str, Ring of Magic +Ene): Ene+10, WizDmg x1.05, SkillDmg+10, MaxMana+30, MaxHP+30, MaxAbility+20, CritDmgChance+10%, ExcDmgChance+10%, Ene+30.
- 14 Barnake (Pad, disc 2, 3 pcs helm/pants +Vit disc2, boots disc1): WizDmg x1.10, Ene+20, SkillDmg+30, MaxMana+100.
- 15 Evis (Bone, disc 1, 4 pcs armor/pants/boots +Vit, Pendant of Wind +Agi): SkillDmg+15, Vit+20, WizDmg x1.10, DoubleDmgChance+5%, AttackRatePvm+50, AbilityRecovery+5.
- 16 Sylion (Bone, disc 2 armor/boots, disc 1 gloves/helm, 4 pcs +Vit): DoubleDmgChance+5%, CritDmgChance+5%, DefenseBase+20(AddFinal), Str+50, Agi+50, Vit+50, Ene+50.
- 17 Heras (Sphinx, disc 1, 6 pcs: Skull Shield +Vit, pants/armor/helm/gloves/boots +Vit): Str+15, WizDmg x1.10, DefWithShield+5%, Ene+15, AttackRatePvm+50, CritDmgChance+10%, ExcDmgChance+10%, MaxHP+50, MaxMana+50.
- 18 Minet (Sphinx, disc 2, 3 pcs pants/armor/boots +Vit): Ene+30, DefenseBase+30(AddFinal), MaxMana+100, SkillDmg+15.
- 19 Anubis (Legendary, disc 1, 4 pcs armor/helm/gloves +Vit, Ring of Fire +Ene): DoubleDmgChance+10%, MaxMana+50, WizDmg x1.10, CritDmgChance+15%, ExcDmgChance+15%, CritDmgBonus+20, ExcDmgBonus+20.
- 20 Enis (Legendary, disc 2, 4 pcs armor/helm/boots/pants +Vit): SkillDmg+10, DoubleDmgChance+10%, Ene+30, WizDmg x1.10, DefenseIgnoreChance+5%.
- 21 Ceto (Vine, disc 1, 6 pcs boots/gloves/helm/pants +Vit, Rapier +Str, Ring of Earth +Str): Agi+10, MaxHP+50, DefenseBase+20(AddFinal), DefWithShield+5%, Ene+10, MaxHP+50, Str+20.
- 22 Drake (Vine, disc 2 boots/helm/pants, armor disc 1, 4 pcs +Vit): Agi+20, FinalDmgBonus+25, DoubleDmgChance+20%, DefenseBase+40(AddFinal), CritDmgChance+10%.
- 23 Gaia (Silk, disc 1, 5 pcs armor/gloves/helm/pants +Vit, Golden Crossbow +Agi): SkillDmg+10, MaxMana+25, Str+10, DoubleDmgChance+5%, Agi+30, ExcDmgChance+10%, ExcDmgBonus+10.
- 24 Fase (Silk, disc 2 gloves/pants, boots disc 1, 3 pcs +Vit): MaxHP+100, MaxMana+100, DefenseBase+100(AddFinal).
- 25 Odin (Wind, disc 1, 5 pcs armor/gloves/helm/pants/boots +Vit): Ene+15, MaxHP+50, AttackRatePvm+50, Agi+30, MaxMana+50, DefenseIgnoreChance+5%, MaxAbility+50.
- 26 Elvian (Wind, disc 2, 2 pcs pants/boots +Vit): Agi+30, DefenseIgnoreChance+5%.
- 27 Argo (Spirit, disc 1, 3 pcs armor/gloves/pants +Vit): MaxPhysDmg+20, SkillDmg+25, MaxAbility+50, DoubleDmgChance+5%.
- 28 Karis (Spirit, disc 2, 3 pcs helm/boots/pants +Vit): SkillDmg+15, DoubleDmgChance+10%, CritDmgChance+10%, Agi+40.
- 29 Gywen (Guardian, disc 1, 5 pcs boots/gloves/armor +Vit, Silver Bow +Agi, Pendant of Ability no bonus stat): Agi+30, MinPhysDmg+20, DefenseBase+20(AddFinal), MaxPhysDmg+20, CritDmgChance+15%, ExcDmgChance+15%, CritDmgBonus+20, ExcDmgBonus+20.
- 30 Aruan (Guardian, disc 2, 4 pcs boots/pants/armor/helm +Vit): FinalDmgBonus+10, DoubleDmgChance+10%, SkillDmg+20, CritDmgChance+15%, ExcDmgChance+15%, DefenseIgnoreChance+5%.
- 31 Gaion (Storm Crow, disc 1, 4 pcs boots/pants/armor +Vit, Pendant of Water +Vit): DefenseIgnoreChance+5%, DoubleDmgChance+15%, SkillDmg+15, ExcDmgChance+15%, ExcDmgBonus+30, WizDmg x1.10, Str+30.
- 32 Muren (Storm Crow, disc 2, 4 pcs gloves/pants/armor +Vit, Ring of Fire +Vit): SkillDmg+10, WizDmg x1.10, DoubleDmgChance+10%, CritDmgChance+15%, ExcDmgChance+15%, DefenseBase+25(AddFinal), TwoHandedWeaponDmg+20%.
- 33 Agnis (Adamantine, disc 1, 4 pcs armor/pants/helm +Vit, Ring of Poison disc2 +Vit): DoubleDmgChance+10%, DefenseBase+40(AddFinal), SkillDmg+20, CritDmgChance+15%, ExcDmgChance+15%, CritDmgBonus+20, ExcDmgBonus+20.
- 34 Broy (Adamantine, disc 2, 4 pcs pants/gloves/boots +Vit, Pendant of Ice +Str): FinalDmgBonus+20, SkillDmg+20, Ene+30, CritDmgChance+15%, ExcDmgChance+15%, DefenseIgnoreChance+5%, Leadership+30.
- 35 Chrono (Red Wing, disc 1, 4 pcs helm/pants/gloves +Vit, Ring of Magic disc2 +Ene): DoubleDmgChance+20%, DefenseBase+60(AddFinal), SkillDmg+30, CritDmgChance+15%, ExcDmgChance+15%, CritDmgBonus+20, ExcDmgBonus+20.
- 36 Semeden (Red Wing, disc 2, 4 pcs boots/gloves/armor/helm +Vit): WizDmg x1.15, SkillDmg+25, Ene+30, CritDmgChance+15%, ExcDmgChance+15%, DefenseIgnoreChance+5%.
- Percent values are stored as fractions (0.05 = 5%); WizDmg multipliers use AggregateType Multiplicate; DefenseBase set options use AddFinal; everything else AddRaw.

### item option combination bonus
- Files: ItemOptionCombinationBonus.cs + CombinationBonusRequirement.cs.
- Fields: Description, Number int, AppliesMultipleTimes bool, Requirements (each: OptionType ref + SubOptionType int + MinimumCount int, default 1), Bonus = one power-up definition.
- Semantics: if the equipped items' options satisfy all requirements, grant the bonus; repeatable per matching set when AppliesMultipleTimes.
- Used only for socket 'package' bonuses (post-S3) and fenrir/horse movement-speed bonuses in the S6 dataset — not needed for a pre-S3 core, but the concept is generic.

### internal option definition numbers (ItemOptionDefinitionNumbers.cs — stable catalog ids, not wire protocol)
- Luck 0x01, DefenseOption 0x02, PhysicalAttack 0x03, WizardryAttack 0x04, CurseAttack 0x05 (S6), DefenseRateOption 0x06, PhysicalAndWizardryAttack 0x07 (S6, MG magic swords), JeweleryHealth 0x10.
- ExcellentDefense 0x12, ExcellentPhysical 0x13, ExcellentWizardry 0x14, ExcellentCurse 0x15 (unused pre-S14).
- DefenseSetBonusOption 0x20, DefenseRateSetBonusOption 0x21 (non-ancient set bonuses), AncientOption 0x29, AncientBonus 0x30.
- SocketBonus 0x31, SocketFire..SocketEarth 0x32..0x37 (post-S3), GuardianOption1 0x41, GuardianOption2 0x42 (post-S3), HarmonyDefense..HarmonyCurse 0x50..0x53 (post-S3).
- WingDefense 0x60, WingPhysical 0x61, WingWizardry 0x62, WingCurse 0x63, WingHealthRecover 0x64, Wing2nd 0x65, Cape 0x66, Wing3rd 0x67.
- Ring/pet definitions: EliteSkeletonTransformationRing 0x70, SkeletonTransformationRing 0x71, WizardRing 0x73, Dino 0x80, Fenrir 0x81, Horse 0x82.

### drop-related option knobs (GameConfiguration)
- ExcellentItemDropLevelDelta: byte = 25 — level subtracted for excellent drop item selection.
- MaximumItemOptionLevelDrop: byte = 3 — max normal option level on dropped items.
- Global excellent drop item group: chance 0.0001, only registered when the version supports the Excellent option type (so absent in Version075).
- MaximumOptionLevel (initializer constant): 4 = highest option level reachable (via Jewel of Life); jewelry health option max 3.

## Enums

### ItemOptionType (catalog, pre-S3 subset)
- Option (normal +4/+8/+12/+16)
- Luck (crit dmg chance +5%)
- Excellent
- Wing (2nd/3rd wing bonus options; S6 dataset only)
- AncientOption (set bonus)
- AncientBonus (per-piece +5/+10)
- DarkHorse
- BlueFenrir/BlackFenrir/GoldFenrir (S6 dataset)

### ItemOptionType (post-S3, excluded)
- HarmonyOption (Jewel of Harmony, S3)
- GuardianOption (380-level items, S3)
- SocketOption (S4)
- SocketBonusOption (S4)

### LevelType
- OptionLevel (default; leveled by jewels e.g. Jewel of Life)
- ItemLevel (value follows item +level; only S6 2nd-wing +50~125 options)

### AggregateType (power-up application)
- AddRaw
- Multiplicate
- AddFinal
- Maximum

### AncientSetDiscriminator
- 0 (not ancient)
- 1 (first ancient set of the item)
- 2 (second ancient set of the item)

### SpecialItemType (drop groups touching options)
- RandomItem
- Excellent
- Money
- Jewel

## Version notes
- Version075: only Option + Luck option types exist; NO excellent options, no excellent drop group; wing options are plain Option-type options (JoL-leveled); no ItemSetGroups at all (source has 'TODO: ItemSetGroups for set bonus' comment) — so no ancient sets and no generic set bonuses in the 075 dataset.
- Version095d: adds the Excellent option type and the shared ExcellentOptions initializer (defense/physical/wizardry, 6 options each, AddChance 0.001, max 2/item); weapons attach exc-physical or exc-wizardry, jewelry rings attach exc-defense and pendants exc-attack; still NO ItemSetGroups/ancient sets; pets add Dinorant options (3 options, Option type, AddChance 0.3).
- Version095d armors: the shared ArmorInitializerBase attaches excellent-defense (and harmony-defense) groups only if they exist in the config, so in 095d armor gets Luck+Defense+ExcDefense; shields get Luck+DefenseRate(+exc).
- VersionSeasonSix: enables all 14 option types; adds ancient sets (36), harmony options, guardian options, socket system, fenrir/dark-horse options, 2nd/3rd wing 'Wing'-type options and cape command option, plus normal options for curse and combined phys+wiz damage.
- Ancient sets are Season 1+ game content but OpenMU only ships them in the VersionSeasonSix dataset; a pre-S3 target must curate that list (S6 file includes DL/MG sets and Red Wing sets; no Summoner/RF sets are defined anyway, but item references like Wings of Curse do not exist pre-S3).
- Excellent implicit base-bonus formulas and ancient implicit bonus formulas live in game logic (ItemPowerUpFactory), not in data; drop level formula: +25 excellent / +30 ancient / +3*itemLevel.
- MaximumOptionLevel = 4 everywhere (jewelry = 3); dropped items cap normal option at level 3 (MaximumItemOptionLevelDrop).

## Post-S3 exclusions
- HarmonyOption type + HarmonyOptions initializer (Jewel of Harmony, Season 3): 4 groups (defense/physical/wizardry/curse), weight-based rollout (Weight 10..50), per-level value tables gated by RequiredItemLevel, MaximumOptionsPerItem 1 — exclude.
- GuardianOption type + GuardianOptions initializer (Season 3, 380-level items via chaos machine + Jewel of Guardian): per-slot pairs, e.g. weapon AttackRatePvp+10 / FinalDamageIncreasePvp+200; pants/armor/helm/gloves/boots DefenseRatePvp+10 + second stat — exclude.
- SocketOption + SocketBonusOption types, SocketSystem initializer, SubOptionType-as-element (fire/water/ice/wind/lightning/earth), Item.SocketCount, ItemOptionLink.Index, socket package combination bonuses (Season 4) — exclude.
- 3rd wings + Wing3rd option group (4 options, 5% each: ignore-def / full-reflect / full-HP-restore / full-mana-restore) and Cape of Fighter/Emperor/Overrule — Season 3+.
- ExcellentCurse option group — Season 14 only (pre-S14 curse books use wizardry excellent options); OpenMU has it disabled.
- Fenrir options (Blue/Black/Gold option types) and their movement-speed combination bonuses — Fenrir is late Season 1/Season 2 content but only in the S6 dataset; Gold Fenrir at least should be reviewed; combination-bonus usage otherwise socket-only.
- ItemOptionCombinationBonus as a whole is only exercised by post-S3/S6 content (socket packages, fenrir speed) — safe to defer.
- 2nd-wing 'excellent wing options' (Wing option type: +HP 50~125, +Mana 50~125, ignore-def 3%; LevelType=ItemLevel; AddChance 0.1) — introduced around Season 2/2.6 depending on server; only present in the S6 dataset; user decision for pre-S3.

## Open questions
- Which ancient sets are in scope for the pre-S3 target? The S6 list (36 sets) includes Adamantine (DL), Storm Crow (MG) and Red Wing sets; sets referencing Wings of Curse-era items do not apply, and classic pre-S3 servers commonly have ~30 sets. Needs curation decision.
- Kantata set option 6 is 'ExcellentDamageChance 10.0f AddRaw' in AncientSets.cs while every comparable option uses 0.10 — almost certainly an OpenMU data bug (1000% vs 10%). Adopt 0.10?
- Excellent options per item: OpenMU caps random drops at 2 (MaximumOptionsPerItem) with first option guaranteed; classic servers allow 1-2 on drops but more via Chaos Machine wings/events. Confirm the desired cap and roll rules for mu-core.
- Wing bonus options for 2nd wings (+HP/+Mana 50~125, ignore-def 3%) — include for pre-S3 or not? OpenMU only defines them in the S6 dataset; 095d dataset has no 2nd wings at all (only the three 1st wings), although real 0.97+ had 2nd wings.
- Ancient per-piece bonus is always +5/+10 of one stat (level chosen randomly at drop). Classic behavior ties bonus level to... (some servers fix +5 Vit for armor, +5~10 for others). Keep OpenMU's random 1-2 model?
- Excellent/ancient implicit base-bonus formulas are hardcoded game logic in OpenMU (marked 'TODO: make configurable'). Should mu-core treat them as fixed formulas or as data?
- Non-ancient set bonuses (DefenseSetBonusOption 0x20 / DefenseRateSetBonusOption 0x21, AlwaysApplies + SetLevel mechanics, e.g. '+defense when full set is +10') are referenced by the model and numbers but no initializer populates them in any version (075 has a TODO). Decide whether mu-core needs them.
- Fenrir/Dark Horse/Dinorant: Dinorant options exist in 095d; Fenrir and Dark Horse options only in the S6 dataset. Which pets (and their option sets) are in scope for pre-S3?
