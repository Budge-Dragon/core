# drop system + chaos machine crafting + jewel mixes

## Counts
- Version075 chaos mixes: 1 (Chaos Weapon)
- Version095d chaos mixes: 6 (Chaos Weapon #1, Devil's Square Ticket #2, +10 #3, +11 #4, Dinorant #5, 1st Wings #11)
- VersionSeasonSix craftings: 35 total (29 at Chaos Goblin, 1 Elphis, 2 Pet Trainer, 1 Osbourne, 1 Jerridon, 1 Cherry Blossom Spirit)
- Jewel mixes: Version075 = 0, Version095d = 0, VersionSeasonSix = 10 (numbers 0-9)
- Default global drop item groups: Version075 = 3 (money 0.5, random 0.3, jewel 0.001), Version095d/S6 = 4 (adds excellent 0.0001)
- MixResult enum values: 4; SpecialItemType enum values: 7; ItemDropEffect enum values: 5; ResultItemSelection enum values: 2

## Entities

### drop item group (DropItemGroup)
- description: string (display only)
- chance: double, 0.0-1.0; chance >= 1.0 means the group is 'guaranteed' and is processed before chance-based groups
- minimum_monster_level: optional u8; null = no lower bound; group skipped if monster level below it
- maximum_monster_level: optional u8; null = no upper bound; group skipped if monster level above it
- monster: optional reference to a monster definition; null = valid for all monsters; used for quest-item drops bound to specific monsters
- item_level: optional u8; fixed item level assigned to the dropped item instance (e.g. Broken Sword+1 quest item); if null, level stays 0 unless the group is an item-drop group
- item_type: enum SpecialItemType (None, Ancient, Excellent, RandomItem, SocketItem, Money, Jewel)
- possible_items: list of references to item definitions; may be empty for special types (Money/RandomItem/Excellent/Ancient/SocketItem generated from global pools)
- attachable to: maps, monsters, characters (quest items), and the global game configuration; source file /tmp/openmu-ref/src/DataModel/Configuration/DropItemGroup.cs

### item-sourced drop group (ItemDropItemGroup, subtype of DropItemGroup; for boxes like Box of Luck)
- attached to an item definition's DropItems collection (consumed when the player drops the box on the ground)
- source_item_level: u8; the +level of the box item this group applies to (Box of Luck +0 vs Box of Kundun +8..+11 share one item definition)
- money_amount: i32; amount of zen dropped when item_type == Money (e.g. Box of Luck money fallback = 10,000)
- minimum_level / maximum_level: u8; dropped item's level is random in [minimum_level, maximum_level] inclusive (Box of Luck 095d: 6..6)
- required_character_level: i16; character level required to open/drop this group
- drop_effect: enum ItemDropEffect (Undefined, Fireworks, ChristmasFireworks, FanfareSound, Swirl); client-side effect on apply
- selection among an item's groups: order groups by chance ascending, roll random 0..1 (scaled by total chance if total > 1), walk list subtracting chance until one is hit; Money-typed group returns money instead of an item
- example data (095d Box of Luck, group 14 number 11): 0.5 chance RandomItem group at level 6 from a fixed list (weapons, staffs, jewels Chaos/Bless/Soul, armor sets Bronze/Pad/Bone/Leather/Scale/Sphinx/Brass/Vine/Silk/Wind), plus 1.0-chance money group of 10,000 zen as fallback; file /tmp/openmu-ref/src/Persistence/Initialization/Version095d/Items/BoxOfLuck.cs

### drop decision flow (DefaultDropGenerator, /tmp/openmu-ref/src/GameLogic/DefaultDropGenerator.cs)
- constants: BaseMoneyDrop = 7; DropLevelMaxGap = 12; SkillDropChancePercent = 50; item option level drop default max = 3 (valid config range 1-4)
- group sources per kill: monster.DropItemGroups + character.DropItemGroups + map.DropItemGroups + active-quest drop groups (party-aware); destructible objects use ONLY monster groups; character/map/quest groups are filtered by min/max monster level and monster binding
- number of drop attempts = monster.NumberOfMaximumItemDrops (i32 on monster definition); guaranteed groups (chance >= 1.0) consume attempts first, then remaining attempts each roll on the chance-based groups
- chance-group selection: totalChance = sum of chances; threshold = random 0..1, multiplied by totalChance if totalChance > 1; walk groups subtracting chance; miss possible (no drop that attempt)
- money drop: when the selected group has item_type == Money and no possible items: money = gainedExperience + 7; global flag ShouldDropMoney decides drop-on-ground vs direct add; global attribute MoneyAmountRate (default 1.0) scales money
- item level from monster level (RandomItem/global pools): item.Level = min((monsterLevel - itemDef.DropLevel) / 3, itemDef.MaximumItemLevel)  [integer division]
- droppable pool for RandomItem at monsterLevel: items with DropsFromMonsters && DropLevel <= monsterLevel && (MaximumDropLevel null or monsterLevel <= MaximumDropLevel) && DropLevel > monsterLevel - 12
- group with possible items, not monster-specific: filter possible items by DropLevel <= monsterLevel && (isJewelGroup || DropLevel == 0 || DropLevel > monsterLevel - 12); monster-specific groups skip the filter
- item level assignment for group drops: ItemDropItemGroup -> random [MinimumLevel, MaximumLevel]; else group.ItemLevel if set; else 0; always clamped to itemDef.MaximumItemLevel
- random options on any dropped item: for each option definition with AddsRandomly (excluding excellent), roll AddChance up to MaximumOptionsPerItem times (standard Option/Luck definitions: AddChance 0.25, max 1 per item); option level chosen randomly from defined levels <= MaximumItemOptionLevelDrop (config default 3)
- skill: 50% chance if the item can have a skill; durability set to max of one piece
- excellent drop: requires monsterLevel >= ExcellentItemDropLevelDelta (config, default 25); pool computed at (monsterLevel - 25); excellent items ALWAYS get skill (if item has one); first excellent option added unconditionally, each further option rolled with the excellent option definition's AddChance up to its MaximumOptionsPerItem
- ancient drop: random item from all droppable items that have an ancient set; always gets skill; random ancient set assigned plus its bonus option (e.g. +5/+10 stat) at a random defined level
- socket item drop (SpecialItemType.SocketItem): same as random but pool restricted to items with MaximumSockets > 0; dropped items with sockets get SocketCount = random 1..MaximumSockets  [post-S3]
- default global drop groups (all versions, GameConfigurationInitializerBase): Money chance 0.5; RandomItem chance 0.3; Excellent chance 0.0001 (only when version has excellent options, i.e. 095d/S6, not 075); Jewel chance 0.001
- related game config fields: ShouldDropMoney bool; ItemDropDuration (60 s default); MaximumItemOptionLevelDrop u8 (3); ExcellentItemDropLevelDelta u8 (25); ClampMoneyOnPickup bool; MaximumInventoryMoney i32

### jewel mix (JewelMix, /tmp/openmu-ref/src/DataModel/Configuration/JewelMix.cs)
- number: u8, client-facing mix id
- single_jewel: reference to the single jewel item definition
- mixed_jewel: reference to the packed/stacked jewel item definition
- banked stack semantics (ItemStackAction): valid stack sizes 10/20/30; packed item level = (stackSize / 10) - 1 (so +0=10, +1=20, +2=30); packed item durability = 1, size 1x1, MaximumItemLevel = 2
- combine fee = (stackSize / 10) * 500,000 zen; dismantle fee = 1,000,000 zen flat; unpacking yields (level + 1) * 10 single jewels, each needing a free inventory slot
- combining requires exactly stackSize matching single jewels in inventory and the Lahap NPC window open
- S6 mix list (mix number -> single jewel item group/number -> packed item): 0 Bless (14,13 -> 12,30), 1 Soul (14,14 -> 12,31), 2 Life (14,16 -> 12,136), 3 Creation (14,22 -> 12,137), 4 Guardian (14,31 -> 12,138), 5 Gemstone (14,41 -> 12,139), 6 Harmony (14,42 -> 12,140), 7 Chaos (12,15 -> 12,141), 8 Lower Refine Stone (14,43 -> 12,142), 9 Higher Refine Stone (14,44 -> 12,143)

### crafting recipe (ItemCrafting)
- number: u8, client-facing crafting id
- name: string
- handler kind: string class name; domain-wise an enum of behaviors: Default(simple), ChaosWeaponAndFirstWings, SecondWings, ThirdWings, Dinorant, DarkHorse, FenrirUpgrade, RefineStone, RestoreItem, GuardianOption, BloodCastleTicket, DevilSquareTicket, IllusionTempleTicket; empty string = plain simple handler
- simple_crafting_settings: optional embedded settings (ticket craftings have none; their data is hardcoded tables)

### crafting settings (SimpleCraftingSettings)
- money: i32, flat zen price
- money_per_final_success_percentage: i32; final price = money + money_per_final_success_percentage * successRate (chaos weapon/1st wings: 10,000/percent)
- npc_price_divisor (settings-level): i32; if > 0 the success rate is ENTIRELY sum(NPC 'old buying' prices of all input items) / npc_price_divisor, replacing the additive path (chaos weapon & 1st wings: 20,000)
- success_percent: u8 base success rate
- maximum_success_percent: u8; if > 0, caps the rate; rate always finally clamped to 100
- multiple_allowed: bool; result item count = count of items matched by the Reference>0 requirement (used by Potion of Bless/Soul to convert whole stacks)
- success_percentage_addition_for_luck: i32 (e.g. +25 on item upgrades)
- success_percentage_addition_for_excellent_item: i32 (S6 upgrades: -10)
- success_percentage_addition_for_ancient_item: i32 (S6 upgrades: -10)
- success_percentage_addition_for_guardian_item: i32 ('380 item', S6 upgrades: -10) [post-S3]
- success_percentage_addition_for_socket_item: i32 (S6 upgrades: -20) [post-S3]
- these per-flag additions are applied once per matched input item that has the flag
- result_item_select: enum ResultItemSelection { Any = one random result item, All = every result item }
- result_item_luck_option_chance: u8 percent (2nd wings/cape: 20; 3rd wings: 5)
- result_item_skill_chance: u8 percent (Dinorant/Dark Horse/Fenrir: 100)
- result_item_excellent_option_chance: u8 percent (2nd wings/cape: 20; cherry blossom: 100); rolled repeatedly up to the excellent option definition's MaximumOptionsPerItem; excellent result always gains skill
- result_item_max_exc_option_count: u8; set in data (1) but NOT read by any game logic in this codebase (open question)
- required_items: list of ItemCraftingRequiredItem; result_items: list of ItemCraftingResultItem
- success roll: random percent < successRate; on success apply each requirement's SuccessResult, on failure apply FailResult

### crafting required item (ItemCraftingRequiredItem)
- possible_items: list of item definition references; EMPTY list = 'any item' matching the other constraints
- minimum_item_level / maximum_item_level: u8 (e.g. chaos weapon input: 4..max)
- required_item_options: list of item option TYPE references (e.g. must have 'Option', must have 'Excellent', must have 'AncientBonus', must have 'HarmonyOption'); item matches only if it has an option of every listed type
- minimum_amount / maximum_amount: u8; 0 minimum = optional ingredient; amount counted by durability for stackable items; exceeding maximum_amount (when > 0) is an error
- success_result / fail_result: enum MixResult { Disappear=0, StaysAsIs=1, ChaosWeaponAndFirstWingsDowngradedRandom=2, ThirdWingsDowngradedRandom=3 }
- npc_price_divisor (per-requirement): i32; adds sum(NPC buying price of matched items) / divisor percent to the rate (2nd wings: 1st-wing input 4,000,000, exc item 40,000; 3rd wings stage1 ancient 300,000, stage2 exc 3,000,000)
- add_percentage: u8; rate += add_percentage * (count - minimum_amount) per extra item (Guardian 380 option: 50/60/70 by level band; Talisman of Luck remark: min 0 max 1 add 25)
- reference: u8; links requirement to a result item with the same reference for in-place modification (item upping); 0 = no link
- downgrade semantics ChaosWeaponAndFirstWingsDowngradedRandom (on fail): level -> random 0..(level-1); 50% chance to lose skill (if not excellent); 50% chance item option -1 level (removed if level 1); durability rescaled proportionally
- downgrade semantics ThirdWingsDowngradedRandom (on fail): level -2 or -3 (50/50); item option removed entirely; durability reset to max

### crafting result item (ItemCraftingResultItem)
- item_definition: optional item definition reference; null means 'modify referenced input item' via reference
- random_minimum_level / random_maximum_level: u8; created item level = random inclusive in that range (chaos weapons: 0..4)
- durability: optional u8; explicit durability of the created item (potions 10, pets 255, stones 1); null = maximum durability of one piece
- reference: u8; when > 0, the input items with matching reference get level += add_level and durability rescaled to new maximum (item upping +10..+15)
- add_level: u8 (item upgrades: 1)
- special case: 'Fruits' result item level is weighted-random 0-4 with weights 30/25/20/20/5

### handler-specific result option formulas (not in settings data)
- ChaosWeaponAndFirstWings: roll i = random 0..2; item option granted with chance (successRate/5 + 4*(i+1)) percent at option level (3 - i); luck granted with chance (successRate/5 + 4) percent; skill granted with chance (successRate/5 + 6) percent
- SecondWings: roll 0..2 -> (chance,level) = (20%,1) / (10%,2) / (4%,3) for the wing item option (~11% overall); luck/exc from settings chances
- Dinorant: 30% chance of an item option (random of the dinorant options), then 20% chance of a second different bonus option; option levels forced by target attribute: DamageReceiveDecrement=1, MaximumAbility=2, others=4; also validates each Horn of Uniria input has full durability (255)
- event tickets (BC/DS/IT): require ingredient1 + ingredient2 of EQUAL item level + 1 Jewel of Chaos; result item level = input level; Devil's Square: success 80% (level<5) else 70%, price by level 1-7 = 100k/200k/400k/700k/1.1m/1.6m/2m; Blood Castle: success 80%, price level 1-8 = 50k/80k/150k/250k/400k/600k/850k/1.05m; Illusion Temple: success 70%, price level 1-6 = 3m/5m/7m/9m/11m/13m

### chaos mix catalog per version
- Version075 (1 mix): #1 Chaos Weapon — inputs: 1 random item lvl 4..11 with Option (fail=ChaosWeaponDowngrade), 1+ Jewel of Chaos, 0+ Bless, 0+ Soul; rate = totalItemNpcPrice/20,000 (price 10,000 zen per final percent); results (Any): Chaos Dragon Axe / Chaos Nature Bow / Chaos Lightning Staff at random level 0-4
- Version095d (6 mixes): #1 Chaos Weapon (same as 075); #5 Dinorant — 1 Chaos + 3 Horn of Uniria, 70%, 250,000 zen, skill 100%; #3 '+10 Item' — item at +9 + 1 Chaos + 1 Bless + 1 Soul, 50% (+25 luck), 2,000,000 zen; #4 '+11 Item' — item at +10 + 1 Chaos + 2 Bless + 2 Soul, 45% (+25 luck), 4,000,000 zen; #11 1st Level Wings — 1 chaos weapon (groups 4/6, 2/6, 5/7) lvl 4..11 with Option (fail=downgrade) + optional random item lvl4+ with Option + 1+ Chaos + 0+ Bless/Soul, rate = price/20,000, results (Any): Fairy/Heaven/Satan wings; #2 Devil's Square Ticket (handler table)
- VersionSeasonSix (35 mixes) chaos goblin: #1 Chaos Weapon (input max lvl 15); #6 Fruits (1 Chaos + 1 Creation, 90%, 3m zen); #5 Dinorant (10 Uniria + 1 Chaos, 70%, 500k); #15 Potion of Bless (1+ Bless -> Siege Potion lvl0 dur10, 100%, 100k, multiple); #16 Potion of Soul (1+ Soul -> Siege Potion lvl1 dur10, 100%, 50k, multiple); item upgrades #3(+10) #4(+11) #22(+12) #23(+13) #49(+14) #50(+15) — formula: money = 2,000,000*(target-9); success% = 60 - ((target-10)/2*5) [int div: 60,60,55,55,50,50]; bless&soul count = target-9; +25 luck, -10 exc, -10 ancient, -10 380item, -20 socket; #8 Blood Castle Ticket; #2 Devil's Square Ticket; #37 Illusion Temple Ticket; #17 Life Stone (1 Guardian + 5 Bless + 5 Soul + 1 Chaos, 100%, 5m); #30/31/32 Small/Medium/Large Shield Potion (3 Large Healing / 3 Small Complex / 3 Medium Complex, 50%/30%/30%, 100k/500k/1m); #25 Fenrir Stage 1 (20 Bless of Guardian + 20 Splinter of Armor + 1 Chaos, 70% -> Fragment of Horn); #26 Fenrir Stage 2 (5 Fragment of Horn + 10 Claw of Beast + 1 Chaos, 50% -> Broken Horn); #27 Fenrir Stage 3 (1 Broken Horn + 3 Life + 1 Chaos, 30%, 10m -> Horn of Fenrir dur255 skill100%); #28 Fenrir Upgrade (handler); #11 1st Wings (adds Wings of Misery result); #24 Cape of Lord/Fighter (1st wing + optional exc item + 1 Chaos + 1 Loch's Feather+1[Monarch's Crest], 5m, max 90%, luck20/exc20/maxexc1); #7 2nd Wings (1st wing [npcDiv 4m] + optional exc item lvl4+ [npcDiv 40k] + 1 Chaos + 1 Loch's Feather, 5m, max 90%, luck 20%, exc 20% max 1 -> Spirit/Soul/Dragon/Darkness/Despair wings); #38 3rd Wings Stage1 (2nd wing/cape +9..15 with Option [fail=ThirdWingsDowngrade] + ancient item +7..15 [npcDiv 300k, fail=downgrade] + 1 Chaos + 1 Creation + 1 Packed Soul(10), rate 1% + price-additions max 60%, 200k per final percent -> Feather of Condor); #39 3rd Wings Stage2 (exc item +9..15 [npcDiv 3m] + 1 Chaos + 1 Creation + 1 Packed Soul + 1 Packed Bless + 1 Feather of Condor + 1 Flame of Condor, 1% base max 40%, 200k/percent, luck 5% -> Storm/Eternal/Illusion/Ruin wings, Cape of Emperor, Dimension, Cape of Overrule); #36 Guardian/380 Option (item bands +4-6/+7-9/+10-15 add 50/60/70% + 1 Harmony + 1 Guardian, 10m); #46 Complete Secromicon (6 fragments, 100%, 1m); other NPCs: #33 Gemstone Refinery (Elphis, 1 Gemstone -> 1 Jewel of Harmony, 80%); #13 Dark Horse (1 Spirit + 5 Bless + 5 Soul + 1 Chaos + 1 Creation, 60%, 5m, dur255 lvl1 skill100%); #14 Dark Raven (1 Spirit+1 + 2 Bless + 2 Soul + 1 Chaos + 1 Creation, 60%, 1m, dur255 lvl1); #34 Refine Stone (Osbourne, up to 32 exc + up to 32 normal items); #35 Restore Item/remove JOH (Jerridon, 100%); #41 Cherry Blossom Event Mix (255 Golden Cherry Blossom Branch -> random exc item from groups 7-11 numbers 0-15, exc chance 100% max 1)

## Enums

### SpecialItemType
- None
- Ancient
- Excellent
- RandomItem
- SocketItem (post-S3)
- Money
- Jewel

### ItemDropEffect
- Undefined
- Fireworks
- ChristmasFireworks
- FanfareSound
- Swirl

### MixResult
- Disappear = 0
- StaysAsIs = 1
- ChaosWeaponAndFirstWingsDowngradedRandom = 2
- ThirdWingsDowngradedRandom = 3

### ResultItemSelection
- Any = 0 (one random result)
- All = 1 (all results)

### crafting handler kind (from ItemCraftingHandlerClassName strings)
- Simple/default
- ChaosWeaponAndFirstWings
- SecondWings
- ThirdWings (post-S3)
- Dinorant
- DarkHorse
- FenrirUpgrade
- RefineStone (post-S3)
- RestoreItem (post-S3)
- GuardianOption (post-S3)
- BloodCastleTicket
- DevilSquareTicket
- IllusionTempleTicket (post-S3)

## Version notes
- Version075: only 1 crafting (Chaos Weapon); no jewel mixes; no excellent option type at all, so no excellent drop group and SpecialItemType.Excellent unused; option types limited to Option+Luck; version max item level constant = 11 (crafting input caps use 11, not 15).
- Version095d: adds Dinorant (3 horns, 250k — S6 uses 10 horns, 500k), +10/+11 upgrades (success 50%/45% flat, luck +25, NO excellent/ancient/380/socket penalties — those penalty fields are S6 data), 1st Wings, Devil's Square ticket; has excellent options and the 0.0001 excellent drop group; has Box of Luck ItemDropItemGroup data; still no jewel mixes; max item level constant = 11.
- VersionSeasonSix: full 35-mix catalog, jewel mixes (10), upgrades to +15 with success formula 60-((target-10)/2*5) and penalty fields, input max item level 15.
- The core schemas (DropItemGroup, ItemDropItemGroup, JewelMix, ItemCrafting, SimpleCraftingSettings, required/result items, MixResult) are shared across all versions; only the data differs.
- Default drop groups (money 0.5 / random item 0.3 / jewels 0.001 / excellent 0.0001) come from a base initializer shared by all three versions; excellent group is conditional on the version having excellent options.
- Drop generator formulas (item level = (monsterLevel - DropLevel)/3 capped, DropLevelMaxGap 12, money = exp + 7, skill 50%, excellent pool at monsterLevel - 25) are version-independent engine behavior driven by config values.

## Post-S3 exclusions
- SpecialItemType.SocketItem drops, SocketCount rolling on drop, socket-items-per-monster-level pool (Season 4 sockets)
- SimpleCraftingSettings.SuccessPercentageAdditionForSocketItem and SuccessPercentageAdditionForGuardianItem ('380 item') fields (S6 data only)
- Jewel of Harmony ecosystem: Gemstone Refinery (#33), Refine Stone (#34), Restore Item / remove JOH option (#35), Guardian/380 Option crafting (#36), HarmonyOption required-option type (Season 4)
- 3rd Level Wings Stage 1/2 (#38/#39), MixResult.ThirdWingsDowngradedRandom, Feather/Flame of Condor (Season 3)
- S6 wing additions in 1st/2nd wing mixes: Wings of Misery (12,41), Wings of Despair (12,42), Wings of Dimension (12,43), Cape of Fighter (12,49), Cape of Overrule (12,50) — Summoner/Rage Fighter wings (S6)
- Item upgrades beyond +11: #22 (+12), #23 (+13), #49 (+14), #50 (+15) and item level range >11 generally (pre-S3 max +11 per version constants)
- Illusion Temple Ticket (#37, Season 3)
- Shield (SD) potions #30/#31/#32 and Complex Potions, Siege/'Potion of Bless/Soul' craftings #15/#16 (SD stat and castle siege potions are Season 3+ in this shape)
- Complete Secromicon (#46, Season 6), Cherry Blossom Event Mix (#41, seasonal S4+)
- Packed jewels for Life/Creation/Guardian/Gemstone/Harmony/Refine Stones (mixes 2-6, 8-9); Lower/Higher Refine Stone entirely
- Excellent-option chance fields on 2nd-wing results predate S3 only partially — 2nd wings themselves are pre-S3 (0.97+) but the S6 exc-option/luck result chances are S6 data values

## Open questions
- Jewel mixes exist only in the S6 dataset (Lahap NPC). For a pre-S3 target: include jewel mixes at all? If yes, which jewels (Bless/Soul/Chaos are period-plausible; Life/Creation debatable; the packed-item ids used are S6 item numbers)?
- Fenrir stages #25-28 and Dark Horse/Dark Raven (#13/#14), Life Stone (#17), Fruits (#6), Cape of Lord (#24), Blood Castle ticket (#8), 2nd Wings (#7) exist only in the S6 dataset but are Season 1-2 era content — user must decide which belong in the pre-S3 catalog and with which numbers (095d dataset omits them).
- SimpleCraftingSettings.ResultItemMaxExcOptionCount is set in data (1) but never read by any logic in this codebase; the generic handler bounds excellent options by the option definition's MaximumOptionsPerItem instead. Decide whether to keep the field.
- Chaos Weapon / 1st Wings success rate depends on an NPC item price calculator (sum of input items' NPC prices / 20,000) — the price formula lives in the items/price area, not here; schema must reference it.
- 095d Box of Luck data gives dropped items fixed level 6 (MinimumLevel=MaximumLevel=6) — verify against the intended classic behavior before adopting.
- Version075 ChaosMixes is marked '// todo' in the initializer — the 075 catalog (1 mix) may be intentionally incomplete; classic 0.75 servers had no Chaos Machine at all until 0.9x. Decide the 0.75 stance.
- Money drop formula 'money = gainedExperience + 7' ties money to the exp calculation (party/level-gap adjusted?) — confirm which experience value feeds it in your engine.
- ItemDropItemGroup.RequiredCharacterLevel and DropEffect are used only by S6 box data in practice; decide if the pre-S3 schema keeps them.
- SimpleItemCraftingHandler rate arithmetic uses byte casts that can wrap (e.g. rate += (byte)(...)); treat the domain formula as plain integer math with clamps, not the C# overflow behavior.
