# item definitions + item instances + storage/inventory (OpenMU domain facts for pre-Season-3 mu-core)

## Counts
- Version075 weapons: 64 (+2 ammunition: Bolt 4/7, Arrows 4/15) — group 0 swords: 16, group 1 axes: 9, group 2 maces/scepters: 7, group 3 spears: 10, group 4 bows/crossbows: 14, group 5 staves: 8
- Version075 armors: 90 — 15 shields (group 6), 15 helms (7), 15 armors (8), 15 pants (9), 15 gloves (10), 15 boots (11)
- Version075 wings: 3 (group 12 numbers 0-2: Wings of Elf, Wings of Heaven, Wings of Satan)
- Version075 orbs: 4 (group 12: Healing 8, Greater Defense 9, Greater Damage 10, Summoning 11 with MaximumItemLevel 5)
- Version075 scrolls: 12 (group 15 numbers 0-11, DW skills Poison..Aqua Beam)
- Version075 jewels: 3 (Jewel of Bless 14/13, Jewel of Soul 14/14, Jewel of Chaos 12/15)
- Version075 potions/consumables: 10 (group 14: Apple 0, Small/Medium/Large Healing 1-3, Small/Medium/Large Mana 4-6, Antidote 8, Ale 9, Town Portal Scroll 10)
- Version075 jewelery: 5 (group 13: Ring of Ice 8, Ring of Poison 9, Transformation Ring 10, Pendant of Lighting 12, Pendant of Fire 13)
- Version075 pets: 3 (group 13: Guardian Angel 0, Imp 1, Horn of Uniria 2)
- Version075 total item definitions: 196
- Version095d weapons: 69 (+2 ammunition) — adds over 075: Sword of Destruction 0/16, Dark Breaker 0/17, Thunder Blade 0/18, Saint Crossbow 4/(new), Staff of Destruction 5/(new)
- Version095d armors: 90 (identical name set to Version075)
- Version095d wings: 3 (same as 075)
- Version095d orbs: 5 (075's 4 + Orb of Twisting Slash 12/7)
- Version095d scrolls: 14 (075's 12 + Scroll of Cometfall, Scroll of Inferno)
- Version095d jewels: 4 (075's 3 + Jewel of Life 14/16)
- Version095d potions: 10 (reuses Version075 Potions initializer)
- Version095d jewelery: 5 (same items; pendants gain excellent damage options)
- Version095d pets: 4 (075's 3 + Horn of Dinorant 13/3 with its own 3-option definition)
- Version095d extras: Box of Luck 1 definition (14/11, levels 0-1 used) with ItemDropItemGroup tables; Devil Square event tickets 3 (Devil's Eye 14/17, Devil's Key 14/18, Devil's Invitation 14/19)
- Version095d total item definitions: ~215
- VersionSeasonSix weapons: 129 CreateWeapon calls incl. Bolt/Arrows — g0: 34, g1: 9, g2: 20, g3: 12, g4: 25, g5: 29 (staves+sticks+summoner books)
- VersionSeasonSix armors: 297 — 22 shields, 55 helms, 55 armors, 55 pants, 53 gloves, 57 boots; MaximumArmorLevel 15
- VersionSeasonSix wings: 18 wing definitions (1st/2nd/3rd wings, capes)

## Entities

### item definition
- identity: Group (byte, 0-15) + Number (short, unique within group). Source: /tmp/openmu-ref/src/DataModel/Configuration/Items/ItemDefinition.cs
- Name: string; may encode per-level names separated by ';' where token index = item level (GetNameForLevel)
- ItemSlot: optional reference to an item slot type; absent for non-equippable items (potions, jewels, scrolls kept in pack only)
- Width: byte, inventory cells horizontal; observed 1-2 for weapons/armor, up to 5 for wings (Wings of Heaven 5x3)
- Height: byte, inventory cells vertical; observed 1-4
- DropsFromMonsters: bool — whether monsters can drop it
- IsAmmunition: bool — item is ammo (bolts/arrows) for an equipped weapon
- IsBoundToCharacter: bool — cannot be traded/vaulted/shop-sold/picked by others (used mostly by S6 quest/event items; unused in 075/095d data)
- StorageLimitPerCharacter: int — max count of this item kind in a character inventory; 0 = unlimited
- DropLevel: byte — minimum monster level that can drop it
- MaximumDropLevel: optional byte — max monster level that drops it; only used by Jewel of Chaos (12..66) in 075/095d
- MaximumItemLevel: byte — max upgrade level; 11 in Version075/Version095d (Constants.MaximumItemLevel), 15 in SeasonSix; 0 for ammunition; small custom values for special items (Summon Orb 5, Transformation Ring = skinCount-1, Box of Luck 1, Devil's Eye 4)
- Durability: byte — maximum durability at item level 0; doubles as stack size for stackables (scroll/orb/jewel 1, potion 3, ammo 255, pet 255)
- Value: int — worth in zen (e.g. Jewel of Bless/Soul 150, Scroll of Fire Ball 300 .. Scroll of Inferno 265000, orbs 150-29000, potions 5-30)
- Skill: optional reference to a skill — for weapons/shields: the weapon skill granted while equipped (if instance HasSkill); for orbs/scrolls: the skill learned on consumption
- ConsumeEffect: optional reference to a magic effect applied when consumed (e.g. Ale -> Alcohol effect)
- QualifiedCharacters: set of character-class references allowed to equip; initializer input is per-class levels: 0=no, 1=base class, 2=second-stage class only; 075/095d parameterize DW/DK/Elf (MG auto-added to non-helm armor when class exists), S6 signature adds MG/DL/Summoner/RageFighter
- Requirements: list of attribute requirements (attribute ref + MinimumValue int). Stored raw; effective wear requirement computed at runtime: for wearables stat_req = (multiplier * effectiveDropLevel * rawValue / 100) + 20, multiplier=3 for str/agi/vit/leadership, 4 for energy; effectiveDropLevel = DropLevel + 3*itemLevel + 25 if excellent / +30 if ancient; strength req additionally + optionLevel*4 when item has an 'option'; Level requirement used raw (no scaling). Source: GameLogic/ItemExtensions.cs GetRequirement/CalculateRequirement/CalculateDropLevel
- BasePowerUpAttributes: list of item base power-ups (see entity below); e.g. weapon min/max phys dmg, attack speed, staff rise = magicPower/2, armor DefenseBase, shield DefenseShield + DefenseRatePvm, gloves AttackSpeed, boots WalkSpeed, wings absorb (DamageReceiveDecrement multiplicate 1-abs%), wings dmg increase (AttackDamageIncrease multiplicate 1+inc%), CanFly=1, flag attributes (EquippedWeaponCount=1, DoubleWieldWeaponCount=1 for 1-wide melee groups 0-2, IsTwoHandedWeaponEquipped=1 for 2-wide non-bow, AmmunitionConsumptionRate=1 for bows, IsShieldEquipped=1, jewelery elemental resistance +1 aggregated as Maximum, pet buffs, TransformationSkin via level table)
- PossibleItemOptions: references to item option definitions that may roll on this item (Luck; 'Option' = phys dmg / wiz dmg / defense / defense-rate; excellent sets in 095d+; wing-specific option lists; jewelery health-recover option; harmony/guardian S6 only)
- PossibleItemSetGroups: references to item set groups (plain full-set bonuses in 075/095d; ancient sets S6)
- DropItems: list of ItemDropItemGroup — per-source-item-level drop tables used when a player drops this item on the ground (Box of Luck etc.); ItemDropItemGroup adds: SourceItemLevel byte, MoneyAmount int, MinimumLevel/MaximumLevel byte (level range of generated item), RequiredCharacterLevel short, DropEffect enum
- PetExperienceFormula: optional string formula (variable 'level') for trainable pets — no trainable pets in 075/095d data
- MaximumSockets: int — POST-S3, always 0 pre-S3

### item base power-up definition
- TargetAttribute: reference to an attribute definition
- BaseValue: float — value at item level 0
- AggregateType: enum (AddRaw, AddFinal, Multiplicate, Maximum) — how the value combines into the attribute
- BonusPerLevelTable: optional reference to an item level bonus table; total at level L = BaseValue + table[L]

### item level bonus table
- Name, Description: strings
- BonusPerLevel: list of (Level int, AdditionalValue float); sparse — entries with value 0 are omitted; Level ranges 0..MaximumItemLevel (0..11 pre-S3, 0..15 S6)
- weapon damage increase (min+max dmg): {0,3,6,9,12,15,18,21,24,27,31,36} (levels 0-11; '+3/level, +1 extra after 10')
- staff rise, even magic power: {0,3,7,10,14,17,21,24,28,31,35,40,(45,50,56,63)}; odd: {0,4,7,11,14,18,21,25,28,32,36,40,(45,51,57,63)} — base staff rise = magicPower/2
- armor defense increase: {0,3,6,9,12,15,18,21,24,27,31,36,(42,49,57,66 S6-only levels 12-15)}
- shield defense increase: +1 per level {0..15}; shield defense-rate increase uses the armor curve
- wing absorb: -2% damage per level (DamageReceiveDecrement -0.02*level); wing damage increase: +2%/level (1st and 3rd wings), +1%/level (2nd wings); wing defense bonus = armor curve (3rd-wing variant {0,4,8,...,81} S6)
- jewelery elemental resistance: {0,1,2,3,4} (+1 per level)
- ammunition damage increase %: 095d {0,0.03,0.05}; S6 {0,0.03,0.05,0.07}
- running movement speed: fixed bonus for boots/underwater-gloves from item level 5 up (aggregate Maximum)
- transformation ring skins: one monster-skin number per item level
- instance durability bonus (hardcoded in logic, not a table entity): AdditionalDurabilityPerLevel {0,1,2,3,4,6,8,10,12,14,17,21,26,32,39,47}; +20 if ancient, +15 if excellent, capped at 255. Source: GameLogic/ItemExtensions.cs

### attribute requirement
- Attribute: reference to attribute definition (Level, TotalStrengthRequirementValue, TotalAgilityRequirementValue, TotalEnergyRequirementValue, TotalVitalityRequirementValue, TotalLeadershipRequirementValue; orbs also use TotalEnergy/TotalStrength/TotalAgility directly)
- MinimumValue: int
- requirements with value 0 are simply not stored

### item slot type
- Description: string; ItemSlots: list of int slot indices this type maps to
- Version075 initializes 12 slot types: LeftHand[0], RightHand[1], LeftOrRightHand[0,1], Helm[2], Armor[3], Pants[4], Gloves[5], Boots[6], Wings[7], Pet[8], Pendant[9], Ring[10,11]
- weapon slot assignment rule in data: 1-wide weapon usable by knight class gets LeftOrRightHand, otherwise the specific hand slot

### item instance
- ItemSlot: byte — slot index within its storage
- Definition: required reference to item definition (by group+number)
- Durability: double — current durability; for stackables = current stack count; max of one piece = def.Durability + durability-per-level table + ancient/excellent bonus (see bonus table entity); non-wearable piece max = 1, trainable pet = 255
- Level: byte — upgrade level, 0..MaximumItemLevel of the definition (0-11 pre-S3, 0-15 S6); also reused as sub-kind selector for multi-purpose items (summon orb level = summoned monster, transformation ring level = skin, box of luck level = box kind)
- HasSkill: bool — instance grants the definition's weapon skill while equipped ('+Skill' roll)
- ItemOptions: list of item option links; luck = link to a Luck-type option; 'option' (+dmg/+def) = link with Level 1..MaximumOptionLevel (4 in 075/095d weapons/armor; 3 for jewelery health-recover); excellent = links to Excellent-type options (095d+)
- ItemSetGroups: list of ItemOfItemSet references; entry with AncientSetDiscriminator != 0 marks the item ancient (ancient data only shipped in S6 init)
- StorePrice: optional int — player-set personal store price
- SocketCount: int — POST-S3 (socket weapons/armor); always 0 pre-S3
- PetExperience: int — trainable pets only; not applicable to 075/095d pets

### item option link (instance option)
- ItemOption: required reference to a concrete increasable item option (from the definition's possible options)
- Level: int — option level (e.g. option +4/+8/... levels 1-4; option value may alternatively depend on item level when the option's LevelType = ItemLevel)
- Index: int — ordering index, only needed for sorted options i.e. sockets (POST-S3)

### item storage
- Items: collection of item instances, each positioned by its ItemSlot byte
- Money: int — zen stored alongside (inventory money, vault money); global caps exist as config values MaximumInventoryMoney / MaximumVaultMoney (int)
- owners: character Inventory (ItemStorage), account Vault (ItemStorage) with string VaultPassword; trade uses a transient temporary storage; personal shop is a slot-range view over the character inventory storage

### inventory layout constants
- equipped slots: indices 0-11 (12 slots): 0 LeftHand, 1 RightHand, 2 Helm, 3 Armor, 4 Pants, 5 Gloves, 6 Boots, 7 Wings, 8 Pet, 9 Pendant, 10 Ring1, 11 Ring2
- defense-item slots = 2..7 plus 10,11 (IsDefenseItemSlot)
- main inventory grid: 8 rows x 8 columns = 64 slots, indices 12..75; slot index = 12 + row*8 + col; an item occupies Width x Height cells anchored at its slot
- inventory extensions: up to 4 extensions x (4 rows x 8) = 128 extra slots — code comment states this is only valid in Season 6; before, there are no extensions and the store begins right after slot 75 (POST-S3 for mu-core)
- personal store: 4 rows x 8 = 32 slots appended after inventory (index 76+ pre-extension era; 204+ in S6)
- temporary storage (trade window / chaos machine): 4 rows x 8 = 32 slots
- vault (warehouse): 15 rows x 8 = 120 slots; account flag IsVaultExtended doubles it to 240 (later-season feature)
- source: /tmp/openmu-ref/src/DataModel/InventoryConstants.cs

## Enums

### ItemGroups (ItemDefinition.Group values)
- 0 Swords
- 1 Axes
- 2 Scepters/Maces
- 3 Spears
- 4 Bows/Crossbows
- 5 Staves
- 6 Shields
- 7 Helm
- 8 Armor
- 9 Pants
- 10 Gloves
- 11 Boots
- 12 Orbs (also wings + Jewel of Chaos)
- 13 Misc1 (pets, rings, pendants)
- 14 Misc2 (potions, jewels, event items)
- 15 Scrolls
- 0xF0 pseudo-group 'Weapon' used only for S6 guardian option lookup — not a valid Group value

### AggregateType (power-up aggregation)
- AddRaw
- AddFinal
- Multiplicate
- Maximum

### wing OptionType (data-building enum for wing options)
- HealthRecover (+0.01 health recovery mult/level)
- PhysDamage (+4/level)
- WizDamage (+4/level)
- CurseDamage (+4/level, Summoner post-S3)
- Defense (+4/level)

### ItemDropEffect (on ItemDropItemGroup)
- Undefined
- Fireworks
- ChristmasFireworks
- FanfareSound
- Swirl

### equipment slot indices
- 0 LeftHand
- 1 RightHand
- 2 Helm
- 3 Armor
- 4 Pants
- 5 Gloves
- 6 Boots
- 7 Wings
- 8 Pet
- 9 Pendant
- 10 Ring1
- 11 Ring2

## Version notes
- Maximum item level: 075 and 095d cap at 11 (Version075/Items/Constants.cs, Version095d/Items/Constants.cs: MaximumItemLevel=11, MaximumOptionLevel=4); SeasonSix uses 15. The DataModel field supports 0-15.
- Item option types available: 075 registers only Option + Luck; 095d adds Excellent (ExcellentOptions initializer, rings get excellent defense options, pendants excellent damage options); Harmony, Guardian, Socket, Ancient options are S6-only initializers.
- Armor set bonuses exist in both 075/095d via BuildSets(): per armor-set (grouped by Number across groups 7-11): full-set defense-rate bonus x1.1 (any level), and for set level 10..maxLevel a defense multiplier 1 + (setLevel-9)*0.05; requires all 5 pieces (MinimumItemCount = group size).
- 095d Weapons adds an ammunition damage-increase level table ({0,3%,5%}) absent in 075; ammunition items themselves exist in both.
- 095d adds: BoxOfLuck (with ItemDropItemGroup drop tables incl. SpecialItemType.RandomItem chance 0.5, money fallback), Devil Square event tickets, Jewel of Life, Horn of Dinorant (with 3 possible 'option'-type bonuses: DamageReceiveDecrement x0.95, MaximumAbility +50, AttackSpeed +5), Orb of Twisting Slash, 2 scrolls, 5 weapons.
- Class qualification in 075/095d data uses 3 class flags (DW/DK/Elf) with auto MG inclusion for body armor when MG class exists in config; 095d weapon rows already carry values 2 (second-stage class only, e.g. Dark Breaker) and a 4th flag column on some rows; S6 signature has 7 class-level parameters.
- SlotTypesInitializer lives in Version075 namespace but is shared by all versions via GameConfigurationInitializerBase.
- Trade/vault/store sizes are version-independent constants except: inventory extensions (S6 only) shift the personal-store start index from 76 to 204; pre-S3 layout = 12 equipped + 64 inventory + 32 store.
- Weapons/armor stats live as data rows (name, droplevel, min/max dmg, attack speed, durability, magic power, level/str/agi/energy/vit requirements, class flags) in Version075/Items/Weapons.cs, Version095d/Items/Weapons.cs, and Armors.cs of each version — full value tables are large; shape is uniform per the CreateWeapon/CreateArmor/CreateShield/CreateGloves/CreateBoots signatures.

## Post-S3 exclusions
- Sockets entirely: ItemDefinition.MaximumSockets, Item.SocketCount, ItemOptionLink.Index (only needed for socket ordering), SocketSystem.cs (S6), seed spheres/seeds.
- Harmony options (HarmonyOptions.cs, S6) and Jewel of Harmony.
- Guardian options / '380 level' items (GuardianOptions.cs, IsGuardian, pseudo item group 0xF0).
- Inventory extensions (4 x 32 = 128 slots) — S6 feature per code comment in InventoryConstants; pre-S3 inventory is 12 + 64 slots with store at index 76.
- Extended vault (Account.IsVaultExtended doubling 120 -> 240 slots) — later-season feature.
- Trainable pets: PetExperienceFormula, Item.PetExperience (Dark Horse / Dark Raven / Fenrir) — none exist in 075/095d datasets.
- S6 wing tiers (2nd wings partially 0.97+, 3rd wings S3+), capes, Summoner books/sticks, Rage Fighter items, item levels 12-15, S6 Misc/Quest/PackedJewels items.
- Ancient sets as shipped (AncientSets.cs is S6-only; Item.ItemSetGroups AncientSetDiscriminator) — see open question.
- CurseDamage wing option (Summoner, S6).
- ItemDefinition.IsBoundToCharacter / StorageLimitPerCharacter — fields exist in model but are only exercised by S6 quest/event items.

## Open questions
- Item level range for mu-core schema: task brief says 0-15, but both pre-S3 reference datasets (075, 095d) cap MaximumItemLevel at 11 and their level-bonus tables only cover 0-11. Store byte 0-15 capacity with per-version cap 11, or hard-cap 11?
- Ancient sets: historically present around 0.97-1.0 (pre-S3), but OpenMU only initializes ancient data in VersionSeasonSix. Include the ancient concept (ItemOfItemSet + AncientSetDiscriminator + bonus-per-piece) in the pre-S3 schema or exclude?
- Personal store (StorePrice, 32-slot store segment): introduced around 0.99/1.0 — inside or outside pre-S3 scope? OpenMU models it in all versions.
- Excellent items: 095d has them, 075 does not. Is mu-core's target closer to 0.75 (no excellent) or 0.95+ (excellent)?
- Which item set of the two pre-S3 datasets is authoritative for mu-core: 075 (196 defs) or 095d (~215 defs incl. Dinorant, Box of Luck, Devil Square tickets)?
- Requirement formula constants (multiplier 3/4, +20, +25 excellent, +30 ancient, +3*itemLevel, +4*optionLevel str) are hardcoded in OpenMU game logic, not data. Encode as fixed formula in mu-core or as data?
- Instance durability bonus table {0,1,2,3,4,6,8,10,12,14,17,21,26,32,39,47} (+20 ancient/+15 excellent, cap 255) is hardcoded logic — same question: formula/table location in mu-core?
- ItemDefinition.Value semantics: zen worth used for buy/sell pricing; most equipment rows leave it 0 (price computed from drop level at runtime elsewhere). Keep as optional override?
- LocalizedString for names/descriptions is an OpenMU concern — assume plain string in mu-core?
- Money caps (MaximumInventoryMoney, MaximumVaultMoney) live on game configuration, not on storage — where should mu-core put them?
