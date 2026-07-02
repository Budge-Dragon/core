# character stats + classes + attribute system (OpenMU domain facts for pre-Season-3 mu-core)

## Counts
- Stats.cs total attribute definitions: 265 (src/GameLogic/Attributes/Stats.cs); majority are S3+/S6/master-tree buckets — pre-S3-relevant subset is roughly 80-100
- Version075 character classes: 3 (Dark Wizard 0, Dark Knight 4, Fairy Elf 8), no class evolution (NextGenerationClass = null)
- Version095d character classes: 4 (adds Magic Gladiator 12), no class evolution
- VersionSeasonSix character classes: 18 (7 class lines; DW/DK/Elf/Summoner with 3 tiers, MG/DL/RF with 2 tiers)
- Common attribute relationships added to every class: ~30 (AddCommonAttributeRelationships)
- Common const base values per class: 11 (+2 if master class, +1 ShieldRecoveryMultiplier if non-classic PvP)
- Character entity fields: ~25 persisted fields
- GameConfiguration character-relevant scalar fields: ~25

## Entities

### attribute definition (AttributeDefinition — node of the attribute system)
- id: unique identity (OpenMU uses GUID; mu-core needs a stable key — name or enum)
- designation: string (display name)
- description: string
- maximum_value: optional float cap enforced when aggregating (used by: AttackSpeed=200, MagicSpeed=200, AreTwoWeaponsEquipped=1, IsOneHandedSwordEquipped=1, IsOneHandedStaffEquipped=1, SoulBarrierReceiveDecrement=0.7)
- attributes are the universal currency: character stats, item power-ups, monster attributes, magic effects all reference AttributeDefinition

### stat attribute definition (per-class entry: which stats a class has)
- attribute: reference to an AttributeDefinition
- base_value: float — initial value at character creation (e.g. DK BaseStrength=28)
- increasable_by_player: bool — true only for Base Strength/Agility/Vitality/Energy/Leadership; false for Level, PointsPerLevelUp, CurrentHealth/Mana/Ability/Shield, IsInSafezone, Resets, MasterLevel, AmmunitionAmount
- a character class owns a list of these; the character persists one StatAttribute (definition ref + float value) per entry

### attribute relationship (derived-stat rule, pure data)
- target_attribute: AttributeDefinition ref — the derived stat being contributed to
- input_attribute (source): AttributeDefinition ref — the stat read from
- input_operand: float constant (multiplier/addend/exponent depending on operator)
- operand_attribute: optional AttributeDefinition used instead of the constant (two uses: dynamic multiplier, or 0/1 'conditional' gate — e.g. target += condition * source)
- input_operator: enum InputOperator — how operand combines with input value
- aggregate_type: enum AggregateType — how the computed value combines into the target
- semantics: contribution = input_attribute.value <op> operand; target aggregates all contributions: sum(AddRaw) * product(Multiplicate) + sum(AddFinal), Maximum takes max
- conditional form (CreateConditionalRelationship): target += conditional_attribute * source_attribute (conditional acts as 0/1 switch), aggregate type selectable
- relationships live in two places: per CharacterClass (AttributeCombinations) and globally on GameConfiguration (GlobalAttributeCombinations)

### const value attribute (class-level constant seed)
- value: float
- definition: AttributeDefinition ref
- per CharacterClass (BaseAttributeValues) and globally on GameConfiguration (GlobalBaseAttributeValues)
- example: DK gets +35 MaximumHealth, +10 MaximumMana as constants before relationships apply

### character class
- number: byte — client-visible class id (see CharacterClassNumber enum)
- name: string
- can_get_created: bool — false for evolved tiers (Blade Knight etc.)
- level_requirement_by_creation: short — another character on the same account must reach this level to unlock creation (MG=220, DL=250, RF=150, others 0)
- creation_allowed_flag: byte bitmask sent to client (1=Summoner, 2=Dark Lord, 4=Magic Gladiator, 8=Rage Fighter)
- next_generation_class: optional ref to evolved class (S6 only; null in 075/095d)
- is_master_class: bool (S4+ master tree; always false pre-S3)
- level_warp_requirement_reduction_percent: int — map-warp level requirement reduction; 34 (=ceil(100/3)) for MG/DL/RF/Summoner, 0 otherwise
- fruit_calculation: FruitCalculationStrategy enum
- stat_attributes: list of stat attribute definitions (see per-class tables below)
- attribute_combinations: list of attribute relationships (derived stat formulas)
- base_attribute_values: list of const value attributes
- home_map: ref to game map (Lorencia=0 for DW/DK/MG/DL/RF, Noria=3 for Elf, Elvenland=51 for Summoner)
- combo_definition: optional skill-combo definition ref (DK combo)
- PER-CLASS BASE STATS (identical in 075/095d/S6): Dark Wizard(0): STR 18, AGI 18, VIT 15, ENE 30, initial HP 60, mana 60, ability 1, points/level 5 | Dark Knight(4): STR 28, AGI 20, VIT 25, ENE 10, HP 110, mana 20, ability 1, points/level 5 | Fairy Elf(8): STR 22, AGI 25, VIT 20, ENE 15, HP 80, mana 30, ability 1, points/level 5, plus AmmunitionAmount stat (base 0) | Magic Gladiator(12): STR 26, AGI 26, VIT 26, ENE 26, HP 110, mana 60, points/level 7 | Dark Lord(16): STR 26, AGI 20, VIT 20, ENE 15, CMD(Leadership) 25, HP 90, mana 40, points/level 7 | Summoner(20) S3+: STR 21, AGI 21, VIT 18, ENE 23, HP 70, mana 40, points/level 5 | Rage Fighter(24) S5+: STR 32, AGI 27, VIT 25, ENE 20, HP 100, mana 40, points/level 7
- every class also has stats: Level (base 1, not increasable), PointsPerLevelUp (5 or 7), IsInSafezone (base 1), Resets (base 0); S6 non-classic adds CurrentShield (base 1); master classes add MasterLevel (base 0)

### class formulas — common relationships (all classes, all versions)
- TotalLevel = Level + MasterLevel (pre-S3: MasterLevel absent, TotalLevel == Level)
- TotalStrength = BaseStrength; TotalAgility = BaseAgility; TotalVitality = BaseVitality; TotalEnergy = BaseEnergy (item/buff bonuses aggregate into the Total* attribute)
- DefenseBase += DefenseShield(item); DefenseFinal = 0.5 * DefenseBase; DefensePvm = DefenseFinal; DefensePvp = DefenseFinal
- AttackSpeedAny += AttackSpeedByWeapon; AttackSpeed += AttackSpeedAny (cap 200); MagicSpeed += AttackSpeedAny (cap 200)
- MinimumPhysBaseDmg += MinimumPhysBaseDmgByWeapon + BaseMinDamageBonus + PhysicalBaseDmg; MaximumPhysBaseDmg += MaximumPhysBaseDmgByWeapon + BaseMaxDamageBonus + PhysicalBaseDmg; PhysicalBaseDmg += BaseDamageBonus; both min/max multiplied by PhysicalBaseDmgIncrease (Multiplicate)
- two weapons equipped: AreTwoWeaponsEquipped = min(EquippedWeaponCount,1); if set, AttackSpeedAny -= 0.5 * AttackSpeedByWeapon (averages dual-weapon speed)
- shield-equipped defense: DefenseFinal *= (1 + IsShieldEquipped * DefenseIncreaseWithEquippedShield); DefenseFinal += DefenseShield * ShieldItemDefenseIncrease
- HealthRecoveryMultiplier += 0.01 * IsInSafezone
- classic PvP (075/095d): DefenseRatePvp = DefenseRatePvm; AttackRatePvp = AttackRatePvm (no separate PvP formulas, no shield)
- non-classic (S6): ShieldRecoveryMultiplier += 0.01 * IsInSafezone
- MaximumGuildSize = 0.1 * Level (DL adds 0.1 * TotalLeadership)
- CanFly = IsDinorantEquipped (Icarus entry gate)

### class formulas — common const base values (all classes)
- ManaRecoveryMultiplier = 1/27.5; DamageReceiveDecrement = 1; AttackDamageIncrease = 1; ExperienceRate = 1; PoisonDamageMultiplier = 0.03; ItemDurationIncrease = 1; AbilityRecoveryAbsolute = 2; PhysicalBaseDmgIncrease = 1; AreTwoWeaponsEquipped = -1 (offset so count of 2 → 1); HasDoubleWield = -1; DefenseDecrement = 1
- master class only (S4+): MasterPointsPerLevelUp = 1; MasterExperienceRate = 1
- non-classic PvP only (S6): ShieldRecoveryMultiplier = 0.01
- regeneration rule: every RecoveryInterval (3000 ms), current += multiplier * maximum + absolute, clamped at maximum; applies to health, mana, ability(AG), shield(S3+); a second set of after-monster-kill regenerations exists (multiplier-of-max + absolute variants)

### class formulas — Dark Knight (identical 075/095d/S6 except PvP/shield block)
- DefenseBase += TotalAgility / 3; DefenseRatePvm += TotalAgility / 3
- AttackRatePvm = 5*TotalLevel + 1.5*TotalAgility + 0.25*TotalStrength
- AttackSpeed += TotalAgility / 15; MagicSpeed += TotalAgility / 20
- MaximumAbility(AG) = 1*ENE + 0.3*VIT + 0.2*AGI + 0.15*STR; AbilityRecoveryMultiplier = 0.05
- MaximumMana = 10 + 1*TotalEnergy + 0.5*TotalLevel
- MaximumHealth = 35 + 2*TotalLevel + 3*TotalVitality
- MinimumPhysBaseDmg += TotalStrength / 6; MaximumPhysBaseDmg += TotalStrength / 4
- SkillMultiplier = 2 (const) + 0.001 * TotalEnergy
- ComboBonus = 0.5*STR + 0.5*AGI + 0.5*ENE (combo damage; DK-only skill combo)
- double wield (shared with MG/RF): HasDoubleWield = max-gate on DoubleWieldWeaponCount; PhysicalBaseDmgIncrease *= (1 - 0.45*HasDoubleWield) i.e. 55% base, doubled later in damage calc → 110%; when double wielding, right-hand weapon min/max damage added conditionally
- FenrirBaseDmg = STR/3 + AGI/5 + VIT/5 + ENE/7 (Fenrir pet is Season 2 content)
- S6-only PvP/shield: MaximumShield = 1.2*(ENE+VIT+AGI+STR) + DefenseFinal + TotalLevel^2/30; DefenseRatePvp = 0.5*AGI + 2*Level; AttackRatePvp = 4.5*AGI + 3*Level

### class formulas — Dark Wizard (identical 075/095d/S6 except PvP/shield block)
- DefenseBase += 0.25 * TotalAgility; DefenseRatePvm += TotalAgility / 3
- AttackRatePvm = 5*TotalLevel + 1.5*TotalAgility + 0.25*TotalStrength
- AttackSpeed += TotalAgility / 20; MagicSpeed += TotalAgility / 10
- MaximumAbility = 0.2*ENE + 0.3*VIT + 0.4*AGI + 0.2*STR; AbilityRecoveryMultiplier = 1/33
- MaximumMana = 2*TotalEnergy + 2*TotalLevel
- MaximumHealth = 30 + 1*TotalLevel + 2*TotalVitality
- MinimumPhysBaseDmg += STR / 8; MaximumPhysBaseDmg += STR / 4
- MinimumWizBaseDmg = ENE / 9; MaximumWizBaseDmg = ENE / 4 (plus BaseMin/MaxDamageBonus, WizardryBaseDmg bucket, multiplied by WizardryBaseDmgIncrease)
- WizardryAttackDamageIncrease = 1.0 (const) + StaffRise / 100 (staff rise % is an item stat)
- SkillMultiplier const = 1
- S6-only PvP: DefenseRatePvp = 0.25*AGI + 2*Level; AttackRatePvp = 4*AGI + 3*Level; shield formula same shape as DK (1.2 * each stat + DefenseFinal + Level^2/30)

### class formulas — Fairy Elf (identical 075/095d/S6 except PvP/shield block)
- TotalStrengthAndAgility = TotalStrength + TotalAgility (helper attribute)
- DefenseBase += TotalAgility / 10; DefenseRatePvm += 0.25 * TotalAgility
- AttackRatePvm = 5*TotalLevel + 1.5*TotalAgility + 0.25*TotalStrength
- AttackSpeed += TotalAgility / 50; MagicSpeed += TotalAgility / 50
- MaximumAbility = 0.2*ENE + 0.3*VIT + 0.2*AGI + 0.3*STR; AbilityRecoveryMultiplier = 1/33
- MaximumMana = 6 + 1.5*TotalEnergy + 1.5*TotalLevel
- MaximumHealth = 39 + 1*TotalLevel + 2*TotalVitality
- two attack modes: ArcheryAttackMode = IsBowEquipped + IsCrossBowEquipped; MeleeAttackMode = 0^ArcheryAttackMode (1 when no bow, 0 when bow)
- archery damage: ArcheryMinDmg = AGI/7 + STR/14; ArcheryMaxDmg = AGI/4 + STR/8 — gated into phys dmg when in archery mode; ammunition damage bonus multiplies phys base dmg in archery mode
- melee damage: MeleeMinDmg = (STR+AGI)/7; MeleeMaxDmg = (STR+AGI)/4 — gated in when in melee mode
- has AmmunitionAmount stat; AmmunitionConsumptionRate governs arrow/bolt use
- SkillMultiplier const = 1
- S6-only PvP: DefenseRatePvp = 0.1*AGI + 2*Level; AttackRatePvp = 0.6*AGI + 3*Level

### class formulas — Magic Gladiator (in 095d and S6; NOT in 075)
- no second base class; creation gated at account level 220; CreationAllowedFlag=4; warp level requirement reduced 34%; points per level 7
- DefenseBase += TotalAgility / 5; DefenseRatePvm += TotalAgility / 3
- AttackRatePvm = 5*TotalLevel + 1.5*TotalAgility + 0.25*TotalStrength
- AttackSpeed += AGI / 15; MagicSpeed += AGI / 20
- MaximumAbility = 0.15*ENE + 0.3*VIT + 0.25*AGI + 0.2*STR
- MaximumMana = 7 + 2*TotalEnergy + 1*TotalLevel
- MaximumHealth = 57 + 1*TotalLevel + 2*TotalVitality
- MinimumPhysBaseDmg += STR/6 + ENE/12; MaximumPhysBaseDmg += STR/4 + ENE/8
- MinimumWizBaseDmg = ENE/9; MaximumWizBaseDmg = ENE/4; WizardryAttackDamageIncrease = 1 + StaffRise/100
- SkillMultiplier const = 2
- can double wield (same 110% phys rule as DK); equipping a one-handed staff switches off IsOneHandedSwordEquipped (energy-MG vs strength-MG wield disambiguation)
- S6-only PvP: DefenseRatePvp = 0.25*AGI + 2*Level; AttackRatePvp = 3.5*AGI + 3*Level

### class formulas — Dark Lord (only in S6 dataset; historically ~v1.0, pre-S3)
- 5th base stat: BaseLeadership/Command (base 25, increasable); TotalLeadership = BaseLeadership
- creation gated at account level 250; CreationAllowedFlag=2; warp reduction 34%; points per level 7
- DefenseBase += AGI / 7; DefenseRatePvm += AGI / 7
- AttackRatePvm = 5*TotalLevel + 2.5*TotalAgility + STR/6 + CMD/10
- AttackSpeed += AGI / 10; MagicSpeed += AGI / 10
- MaximumAbility = 0.15*ENE + 0.1*VIT + 0.2*AGI + 0.3*STR + 0.3*CMD
- MaximumMana = 38 + 1.5*(TotalEnergy - 15) + 1*TotalLevel
- MaximumHealth = 48.5 + 1.5*TotalLevel + 2*TotalVitality
- MinimumPhysBaseDmg += STR/7 + ENE/14; MaximumPhysBaseDmg += STR/5 + ENE/10
- SkillMultiplier = 2 (const) + 0.0005 * TotalEnergy
- RavenAttackDamageIncrease = ScepterRise / 100
- MaximumGuildSize += 0.1 * TotalLeadership; DamageReceiveDecrement *= (1 + DamageReceiveHorseDecrement)
- dark raven pet stats: RavenMinimumDamage = 180 + CMD/8 + 15*RavenLevel; RavenMaximumDamage = 200 + CMD/4 + 15*RavenLevel; RavenAttackSpeed = 20 + CMD/50 + 0.8*RavenLevel; RavenAttackRate = 1000 + (16/15)*RavenLevel; RavenCriticalDamageChance = 0.3; PetDurationIncrease = 1
- S6-only PvP: DefenseRatePvp = 0.5*AGI + 2*Level; AttackRatePvp = 4*AGI + 3*Level

### character (persisted entity)
- name: string, validated by GameConfiguration.CharacterNameRegex (default ^[a-zA-Z0-9]{3,10}$)
- character_class: ref to CharacterClass
- character_slot: byte — slot within account (account max characters default 5)
- create_date: timestamp
- experience: long (i64); master_experience: long (S4+, exclude)
- level: NOT a direct field — persisted as a StatAttribute (Level definition, float value); level range 1..GameConfiguration.MaximumLevel
- level_up_points: int — remaining spendable stat points; gained per level = value of class PointsPerLevelUp stat (5 or 7); spending decrements, one point per stat point; a stat cannot exceed its AttributeDefinition.maximum_value if set; GM characters bypass the points check
- master_level_up_points: int (S4+, exclude)
- current_map: ref to game map definition; position_x: byte (0-255); position_y: byte (0-255)
- player_kill_count: int; state: HeroState enum; state_remaining_seconds: int (countdown until hero/PK state decays toward Normal)
- character_status: CharacterStatus enum (Normal=0, Banned=1, GameMaster=32)
- pose: CharacterPose enum (byte)
- used_fruit_points: int; used_neg_fruit_points: int — fruit stat add/remove budgets; max fruit points by level: start 2, every 10th level add (3*(level+10)/divisor)+2 where divisor=400 default /700 MG /500 DL (yields class caps ~127/100/115)
- inventory_extensions: int (count of purchased inventory expansions; S3+ feature)
- key_configuration: opaque byte[] client hotkey blob; mu_helper_configuration: opaque byte[] (S9+, exclude)
- store_name: optional string; is_store_opened: bool (personal store)
- attributes: collection of StatAttribute (attribute definition ref + float value) — the persisted base stats
- letters: list of letter headers; learned_skills: collection of skill entries; quest_states: collection
- inventory: ItemStorage ref; money lives on ItemStorage.Money (int, capped by GameConfiguration.MaximumInventoryMoney)
- drop_item_groups: character-specific drop groups (by reference)

### game configuration (character/progression-relevant fields; same values initialized for 075, 095d, S6)
- maximum_level: short = 400
- maximum_master_level: short = 200 (S4+, exclude)
- experience_rate: float = 1.0
- experience_formula: string per-level total-XP formula, variable 'level' = if(level==0, 0, if(level<256, 10*(level+8)*(level-1)^2, 10*(level+8)*(level-1)^2 + 1000*(level-247)*(level-256)^2))
- master_experience_formula (S4+): (505*level^3) + (35278500*level) + (228045*level^2)
- prevent_experience_overflow: bool
- minimum_monster_level_for_master_experience: byte = 95 (S4+)
- recovery_interval: int ms = 3000 (regeneration tick)
- info_range: byte = 12 (visibility range in tiles)
- maximum_inventory_money: int = i32::MAX; maximum_vault_money: int = i32::MAX
- maximum_letters = 50; letter_send_price = 1000 zen
- maximum_characters_per_account: byte = 5; maximum_password_length = 20
- maximum_party_size: byte = 5
- should_drop_money: bool = true (monster money drops on ground vs direct add)
- item_drop_duration = 60 s; maximum_item_option_level_drop: byte = 3
- damage_per_one_item_durability = 2000; damage_per_one_pet_durability = 100000; hits_per_one_item_durability = 10000
- owns global collections: attributes, character classes, global attribute combinations, global base attribute values, skills, items, maps, monsters, magic effects, drop item groups, master skill roots (S4+)

### stat catalogue — pre-S3 relevant attributes (from Stats.cs)
- base/increasable: BaseStrength, BaseAgility, BaseVitality, BaseEnergy, BaseLeadership (DL only)
- totals (base + item/buff powerups): TotalStrength, TotalAgility, TotalVitality, TotalEnergy, TotalLeadership, TotalStrengthAndAgility (elf helper), TotalLevel
- item requirement helpers: TotalStrengthRequirementValue, TotalAgilityRequirementValue, TotalVitalityRequirementValue, TotalEnergyRequirementValue, TotalLeadershipRequirementValue; RequiredStrength/Agility/Energy/Vitality/LeadershipReduction (item options reduce requirement)
- progression: Level, PointsPerLevelUp, ExperienceRate, BonusExperienceRate, RandomExperienceMinMultiplier, RandomExperienceMaxMultiplier
- resources: CurrentHealth/MaximumHealth, CurrentMana/MaximumMana, CurrentAbility/MaximumAbility (AG)
- offense: MinimumPhysBaseDmg, MaximumPhysBaseDmg, PhysicalBaseDmg (min+max bucket), MinimumPhysBaseDmgByWeapon, MaximumPhysBaseDmgByWeapon, MinPhysBaseDmgByRightWeapon, MaxPhysBaseDmgByRightWeapon, MinimumWizBaseDmg, MaximumWizBaseDmg, WizardryBaseDmg, StaffRise (%), BaseDamageBonus, BaseMinDamageBonus, BaseMaxDamageBonus, FinalDamageBonus, SkillMultiplier, SkillDamageBonus, CriticalDamageBonus, CriticalDamageChance, ExcellentDamageBonus, ExcellentDamageChance, DoubleDamageChance, DefenseIgnoreChance, StunChance, AttackDamageIncrease, WizardryAttackDamageIncrease, PhysicalBaseDmgIncrease, WizardryBaseDmgIncrease, GreaterDamageBonus (elf buff), ComboBonus, IsSkillComboAvailable
- elf modes: MeleeAttackMode, MeleeMinDmg, MeleeMaxDmg, ArcheryAttackMode, ArcheryMinDmg, ArcheryMaxDmg, AmmunitionAmount, AmmunitionConsumptionRate, AmmunitionDamageBonus
- hit rates: AttackRatePvm, AttackRatePvp, DefenseRatePvm, DefenseRatePvp
- defense: DefenseBase, DefenseFinal, DefensePvm, DefensePvp, DefenseShield (item), DamageReceiveDecrement (multiplier), ArmorDamageDecrease, DamageReflection, DefenseIncreaseWithEquippedShield, SoulBarrierReceiveDecrement (cap 0.7), SoulBarrierManaTollPerHit, DefenseDecrement, WeaknessPhysDmgDecrement
- speed: AttackSpeed (cap 200), MagicSpeed (cap 200), AttackSpeedAny, AttackSpeedByWeapon, WalkSpeed, MovementSpeed, MovementSpeedFactor
- weapon state flags (0/1): EquippedWeaponCount, AreTwoWeaponsEquipped, HasDoubleWield, DoubleWieldWeaponCount, IsTwoHandedWeaponEquipped, IsBowEquipped, IsCrossBowEquipped, IsOneHandedSwordEquipped, IsTwoHandedSwordEquipped, IsMaceEquipped, IsSpearEquipped, IsOneHandedStaffEquipped, IsTwoHandedStaffEquipped, IsScepterEquipped (DL), IsShieldEquipped, IsHorseEquipped (DL), ScepterRise (%)
- recovery: HealthRecoveryMultiplier, ManaRecoveryMultiplier, AbilityRecoveryMultiplier, HealthRecoveryAbsolute, ManaRecoveryAbsolute, AbilityRecoveryAbsolute, Health/Mana/AbilityAfterMonsterKillMultiplier+Absolute, ManaUsageReduction (0-1), AbilityUsageReduction (0-1), HealthLossAfterHit, SkillExtraManaCost
- status flags: IsIced, IsFrozen, IsPoisoned, IsStunned, IsAsleep(S3+ summoner), IsBleeding, PoisonDamageMultiplier (0.03 base), BleedingDamageMultiplier
- resistances (0..1): Ice, Fire, Water, Earth, Wind, Poison, Lightning; matching elemental DamageBonus attributes (ancient jewelry)
- misc: IsInSafezone (0/1), TransformationSkin (monster number when transformed), IsInvisible, MaximumGuildSize, MoneyAmountRate, ItemDurationIncrease, PetDurationIncrease (DL), CanFly, IsDinorantEquipped, NearbyPartyMemberCount, SkillLevel, GainHeroStatusQuestCompleted, FinalDamageIncreasePvp, Resets, TwoHandedWeaponDamageIncrease (ancient opt)
- DL raven/horse: RavenLevel, HorseLevel, RavenMinimumDamage, RavenMaximumDamage, RavenAttackRate, RavenAttackSpeed, RavenCriticalDamageChance, RavenExcDamageChance, RavenAttackDamageIncrease, DamageReceiveHorseDecrement, FenrirBaseDmg (Fenrir = Season 2)

## Enums

### InputOperator (attribute relationship operator)
- Multiply (target += input * operand)
- Add (target += input + operand)
- Exponentiate (target += input ^ operand; e.g. Level^2 for shield)
- ExponentiateByAttribute (target += operand_attribute-driven exponent; used as 0^x boolean-NOT gate)
- Minimum (min(input, operand))
- Maximum (max(input, operand); used to clamp counts to 0/1)

### AggregateType (how a contribution combines into the target attribute)
- AddRaw (summed first)
- Multiplicate (product applied to raw sum)
- AddFinal (added after multiplication)
- Maximum (target becomes max of contributions; used by movement-speed item powerups)

### CharacterClassNumber (byte, client class id)
- DarkWizard = 0
- SoulMaster = 2
- GrandMaster = 3 (S4+)
- DarkKnight = 4
- BladeKnight = 6
- BladeMaster = 7 (S4+)
- FairyElf = 8
- MuseElf = 10
- HighElf = 11 (S4+)
- MagicGladiator = 12
- DuelMaster = 13 (S4+)
- DarkLord = 16
- LordEmperor = 17 (S4+)
- Summoner = 20 (S3+)
- BloodySummoner = 22 (S3+)
- DimensionMaster = 23 (S4+)
- RageFighter = 24 (S5+)
- FistMaster = 25 (S5+)

### HeroState (PK/hero status, decays toward Normal over time)
- New
- Hero
- LightHero
- Normal
- PlayerKillWarning
- PlayerKiller1stStage
- PlayerKiller2ndStage

### CharacterStatus
- Normal = 0
- Banned = 1
- GameMaster = 32

### CharacterPose (byte)
- Standing = 0
- Sitting = 2
- Leaning = 3
- Hanging = 4

### FruitCalculationStrategy (max fruit points per class line)
- Default = 0 (divisor 400, max ~127)
- MagicGladiator = 1 (divisor 700, max ~100)
- DarkLord = 2 (divisor 500, max ~115)

### CreationAllowedFlag (account unlock bitmask sent to client)
- 1 = Summoner
- 2 = Dark Lord
- 4 = Magic Gladiator
- 8 = Rage Fighter

## Version notes
- Version075: only 3 classes (Dark Wizard 0, Dark Knight 4, Fairy Elf 8); no NextGenerationClass (no BK/SM/ME evolution); UseClassicPvp=true → no shield (SD) stats at all, DefenseRatePvp=DefenseRatePvm and AttackRatePvp=AttackRatePvm; file src/Persistence/Initialization/Version075/CharacterClassInitialization.cs
- Version095d: same as 075 plus Magic Gladiator (12), still no evolution classes and classic PvP; file src/Persistence/Initialization/Version095d/CharacterClassInitialization.cs
- VersionSeasonSix: 18 classes across 7 lines with evolution chains (DK→BK→BM etc.), master classes (IsMasterClass, MasterLevel stat), shield/SD stat block, separate PvP attack/defense rate formulas
- The per-class stat formulas (defense, attack rate, HP/mana/AG, damage) are shared code — identical numbers in all three versions; the version difference is purely which classes exist and the classic-PvP/shield toggle
- Stats.cs is a single master list shared by all versions; pre-S3 versions simply never wire up the S3+ attributes
- GameConfiguration defaults (max level 400, experience formula, recovery interval 3000ms, party size 5, etc.) come from a shared base initializer — neither 075 nor 095d overrides them
- OpenMU 075/095d datasets omit Dark Lord entirely, although historically DL appeared pre-S3 (~v1.0); DL exists only in the S6 dataset
- Character.Experience is a total (not per-level) value; the XP formula gives cumulative XP required for a level; levels ≥256 use the extended second term

## Post-S3 exclusions
- Shield/SD system entirely: CurrentShield, MaximumShield, MaximumShieldTemp (Level^2 intermediate), ShieldRecovery*/ShieldAfterMonsterKill*, ShieldBypassChance, ShieldDecreaseRateIncrease, ShieldRateIncrease, ShieldRecoveryEverywhere, and the separate PvP rate formulas (DefenseRatePvp/AttackRatePvp per-class formulas) — OpenMU models pre-S3 via UseClassicPvp=true
- Master level system (S4+): MasterLevel, MasterLevelUpPoints, MasterPointsPerLevelUp, MasterExperience(-Rate/-Formula), MaximumMasterLevel, MinimumMonsterLevelForMasterExperience, MasterSkillRoots, IsMasterClass, all master-tier classes (GrandMaster, BladeMaster, HighElf, DuelMaster, LordEmperor, DimensionMaster, FistMaster)
- All '(MST)' master-tree bucket attributes in Stats.cs (~60): weapon strengthener/mastery bonus damages (sword/bow/crossbow/mace/spear/staff/stick/book/scepter/glove), WeaponMasteryAttackSpeed, MasterSkillPhysBonusDmg, BonusDefenseWithShield/Horse, BonusDefenseRateWithShield, wing-3 options (FullyRecoverMana/Health/ReflectDamageAfterHitChance), CrossBow/TwoHandedSword/TwoHandedStaff/Stick/Scepter MasteryBonusDamage, Min/WizardryAndCurseDmgBonus, PointsPerReset-style extras
- Summoner class (S3) and its stats: curse damage line (Min/MaximumCurseBaseDmg, CurseBaseDmg, CurseAttackDamageIncrease), BookRise, IsStickEquipped/IsBookEquipped bonus damages, Berserker* (8 attributes), Explosion/Requiem/PollutionBonusDmg, IsAsleep, SummonedMonsterHealth/DefenseIncrease, WeaknessPhysDmgDecrement, InnovationDefDecrement
- Rage Fighter class (S5/6): VitalitySkillMultiplier, IsGloveWeaponEquipped, GloveWeaponBonusDamage; base stats STR 32/AGI 27/VIT 25/ENE 20 recorded here only for reference
- Fenrir pet (Season 2 — borderline): FenrirBaseDmg relationships exist on every class; keep only if targeting S2+; RavenExcDamageChance-style Fenrir damage flows are S6 wiring
- Socket/harmony references (S3/S4): socket option mentions inside stat remarks; ShieldItemDefenseIncrease (water socket), harmony defense bonus — the attributes named for them
- MoonstonePendantEquipped (Kanturu, S3), IsMuHelperActive (S9), IsVip, PointsPerReset, Resets/reset system (custom server feature), TransformationSkin elite-ring variants, MovementSpeedUnderwater/IsUnderwater (S6 swamp), NovaStageDamage placeholder is 0.97+/1.0 (Nova skill — verify target version), InventoryExtensions on Character (S3+)
- SkillBase/FinalMultiplier + SkillBase/FinalDamageBonus 'skill attribute' quartet (used by S6 AreaSkillSettings)

## Open questions
- Dark Lord: not present in OpenMU 075/095d datasets, but historically pre-S3 (~v1.0). Include DL (with Leadership stat, raven/horse attributes, fruit strategy 2) in mu-core's pre-S3 target or follow OpenMU's 095d class set (DW/DK/Elf/MG)?
- Class evolution (Dark Knight→Blade Knight etc. at level 150): OpenMU 075/095d have NO second classes (NextGenerationClass=null). Historical 0.97+ had second classes. Model evolution chains or not?
- MaximumLevel=400 and the S6 experience formula (with ≥256 extension) are applied to ALL versions in OpenMU, including 0.75. Historical 0.75 caps were lower (~350). Which cap/formula does mu-core want?
- Fruit system (fruit points, UsedFruitPoints/UsedNegFruitPoints, per-class caps 127/100/115): OpenMU wires it globally; fruits historically arrived ~v1.0. Include for pre-S3?
- Fenrir attributes (FenrirBaseDmg per class): Season 2 content — inside or outside 'pre-Season-3'? Same question for DK skill combo (ComboBonus) which OpenMU wires for all versions but historically appeared ~v0.98/1.0.
- Elf Ammunition (arrows/bolts) stats exist in all versions — confirm mu-core wants ammunition consumption modeling.
- MaximumShield-style AttributeDefinition.maximum_value caps: only 6 attributes carry caps (AttackSpeed/MagicSpeed 200 etc.). Should mu-core support optional per-attribute caps generally? (needed for stat-increase clamping)
- Stat point spend: OpenMU allows spending multiple points at once and clamps at attribute maximum_value; no per-stat max like 32767 found in domain model — confirm mu-core's stat cap policy (u16 max? none?)
- Money is int (i32) on ItemStorage with configurable max (default i32::MAX); classic servers cap zen at 2,000,000,000 — decide cap.
- AttributeRelationship in OpenMU supports stacked temp attributes (anonymous intermediates created per class, e.g. 'Temp Half weapon attack speed', DL 'TotalEnergy minus 15'). mu-core schema must allow named intermediate attributes not in the master stat list.
- HeroState decay timing (StateRemainingSeconds) semantics are engine-side; only the countdown field is data. Confirm mu-core scope.
- CharacterClass.Name is localized (LocalizedString) in OpenMU — single string sufficient for mu-core?
