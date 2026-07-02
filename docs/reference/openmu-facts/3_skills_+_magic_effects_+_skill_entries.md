# skills + magic effects + skill entries

## Counts
- Version075 skills: 30 (skill numbers 1-50 range; DW 13, DK 6, Elf 10, plus monster skill Flame of Evil #50)
- Version075 area-skill settings: 4 (Flame, Twister, Evil Spirit, Aqua Beam)
- Version075 magic effects: 5 initializers (ShieldSkill/Defense, GreaterDamage, GreaterDefense, Heal, Alcohol) + elemental effects created on demand (Poisoned, Iced, Freeze)
- Version095d skills: 35 (075 set + Cometfall, Inferno, Twisting Slash, Impale, Fire Breath; MagicGladiator added to class masks)
- Version095d area-skill settings: 6 (adds Cometfall, TripleShot)
- Version095d magic effects: same 5 initializers as 075
- VersionSeasonSix skills: 384 CreateSkill calls (file ~1082 lines)
- VersionSeasonSix area-skill settings: 28
- VersionSeasonSix master skill definitions: 78 AddMasterSkillDefinition calls + AddPassiveMasterSkillDefinition calls; 3 master skill roots; 12 combo steps for Blade Knight combo
- VersionSeasonSix magic effect initializers: 32
- SkillNumber enum: 385 named members, values 0-617 (300+ are master skills)
- MasterSkillTree.xml: 187 skill nodes (ID, Name, Rank, ReqID, MaxLevel) - Season 4+ data
- MagicEffectNumber enum: ~60 members, range -5 to 201 (negative and >=200 are internal, not sent to client)

## Entities

### skill definition (Skill)
- Number: short, unique skill id, client references skills by this number; pre-S3 range observed 1-50 (075) / 1-50 (095d); full enum 0-617
- Name: string (localized)
- AttackDamage: int, base attack damage, only relevant for attack skills; observed 3-120 pre-S3
- Requirements: list of AttributeRequirement (attribute ref + MinimumValue int); used for level requirement (Stats.Level), energy requirement (Stats.TotalEnergy), leadership requirement (Stats.TotalLeadership); observed energy 30-578, level 28-110
- ConsumeRequirements: list of AttributeRequirement; attribute values consumed on each cast: mana cost (Stats.CurrentMana, observed 1-250) and ability/AG cost (Stats.CurrentAbility; 0 for all 075/095d skills - AG costs appear only in SeasonSix data)
- Range: short, max distance in tiles between caster and target; observed 0-7 pre-S3 (0 = self/map-wide like Hellfire/Inferno/summons, 6 typical for ranged, 2-3 melee)
- DamageType: enum DamageType; pre-S3 uses None(-1), Physical(0), Wizardry(1); Curse(2)/SummonedMonster(3)/Fenrir(4)/All(5) are later
- SkillType: enum SkillType (see enums)
- Target: enum SkillTarget (see enums); pre-S3 skills all use Explicit
- ImplicitTargetRange: short, range for automatic additional targets around primary target; only effective if > 0 (e.g. Fireburst/Deathstab extra hits - both post-095d skills; 0 for all 075/095d skills)
- TargetRestriction: enum SkillTargetRestriction; pre-S3 usage: Self (Defense), Player (Heal, Greater Defense, Greater Damage), Undefined otherwise
- MovesToTarget: bool - skill teleports/moves attacker to target (DK weapon skills: Falling Slash, Lunge, Uppercut, Cyclone, Slash)
- MovesTarget: bool - target gets pushed to a random nearby tile on hit (same DK weapon skills)
- ElementalModifierTarget: optional reference to a resistance attribute (IceResistance, PoisonResistance, LightningResistance, FireResistance, EarthResistance, WindResistance, WaterResistance); semantic: hitting the target may apply the element's side effect; resistance value 255/255 (=1.0) means immune; monster resistances stored as n/255 fractions
- SkipElementalModifier: bool - element side-effect applies regardless of resistance (special-case skills; pre-S3 none set it except none in 075/095d... only SeasonSix skills like ChainDrive, Pollution, LightningShock, Earthshake, Explosion223, Requiem)
- MagicEffectDef: optional reference to a MagicEffectDefinition; applied for buff skills and for elemental status effects (Iced/Poisoned/Freeze)
- QualifiedCharacters: list of references to character classes allowed to learn/use the skill
- MasterDefinition: optional MasterSkillDefinition - Season 4+ master skill data, EXCLUDE
- AreaSkillSettings: optional embedded settings object for area skills (see entity)
- NumberOfHitsPerAttack: short, default 1
- NO cooldown field exists in the domain model (a cooldownMinutes parameter exists in the initializer signature but is never stored)
- item relationship: an ItemDefinition may reference a Skill - either a weapon that grants the skill to the skill list while equipped, or a consumable (orb/scroll) that teaches the skill when consumed
- elemental status effect wiring (pre-S3 relevant): Ice element -> Iced effect (number 0x38), duration 10s, power-ups: IsIced=1 and MovementSpeedFactor *0.5; Poison element -> Poisoned effect (0x37), duration 20s for Poison skill / 10s for Decay; effect SubType = 255 - elementalType ordinal; effect InformObservers=true, StopByDeath=true
- in 075/095d, after mapping, magic effect numbers of skill buffs are overwritten with the skill number itself (client convention: effect number = skill number for these versions)

### area skill settings (AreaSkillSettings)
- UseFrustumFilter: bool - filter targets by a frustum (directional cone/trapezoid) from caster
- FrustumStartWidth: float, tiles (observed 1.0-1.5 pre-S3)
- FrustumEndWidth: float, tiles (observed 1.5-4.5)
- FrustumDistance: float, tiles (observed 4-8)
- UseTargetAreaFilter: bool - filter targets by distance from target coordinate
- TargetAreaDiameter: float, tiles (observed 2)
- UseDeferredHits: bool - hits arrive delayed to match projectile travel
- DelayPerOneDistance: duration, delay per tile of distance (observed 0-300ms)
- DelayBetweenHits: duration (observed 500-1000ms)
- MinimumNumberOfHitsPerTarget: int (observed 0-1)
- MaximumNumberOfHitsPerTarget: int (observed 1-3)
- MinimumNumberOfHitsPerAttack: int - hits after this count get reduced chance
- MaximumNumberOfHitsPerAttack: int (observed 3 for TripleShot)
- HitChancePerDistanceMultiplier: float; chance to hit = multiplier^distance (e.g. 0.9^5 = 0.59); observed 0.5, 0.7, 1.0
- ProjectileCount: int, default 1; >1 = projectiles evenly distributed in frustum, target hit only if a projectile path crosses it (TripleShot = 3)
- EffectRange: int, max distance from target-area center
- pre-S3 skills with settings - 075: Flame(targetArea d2, maxHits/target 2, 0.5 chance mult, 500ms between hits), Twister(frustum 1.5/1.5/4, deferred 300ms/dist), EvilSpirit(deferred 100ms/dist, maxHits/target 2, 0.7), AquaBeam(frustum 1.5/1.5/8); 095d adds Cometfall(targetArea d2) and TripleShot(frustum 1/4.5/7, 3 projectiles, 3 hits max)

### magic effect definition (MagicEffectDefinition)
- Number: short, effect id known by the game client; negative numbers and numbers >= 200 are host-internal, never sent to client (Heal=-2, ShieldRecover=-3, ShieldSkill=200, Alcohol=201)
- Name: string (localized)
- SubType: byte; effects with the same SubType cannot stack - applying a new effect with same SubType removes the existing one; elemental effects use SubType = 255 - elementalType; potions use 255 - effectNumber
- InformObservers: bool - HOST-SIDE CLIENT CONCERN: whether the effect change is broadcast to observing players (visible effects like buffs true, self-only like heal/shield-skill false)
- StopByDeath: bool - effect is removed when the affected entity dies
- SendDuration: bool - HOST-SIDE CLIENT CONCERN: whether the remaining duration is sent to the client (true only for InfiniteArrow and DefenseReduction in dataset)
- DurationDependsOnTargetLevel: bool - duration divided by target level (used with divisors below); not used by pre-S3 effects
- MonsterTargetLevelDivisor: float, default 1; divisor applied against monster target level when DurationDependsOnTargetLevel
- PlayerTargetLevelDivisor: float, default 1; same for player targets
- Chance: optional PowerUpDefinitionValue, probability 0..1 of applying the effect; defaults to 1.0 when absent (only SeasonSix ChainDrive sets 0.4 in data)
- ChancePvp: optional PowerUpDefinitionValue; falls back to Chance when absent
- Duration: optional PowerUpDefinitionValue, seconds; can be constant + attribute-scaled (e.g. SoulBarrier 60s + energy/40 seconds) and capped by MaximumValue
- DurationPvp: optional PowerUpDefinitionValue, seconds; falls back to Duration
- PowerUpDefinitions: list of PowerUpDefinition - the actual stat boosts applied while the effect is active
- PowerUpDefinitionsPvp: list of PowerUpDefinition; falls back to PowerUpDefinitions when absent
- PvP variants (ChancePvp/DurationPvp/PowerUpDefinitionsPvp) exist in the current model; pre-S3 initializers never populate them - candidate for exclusion

### power-up definition (PowerUpDefinition)
- TargetAttribute: reference to an attribute definition (the stat being boosted)
- Boost: PowerUpDefinitionValue (see entity)
- a magic effect owns 1..n power-up definitions (e.g. Iced owns IsIced=1 and MovementSpeedFactor*0.5)

### power-up value (PowerUpDefinitionValue)
- ConstantValue: element with Value: float and AggregateType: enum (AddRaw | AddFinal | Multiplicate | Maximum) describing how the value combines with the target attribute
- RelatedValues: list of AttributeRelationship - attribute-scaled contributions added to the constant; each has InputAttribute (attribute ref), InputOperator (Multiply | Add | Exponentiate | ExponentiateByAttribute | Minimum | Maximum), InputOperand: float
- MaximumValue: optional float cap on the computed result (e.g. CriticalDamageIncrease duration capped at 180s)
- semantics: result = ConstantValue + sum(RelatedValues), clamped to MaximumValue

### learned-skill entry (SkillEntry)
- Skill: required reference to a skill definition
- Level: int - master skill level; always 0 for normal (pre-S3) skills; meaningful only for Season 4+ master skills - EXCLUDE the level semantics, keep entry as plain skill reference for pre-S3
- all other fields (PowerUps, PowerUpsPvp, PowerUpDuration, PowerUpChance, Attributes) are transient runtime caches, not persisted domain data

### attribute requirement (AttributeRequirement)
- Attribute: required reference to an attribute definition (Level, TotalEnergy, TotalLeadership, CurrentMana, CurrentAbility)
- MinimumValue: int - minimum required (for Requirements) or amount consumed per cast (for ConsumeRequirements)
- shared shape between skill requirements and item requirements

### skill combo (SkillComboDefinition + SkillComboStep)
- SkillComboDefinition: Name string, MaximumCompletionTime duration (3 seconds in data), Steps list
- SkillComboStep: Order int (steps with same Order are alternatives), IsFinalStep bool, Skill reference
- attached to a character class (Blade Knight has ComboDefinition); combo data only initialized in VersionSeasonSix but the DK combo mechanic itself is pre-S3 (introduced ~0.97/1.0); 075/095d initializers define no combo
- SeasonSix Blade Knight combo: step1 any of Slash/Cyclone/Lunge/FallingSlash/Uppercut, step2 any of TwistingSlash/RagefulBlow/DeathStab/StrikeofDestruction, step3(final) TwistingSlash/RagefulBlow/DeathStab

### buff/debuff concrete values (pre-S3 relevant effects)
- Defense skill (ShieldSkill effect 200, internal): duration 4s, DamageReceiveDecrement *0.5 (50% damage reduction), InformObservers false, StopByDeath true
- Greater Damage (effect 1): duration 60s, GreaterDamageBonus += 3 + TotalEnergy/7, InformObservers true, StopByDeath true
- Greater Defense (effect 2): duration 60s, DefenseFinal += 2 + TotalEnergy/8 (AddFinal), InformObservers true, StopByDeath true
- Heal (effect -2, internal, Regeneration type): instant, CurrentHealth += 5 + TotalEnergy/5, no duration
- Alcohol (effect 201, internal, from consuming Ale): duration 80s, SubType 54, AttackSpeedAny += 20, StopByDeath false
- Iced (effect 0x38=56): duration 10s, IsIced=1, MovementSpeedFactor *0.5, SubType 255-Ice
- Poisoned (effect 0x37=55): duration 20s (Poison) / 10s (Decay), IsPoisoned=1; damage ticks every 3s (comment: poison damages 7 times, decay 3 times)
- Freeze (effect 0x39=57, from Ice Arrow - Ice Arrow is 095d+/S6 elf skill): duration 5s, IsFrozen=1
- Soul Barrier (effect 4; skill exists from 0.97, not in 075/095d data): duration 60s + energy/40, SoulBarrierReceiveDecrement = 0.1 + energy/20000 + agility/5000 (10% + 1%/200energy + 1%/50agi), mana toll per hit = 2% of MaximumMana
- Swell Life / GreaterFortitude (effect 8; DK skill from 0.97, not in 075/095d data): duration 60s + energy/5, MaximumHealth *1.12 * (1+0.01/20)^energy * (1+0.01/100)^vitality
- Critical Damage Increase (effect 5, DL skill - DL is 0.97+, pre-S3): duration 60s + energy/10 capped 180s, CriticalDamageBonus += energy/30 + leadership/25, SubType 17
- Infinite Arrow (effect 6, Muse Elf 1.0+): duration 600s, AmmunitionConsumptionRate *0 (no arrows consumed), SendDuration true
- Defense Reduction (effect 0x3A=58): duration 10s, DefenseDecrement *0.9 (10% defense decrease), SendDuration true
- Stun (effect 0x3D=61): duration 2s, IsStunned=1 - castle siege skill, S3+, EXCLUDE
- Transparency/Invisible (effect 0x12=18): duration 1 day, IsInvisible=1 - castle siege skill, S3+, EXCLUDE
- Potion of Bless (effect 10): duration 120s, AttackDamageIncrease *1.2 - Siege potion, S3+, EXCLUDE
- Potion of Soul (effect 11): duration 60s, AttackSpeedAny +20, AbilityRecoveryAbsolute +8, Lightning/Ice resistance +0.5 - Siege potion, S3+, EXCLUDE
- Shield Recover (effect -3, internal): CurrentShield += level + energy/4 - shield/SD system is S3+, EXCLUDE

## Enums

### DamageType
- None = -1
- Physical = 0
- Wizardry = 1
- Curse = 2 (Summoner, S3+ - exclude)
- SummonedMonster = 3
- Fenrir = 4 (S2+ pet, likely exclude)
- All = 5

### SkillType
- DirectHit = 0
- CastleSiegeSpecial = 1 (S3+ - exclude)
- CastleSiegeSkill = 2 (S3+ - exclude)
- AreaSkillAutomaticHits = 3 (server computes hits in area)
- AreaSkillExplicitHits = 4 (client declares each hit)
- AreaSkillExplicitTarget = 5 (area animation, hits only explicit target)
- Buff = 10
- Regeneration = 11 (regenerates target attribute of the effect, e.g. Heal)
- PassiveBoost = 20 (applies without casting; used by master skills and elf pets)
- SummonMonster = 30
- Other = 40 (e.g. Teleport)

### SkillTarget
- Undefined = 0
- Explicit = 1
- ImplicitParty = 2 (party members in view range)
- ImplicitPlayersInRange = 3
- ImplicitNpcsInRange = 4
- ImplicitAllInRange = 5
- ExplicitWithImplicitInRange = 6 (primary target + all in ImplicitTargetRange of it)
- ImplicitPlayer = 7 (self only)

### SkillTargetRestriction
- Undefined = 0 (any entity)
- Self = 1
- Party = 2
- Player = 3 (players and their summons)

### ElementalType (initializer-side, maps to resistance attributes)
- Undefined
- Ice
- Poison
- Lightning
- Fire
- Earth
- Wind
- Water

### AggregateType (how a power-up combines with the target attribute)
- AddRaw
- AddFinal
- Multiplicate
- Maximum

### InputOperator (how an input attribute scales into a value)
- Multiply
- Add
- Exponentiate
- ExponentiateByAttribute
- Minimum
- Maximum

### MagicEffectNumber (client effect ids, pre-S3-relevant subset)
- Heal = -2 (internal)
- ShieldRecover = -3 (internal, S3+ SD)
- GreaterDamage = 1
- GreaterDefense = 2
- ElfSoldierBuff = 3
- SoulBarrier = 4
- CriticalDamageIncrease = 5
- InfiniteArrow = 6
- AbilityRecoverSpeedIncrease = 7
- GreaterFortitude(SwellLife) = 8
- Poisoned = 55 (0x37)
- Iced = 56 (0x38)
- Freeze = 57 (0x39)
- DefenseReduction = 58 (0x3A)
- Stunned = 61 (0x3D, S3+ CS)
- Transparency = 18 (S3+ CS)
- PotionOfBless = 10 / PotionOfSoul = 11 (S3+ CS)
- ShieldSkill = 200 (internal)
- Alcohol = 201 (internal)
- remaining members (Reflection 0x47, Sleep 0x48, Blind, Requiem 0x4A, Explosion 0x4B, Weakness 0x4C, Innovation 0x4D, Berserker 0x51, WizEnhance 0x52, Cold 0x56, 129-148 Rage Fighter/mastery, seals, scrolls, Jack O'Lantern) are S3+/S6 - exclude

## Version notes
- Version075 (30 skills): DW (Poison, Meteorite, Lightning, FireBall, Flame, Teleport, Ice, Twister, EvilSpirit, Hellfire, PowerWave, AquaBeam, EnergyBall), DK (Defense, FallingSlash, Lunge, Uppercut, Cyclone, Slash), Elf (TripleShot, Heal, GreaterDefense, GreaterDamage, 6 summons Goblin->Bali), monster skill FlameofEvil. TripleShot is AreaSkillExplicitHits (client declares hits) in 075.
- Version095d (35 skills): adds Cometfall, Inferno, TwistingSlash, Impale, FireBreath; MagicGladiator added as qualified class on most DW/DK skills (MG introduced in 0.9); TripleShot changed to AreaSkillAutomaticHits with 3-projectile frustum settings; Teleport stays DW-only; summon energy requirements raised (e.g. SummonGoblin 30->90).
- Both 075 and 095d create only 5 magic effects via initializers (ShieldSkill, GreaterDamage, GreaterDefense, Heal, Alcohol) plus on-demand elemental effects (Poisoned, Iced) attached to skills with elemental modifiers; effect numbers of skill buffs are then rewritten to equal the skill number.
- Both 075 and 095d: no AG(ability) costs, no ImplicitTargetRange usage, no skill AttributeRelationships, no combos, no master skills, no PvP-variant effect data.
- EvilSpirit range differs: 7 in 075, 6 in 095d.
- Special summon monster: both versions create monster Bali (number 150) specifically for the SummonBali skill; other summons reference regular monster definitions by number.
- VersionSeasonSix: 384 skills including Summoner (Curse damage type, book skills), Rage Fighter, castle siege skills, S4+ master skills (78+ master definitions across 3 roots with rank 1-9, level formulas as MathParser strings like '(1 + (((((((level - 30) ^ 3) + 25000) / 499) / 50) * 100) / 12))'), skill combos, and per-skill AttributeRelationships (e.g. Nova SkillBaseDamageBonus += TotalStrength/2; Earthshake += TotalStrength/10 + TotalLeadership/5 + HorseLevel*10).
- Skills that exist in retail pre-S3 but are absent from OpenMU 075/095d datasets (present only in SeasonSix dataset): SoulBarrier, IceStorm, Nova, RagefulBlow, DeathStab, SwellLife, IceArrow, Penetration, FireSlash, PowerSlash, Combo, FireBurst, Earthshake, ElectricSpike, IncreaseCriticalDamage, InfinityArrow, Stun-family, Summon(DL), etc. If mu-core targets 0.97/1.0-style pre-S3, these must be sourced from the SeasonSix initializer selectively.

## Post-S3 exclusions
- MasterSkillDefinition + MasterSkillRoot + MasterSkillTree.xml (187 nodes) - master skill system is Season 4+; Skill.MasterDefinition field and SkillEntry.Level (master level) semantics excluded with it
- SkillType.CastleSiegeSpecial (1) and CastleSiegeSkill (2) + CS skills (Stun 67, CancelStun 68, SwellMana 69, Invisibility 70, CancelInvisibility 71, AbolishMagic 72) + Stunned/Transparency effects + Potion of Bless/Soul, seal and scroll effects - Castle Siege is Season 3
- DamageType.Curse (2) and all Summoner skills/effects (DrainLife, ChainLightning, Sleep, Weakness, Innovation, Berserker, Reflection/DamageReflection, Requiem, Explosion, Pollution, book/stick mechanics) - Summoner is Season 3
- Rage Fighter skills and effects (KillingBlow, BeastUppercut, ChainDrive, DarkSide, DragonRoar, DragonSlasher, IgnoreDefense 129, IncreaseHealth 130, IncreaseBlock 131, DecreaseBlock 132, Charge, PhoenixShot, Cold effect 0x56) - Season 6
- DamageType.Fenrir (4) - Fenrir pet is Season 2+; decide separately if S2 content is out
- Shield/SD-related effects (ShieldRecover -3, SoulBarrierManaTollPerHit interplay with CurrentShield, MaximumSDincrease) - SD system is Season 3+
- MagicEffectDefinition PvP variants (ChancePvp, DurationPvp, PowerUpDefinitionsPvp) - only used by later-season balance, never populated pre-S3
- WizEnhance effects (0x52, 138, 139), CriticalDamageIncreaseMastery (148), ElfSoldierBuff via master formulas - S6/master-tree
- Skill combo definitions in SeasonSix data reference StrikeofDestruction (S2/S3 skill); the combo mechanic itself is 0.97+ but OpenMU only ships combo data in SeasonSix

## Open questions
- Exact pre-S3 cutoff: OpenMU's 095d dataset lacks many skills that existed in retail 0.97-1.02 (Soul Barrier, Ice Storm, Nova, Rageful Blow, Death Stab, Swell Life, Ice Arrow, Penetration, DL skill set, Combo). Should mu-core's schema seed from 095d as-is, or cherry-pick these from the SeasonSix initializer? User must decide the target sub-version.
- Skill cooldown: no cooldown field exists in the domain model (the initializer accepts cooldownMinutes but discards it). Include a cooldown field in the Rust schema anyway, or omit?
- Blade Knight combo: mechanic is pre-S3 (0.97+) but data only exists in the SeasonSix initializer and includes S2+ Strike of Destruction. Include combo entities?
- SkillEntry.Level: only meaningful for master skills (excluded). Keep learned-skill entry as a bare skill reference, or keep an (unused) level field for forward compatibility?
- Magic effect PvP variants (ChancePvp/DurationPvp/PowerUpDefinitionsPvp) and DurationDependsOnTargetLevel/divisors: present in the model but unused pre-S3. Include in schema or drop?
- InformObservers and SendDuration are host/networking concerns (who gets notified, whether duration is transmitted). Include as data flags in mu-core or leave to the host layer?
- Elemental immunity encoding: resistances are floats 0..1 (n/255), immunity = 255/255. Adopt the same fraction encoding or use raw 0-255 bytes?
- The Skill.AttributeRelationships collection (per-skill damage bonus scaling, e.g. Earthshake/Nova) is only populated in SeasonSix but Earthshake(DL horse skill) is 0.97+. Include the concept?
- Version075 Teleport is DamageType.Wizardry with 0 damage and SkillType.Other - decide whether teleport is a 'skill' in mu-core or a separate mechanic.
- ItemDefinition.Skill conflates two semantics (weapon grants skill while equipped vs consumable teaches skill) - OpenMU itself has a TODO to split them; mu-core should probably model them as two distinct relationships.
