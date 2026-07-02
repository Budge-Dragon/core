# monsters/NPCs + spawn areas + game maps + gates/warps (OpenMU domain model, pre-Season-3 focus)

## Counts
- Version075 maps: 8 (0 Lorencia, 1 Dungeon, 2 Devias, 3 Noria, 4 LostTower, 5 Exile, 6 Arena, 7 Atlans)
- Version075 monster/NPC definitions: 73 total = 54 map monsters+4-of-those-are-traps (Lorencia 8, Dungeon 14 incl. 3 traps, Devias 7, Noria 8, LostTower 9 incl. 1 trap, Atlans 8) + 18 NPCs/guards/soccerball (NpcInitialization) + 1 summon monster (Bali #150)
- Version075 spawn areas: 1564 CreateMonsterSpawn calls across its 8 maps (0.75 uses mostly single-point spawns)
- Version075 gates/warps: 32 exit gates, 23 enter gates, 14 warp entries
- Version095d maps: 14 registered = 0.75's 8 (Dungeon/LostTower/Exile/Arena/Atlans reused; Lorencia/Devias/Noria re-derived with 0.95 terrain) + Tarkan (8), Icarus (10), DevilSquare1-4 (all number 9, discriminators 1-4)
- Version095d additional monster/NPC definitions: 27 = Tarkan 7 (#57-63), Icarus 9 (#69-77), DevilSquare3 3, DevilSquare4 1, NPCs 3 (#235-237), invasion mobs 4 (#43 Golden Budge Dragon, #44 Red Dragon, #53 Golden Titan, #54 Golden Soldier); rest shared with 0.75
- Version095d spawn areas in own map files: 314 (plus reused 0.75 maps)
- Version095d gates/warps: 36 exit gates, 23 enter gates, 14 warp entries (warp list identical to 0.75, flagged 'todo')
- VersionSeasonSix maps: 68 files in Maps/ (2 abstract bases: BloodCastleBase, KalimaBase); GameMapsInitializer registers 73 map initializers incl. reused 0.75 Exile/Arena and 0.95d Tarkan/DevilSquare1-4
- VersionSeasonSix monster definitions: 367; spawn calls: 4412; gates/warps: 226 exit, 117 enter, 40 warp entries

## Entities

### monster/NPC definition (MonsterDefinition; one type covers monsters, merchants, guards, traps, statues, soccer ball)
- number: i16, unique id, identifies the model/NPC on the client; Version075 data uses 0-52 (map monsters), 100-103 (traps), 150 (summoned 'Bali'), 200 (Soccerball), 238-255 (town NPCs/guards); Version095d adds 43/44/53/54 (invasion mobs), 57-77 (Tarkan/Icarus monsters), 235-237 (NPCs)
- designation: string, monster/NPC name; server-side informational only (client knows names by number)
- move_range: u8, tiles of random movement; observed 0 (traps/NPCs) or 3 (all mobile monsters/guards); OpenMU marks it 'not used yet'
- attack_range: u8, tiles within which it attacks without moving closer; observed 1 (melee) to 6 (ranged/magic), guards 2-5, traps 0-4
- view_range: i16 in the model, byte-sized in data; tiles for target recognition; observed 1-7 (typical monsters 5-7, traps 1-4)
- move_delay: duration, ms per movement step; typical 400 ms; absent/zero for passive NPCs and traps
- attack_delay: duration, ms between attacks; observed 1000 (traps) - 2200, typical 1600-2000, guards 1500
- respawn_delay: duration, seconds until a dead instance respawns; observed 3 s (most), 10 s (some), 100 s (summoned monster Bali)
- attribute: u8, semantics unknown even to OpenMU (TODO comment: 'maybe max concurrent magic effects'); observed values: 1 for guards/traps, 2 for monsters; carried verbatim from Monsters.txt column
- number_of_maximum_item_drops: i32, max item drops on death; 0 for NPCs/guards/traps, 1 for all normal monsters
- npc_window: enum NpcWindow, dialog opened when player talks to the NPC (Undefined for monsters)
- object_kind: enum NpcObjectKind, discriminates monster vs passive NPC vs guard vs trap etc.; Monster is the default (=0)
- intelligence_type_name: optional string naming the AI behavior; pre-S3 usages: GuardIntelligence (guards), AttackSingleWhenPressedTrapIntelligence, AttackAreaWhenPressedTrapIntelligence, RandomAttackInRangeTrapIntelligence, AttackAreaTargetInDirectionTrapIntelligence (traps); default = basic aggressive monster AI
- attack_skill: optional reference to a skill definition ('attack type' from Monsters.txt); skill's additional damage is NOT applied, but its magic effects are; pre-S3 examples: Meteorite (Lich), Poison, Lightning, PowerWave, EnergyBall, Ice, Meteorite/FlameOfEvil etc.
- merchant_store: optional reference to an item storage (only merchant NPCs); store contents live in Version075/MerchantStores.cs
- item_craftings: collection of crafting recipes (only crafting NPCs, e.g. Chaos Goblin #238)
- drop_item_groups: references to drop item groups for monster-specific special drops (e.g. bosses); empty for normal monsters
- attributes: list of (attribute-definition ref, f32 value) pairs = the monster's stat block (see 'monster stat attribute' entity)
- quests: collection of quest definitions startable at this NPC (relevant for quest NPCs, e.g. Sevina #235 in 0.95d)

### monster stat attribute (MonsterAttribute: attribute ref + f32 value)
- attribute: reference to a stat attribute definition (shared with character stat system)
- value: f32
- stats used in Version075/Version095d monster data: Level (4-90), MaximumHealth (60-10000+), MinimumPhysBaseDmg, MaximumPhysBaseDmg, DefenseBase, AttackRatePvm, DefenseRatePvm, PoisonDamageMultiplier (0.03 on poison-attack monsters), WindResistance, PoisonResistance, IceResistance, WaterResistance, FireResistance, CanFly (Icarus monsters)
- resistances unit: fraction 0..1; original Monsters.txt stores 0-255, converted as raw/255 (per the documented extraction regex in BaseMapInitializer.cs)
- guard stat block example (Crossbow/Berdysh Guard #247/#249): Level 90, MaximumHealth 10000, PhysDmg 180-195, AttackRatePvm 300, DefenseRatePvm 100, DefenseBase 70
- trap stat block example (#100 Lance Trap): Level 80, MaximumHealth 1000, PhysDmg 100-110, AttackRatePvm 400, DefenseRatePvm 500, no defense/no level-based stats beyond that

### monster spawn area (MonsterSpawnArea)
- monster: reference to a monster/NPC definition (by number in source data)
- map: reference to a game map definition
- x1, y1: u8, top-left corner of spawn rectangle (validation: x1<=x2, y1<=y2)
- x2, y2: u8, bottom-right corner; point spawn when x1==x2 && y1==y2 (all town NPCs and most 0.75 monster spawns are points)
- quantity: i16, number of instances to spawn inside the rectangle; observed 1 (point spawns) up to 45 (Lorencia area spawns); event maps use ~35 per wave
- direction: enum Direction, facing on spawn (mostly meaningful for NPCs; Undefined for area monster spawns)
- spawn_trigger: enum SpawnTrigger (Automatic for normal monsters; Wandering for wandering merchants #248/#250; wave/event triggers for Devil Square etc.)
- wave_number: u8, event wave this spawn belongs to (Devil Square 0.95d: waves 1, 2, 3, boss wave 10); 0 for non-wave spawns
- maximum_health_override: optional i32, per-spawn HP override; null = use definition HP; only used by Season-6 Blood Castle (castle door / crystal statue per castle level) - effectively unused pre-S3
- spawns also carry a per-map ordinal number in the initializers (used only as a stable id, 1..n NPC spawns, then monster spawns)

### game map definition (GameMapDefinition)
- number: i16, client map id; 0.75: 0-7; 0.95d adds 8 (Tarkan), 9 (Devil Square), 10 (Icarus)
- name: string (Lorencia, Dungeon, ...)
- discriminator: i32, distinguishes multiple map definitions sharing one client number (Devil Square 1-4 are all map number 9 with discriminators 1-4); 0 for normal maps
- terrain_data: byte blob from original *.att file; shape = 3-byte header + 65536 cells (256x256); cell index i -> x = i & 0xFF, y = (i >> 8) & 0xFF; cell value 0 = walkable, 1 = walkable + safezone, any other value = unwalkable; original .att flag bits: Safezone=1, Character=2, Blocked=4, NoGround=8, Water=16
- map size: fixed 256x256 tiles, coordinates are u8
- exp_multiplier: f64; set to 1 for every map in all three shipped datasets
- safezone_map: reference to the map where a player respawns after death (self if the map has a spawn gate, otherwise Lorencia by default); the actual point is the target map's ExitGate with is_spawn_gate=true
- exit_gates: owned list of ExitGate (spawn points and warp targets located ON this map)
- enter_gates: owned list of EnterGate (tiles on this map that teleport the player elsewhere)
- monster_spawns: owned list of spawn areas
- drop_item_groups: references to map-wide drop groups (default: money + random-item groups added to every map; special maps can differ)
- map_requirements: list of (attribute ref, minimum value) a character must satisfy to enter; only pre-S3 usage: Icarus requires CanFly >= 1
- character_power_ups: list of (attribute ref, value, aggregate type) applied to every character on the map; only pre-S3 usage: Atlans sets IsUnderwater = 1
- battle_zone: optional battle-zone definition; only used by Arena (battle soccer)

### gate (common base of enter/exit gates)
- x1, y1: u8, top-left corner of the gate rectangle
- x2, y2: u8, bottom-right corner (point gate when equal)

### enter gate (EnterGate: tile area that teleports the player)
- inherits gate rectangle (x1,y1,x2,y2 on the map that owns it)
- number: i16, the gate id from the original Gate.txt (client references gates by this number)
- target_gate: required reference to an ExitGate (possibly on another map)
- level_requirement: i16, minimum character level to pass; observed 0-60 in 0.75/0.95d data
- no fee field on enter gates - fees exist only on warp-list entries

### exit gate (ExitGate: arrival area on a map)
- inherits gate rectangle; player appears at a random point inside it
- map: required reference to the map the player arrives on
- direction: enum Direction the player faces on arrival; in the 0.75/0.95d init data the raw Gate.txt byte is used directly (0 treated as Undefined)
- is_spawn_gate: bool; true = town/safezone spawn point selectable as respawn target (flag 0 rows of Gate.txt), false = only reachable as a target of an EnterGate (flag 2 rows)

### warp list entry (WarpInfo; the 'warp command/menu' list, global not per-map)
- index: i32, position/id in the warp list
- name: string, e.g. 'Arena', 'LostTower7'
- costs: i32, zen fee; 0.75 values 2000-8000
- level_requirement: i32, minimum character level; 0.75 values 10-70
- gate: required reference to an ExitGate (the destination)
- 0.75 full table (index, name, cost, minLevel -> gate#): 1 Arena 2000 50 g50; 2 Lorencia 2000 10 g17; 3 Noria 2000 10 g27; 4 Devias 2000 20 g22; 5 Dungeon 3000 30 g2; 6 Dungeon2 3500 40 g6; 7 Dungeon3 4000 50 g10; 8 LostTower 5000 50 g42; 9 LostTower2 5500 50 g31; 10 LostTower3 6000 50 g33; 11 LostTower4 6500 60 g35; 12 LostTower5 7000 60 g37; 13 LostTower6 7500 70 g39; 14 LostTower7 8000 70 g41
- 0.75 has no graphical warp menu; the same list serves chat warp commands; 0.95d ships the identical 14 entries (marked 'todo: update for 0.95d' in OpenMU)

### battle zone definition (Arena battle soccer only)
- type: enum BattleType (Normal | Soccer)
- left_team_spawn_point x/y: optional u8 / u8
- right_team_spawn_point x/y: optional u8 / u8
- ground: rectangle (x1,y1,x2,y2 u8)
- left_goal, right_goal: rectangles
- Arena data: spawns L(60,156) R(60,164), ground (55,141)-(69,180), left goal (61,139)-(63,140), right goal (61,181)-(63,182); soccer ball is NPC #200 spawned on the map

### movement speed constants (src/Persistence/Initialization/MovementSpeedConstants.cs; item/effect side of movement)
- running_gear_minimum_level: 5 (item level to grant run speed)
- running_gear_movement_speed: 15.0
- default_wing_movement_speed: 15.0
- fast_wing_movement_speed: 16.0
- basic_mount_movement_speed: 15.0 (post-S3 relevance: mounts)
- horse_or_fenrir_movement_speed: 17.0 (Dark Horse pre-S3-adjacent; Fenrir is S2+)
- upgraded_fenrir_movement_speed: 19.0
- iced_movement_speed_factor: 0.5 (multiplier while iced)
- cold_movement_speed_factor: 0.33 (multiplier while slowed by cold)

### map initializer output (what one map's static data consists of, per BaseMapInitializer)
- emits: map-local monster definitions (added to the global monster list, keyed by number), then the map record: number, name, discriminator, terrain blob (from .att resource, version-prefixed '075_' for 0.75 terrains), exp_multiplier = 1, NPC spawn list + monster spawn list, map attribute requirements, drop item groups (global defaults: money + random items), optional battle zone / character power-ups
- safezone_map reference is resolved in a second pass after all maps exist
- monster/NPC definitions are global and shared across maps; spawn areas are per-map

## Enums

### NpcObjectKind
- Monster (=0, default)
- PassiveNpc (merchants, quest NPCs, storage/vault NPCs)
- Guard (attacks aggressors, GuardIntelligence)
- Trap
- Gate (event gate object, e.g. blood castle gate)
- Statue
- SoccerBall
- Destructible

### NpcWindow (dialog opened on talk) - pre-S3 subset
- Undefined
- Merchant
- Merchant1
- Storage
- VaultStorage (Baz #240)
- ChaosMachine (Chaos Goblin #238)
- DevilSquare (Charon #237, 0.95d)
- BloodCastle (Archangel Messenger)
- GuildMaster (#241)
- LegacyQuest (Sevina #235, 0.95d)
- PetTrainer (uncertain era - trainer NPC exists from ~0.97/1.0)
- Lahap (uncertain era)
- CastleSeniorNPC (castle siege - uncertain vs S3 cutoff)

### SpawnTrigger
- Automatic (normal respawning monsters)
- AutomaticDuringEvent
- OnceAtEventStart (event gates/statues, golden monsters)
- AutomaticDuringWave (Devil Square waves)
- OnceAtWaveStart (Devil Square boss wave)
- ManuallyForEvent (chaos castle enemies)
- Wandering (wandering merchants, one spawn active at a time across maps)

### Direction (OpenMU encoding; original client uses 0=West without an Undefined slot, i.e. client value = openmu value - 1)
- Undefined=0
- West=1
- SouthWest=2
- South=3
- SouthEast=4
- East=5
- NorthEast=6
- North=7
- NorthWest=8

### TerrainAttributeType (.att cell flag bits)
- Safezone=1
- Character=2 (runtime-only occupancy flag)
- Blocked=4
- NoGround=8
- Water=16

### BattleType
- Normal (pvp)
- Soccer (battle soccer)

## Version notes
- Version075 gate direction bytes are used raw from Gate.txt where 0 means Undefined; all spawn-gate rows have direction 0. OpenMU's Direction enum shifts client values by +1 elsewhere.
- Version075 warp list is command-based (no warp menu in 0.75); 14 entries, fees 2000-8000 zen, level 10-70.
- Version095d reuses Version075 map initializers for Dungeon, LostTower, Exile, Arena, Atlans; Lorencia/Devias/Noria get 0.95 terrain files (no '075_' resource prefix) but inherit the 0.75 monster data; NpcInitialization inherits 0.75's and adds #235 Sevina (LegacyQuest), #236 Golden Archer, #237 Charon (DevilSquare window).
- Version095d introduces: Devil Square mini-game (map 9 with discriminators 1-4, wave-based spawns: waves 1/2/3 + boss wave 10), Golden/Red Dragon invasions (InvasionMobsInitialization), Icarus with CanFly>=1 map requirement, Tarkan.
- MonsterSpawnArea.wave_number and the wave-related SpawnTrigger values first become meaningful in 0.95d (Devil Square); 0.75 uses only Automatic and Wandering plus point NPC spawns.
- MonsterSpawnArea.maximum_health_override exists in the model but is only populated by Season-6 Blood Castle data.
- Exp multiplier is 1.0 for every map in every shipped dataset - the field exists but is never varied.
- Terrain files: 0.75 uses its own '075_Terrain{n}.att' resources; 0.95d/S6 use 'Terrain{n}[_discriminator].att'. Blob layout identical (3-byte header + 256x256 cells).
- VersionSeasonSix extends monster stat blocks with more attributes (e.g. lightning resistance, critical damage etc.) and ~300 more monsters, mini-game definitions (Blood Castle 1-8, Chaos Castle 1-7, Illusion Temple, Doppelganger, Imperial Guardian), and a 40-entry warp menu - use only as a lookup reference, not as pre-S3 source data.

## Post-S3 exclusions
- Season-6-only map data: Karutan1/2, LorenMarket, Vulcanus, DuelArena (+DuelConfiguration/DuelArea), Doppelgaenger1-4, FortressOfImperialGuardian1-4, Raklion/RaklionBoss, SwampOfCalmness, KanturuRuins/KanturuRelics/KanturuEvent (Kanturu = Season 3), IllusionTemple1-6 (Season 3), Elvenland + SilentMap (Season 4), SantaVillage
- NpcWindow values that are Season-3+: ElphisRefinery, RefineStoneMaking, RemoveJohOption (Jewel of Harmony, S3), IllusionTemple, ChaosCardCombination, CherryBlossomBranchesAssembly, SeedMaster, SeedResearcher (sockets, S4), StatReInitializer, DelgadoLuckyCoinRegistration, DoorkeeperTitusDuelWatch, LugardDoppelgangerEntry, JerintGaionEvententry, JuliaWarpMarketServer, CombineLuckyItem, NpcDialog
- MiniGameDefinition/MiniGameSpawnWave/MiniGameReward machinery as configured for S6 events (Devil Square itself is pre-S3, but OpenMU's full mini-game config lives in S6 initializers)
- MonsterSpawnArea.maximum_health_override usage (S6 Blood Castle door/statue HP tables)
- Movement speed constants for mounts/Fenrir (Fenrir = Season 2 item but S3+ upgrade tiers; basic/horse constants borderline)
- S6 monster stat extensions beyond the 0.75/0.95d stat set (extra resistances/multipliers)

## Open questions
- MonsterDefinition.Attribute (u8, values 1=NPC/guard/trap, 2=monster in shipped data): semantics unknown even to OpenMU. Keep as opaque byte, or drop since ObjectKind already discriminates?
- IntelligenceTypeName is a free-form string in OpenMU (GuardIntelligence + 4 trap AIs pre-S3). Model as enum of behavior kinds in mu-core?
- Exact pre-S3 map cutoff must be decided: Aida, CrywolfFortress, BarracksOfBalgass/BalgassRefuge (Season 2), ValleyOfLoren + castle siege, LandOfTrials, BloodCastle 7/8, ChaosCastle tiers, Kalima 7 - which are in scope? OpenMU only ships 0.75 (8 maps) and 0.95d (14) datasets below S6, so pre-S3 maps beyond those would need data from the S6 initializers with version filtering.
- 0.95d warp list is a verbatim copy of 0.75's with an OpenMU 'todo: update for 0.95d' - accept as-is or source authentic 0.95 move list (fees/levels)?
- Direction encoding choice: OpenMU's 1-8 with 0=Undefined vs original client 0-7 (client = openmu - 1). Gate direction bytes in 0.75/0.95 initializers are raw casts where 0=Undefined - decide one canonical encoding.
- ViewRange is declared i16 while all data fits u8 - which width for the schema?
- Keep per-map exp_multiplier (always 1.0 in data) and MaximumHealthOverride (S6-only usage) in a pre-S3 schema, or omit?
- SafezoneMap default rule (self if map has spawn gate, else Lorencia): encode as explicit reference per map, or as a derivation rule?
- NpcWindow values with uncertain introduction era relative to S3: PetTrainer, Lahap, CastleSeniorNPC, Storage vs VaultStorage distinction - include which?
- Wandering merchants (SpawnTrigger.Wandering, NPCs #248/#250) imply cross-map single-instance logic - in scope for mu-core spawn model?
