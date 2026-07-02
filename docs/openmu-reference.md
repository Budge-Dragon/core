# OpenMU Domain Reference Protocol

The OpenMU repo ([github.com/MUnique/OpenMU](https://github.com/MUnique/OpenMU)) is our **domain reference only** — a map of what exists in MU Online: entities, attributes, stats, item options, drop mechanics, crafting recipes, formulas. It is never a design or architecture reference.

## Strict Rules

1. **Extract WHAT, never HOW.** Use these files to answer "what fields/stats/mechanics exist?" — never copy their class design, inheritance, patterns, or architecture.
2. OpenMU is C# OOP with EF persistence baked in. We are NOT. This project is a pure Rust logic crate: plain data structs + free functions, no OOP, no inheritance, no persistence concerns, no host dependencies.
3. All design decisions follow OUR rules ([`CLAUDE.md`](../CLAUDE.md), [`README.md`](../README.md)): tick-based, injected RNG, no I/O, events returned not dispatched, serializable plain data. If OpenMU's structure conflicts with our rules, our rules win — always.
4. When mapping an entity: list the domain facts from the reference (e.g. "an item has: level 0–15, durability, luck flag, skill flag, excellent options, ancient set, sockets"), then design our own struct from scratch per our conventions.
5. Ignore anything in OpenMU related to: persistence, networking, plugins, views, EF navigation properties, virtual/ICollection patterns, GUIDs-as-identity.

## Workflow When Planning Any Feature

a. Check the relevant reference file(s) → extract the domain facts as a plain list.
b. Present that list to the user for confirmation of scope.
c. Design our own entities/services per OUR rules — from scratch, no OpenMU structure carried over.

## Reference Files

### Entities

- [Character.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Entities/Character.cs)
- [Item.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Entities/Item.cs)
- [ItemStorage.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Entities/ItemStorage.cs)
- [ItemOptionLink.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Entities/ItemOptionLink.cs)
- [SkillEntry.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Entities/SkillEntry.cs)
- [Guild.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Entities/Guild.cs)

### Static Data Schemas

- [GameConfiguration.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/GameConfiguration.cs)
- [CharacterClass.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/CharacterClass.cs)
- [Skill.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/Skill.cs)
- [MagicEffectDefinition.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/MagicEffectDefinition.cs)
- [MonsterDefinition.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/MonsterDefinition.cs)
- [MonsterSpawnArea.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/MonsterSpawnArea.cs)
- [GameMapDefinition.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/GameMapDefinition.cs)
- [DropItemGroup.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/DropItemGroup.cs)
- [StatAttributeDefinition.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/StatAttributeDefinition.cs)
- [JewelMix.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/JewelMix.cs)
- [LevelDependentDamage.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/LevelDependentDamage.cs)

### Item System

- [ItemDefinition.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/Items/ItemDefinition.cs)
- [ItemOptionDefinition.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/Items/ItemOptionDefinition.cs)
- [ItemOption.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/Items/ItemOption.cs)
- [ItemOptionTypes.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/Items/ItemOptionTypes.cs)
- [IncreasableItemOption.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/Items/IncreasableItemOption.cs)
- [ItemSetGroup.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/Items/ItemSetGroup.cs)
- [ItemLevelBonusTable.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/Items/ItemLevelBonusTable.cs)
- [ItemSlotType.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/Items/ItemSlotType.cs)
- [ItemBasePowerUpDefinition.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/Items/ItemBasePowerUpDefinition.cs)

### Crafting

- [ItemCrafting.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/ItemCrafting/ItemCrafting.cs)
- [SimpleCraftingSettings.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/Configuration/ItemCrafting/SimpleCraftingSettings.cs)

### Formulas / Mechanics

- [Stats.cs](https://github.com/MUnique/OpenMU/blob/master/src/GameLogic/Attributes/Stats.cs) — master stat list, **read first**
- [AttackableExtensions.cs](https://github.com/MUnique/OpenMU/blob/master/src/GameLogic/AttackableExtensions.cs) — damage calc
- [DefaultDropGenerator.cs](https://github.com/MUnique/OpenMU/blob/master/src/GameLogic/DefaultDropGenerator.cs) — drops
- [HitInfo.cs](https://github.com/MUnique/OpenMU/blob/master/src/GameLogic/HitInfo.cs)
- [InventoryConstants.cs](https://github.com/MUnique/OpenMU/blob/master/src/DataModel/InventoryConstants.cs)

### Mechanics Coverage Checklist

- [GameLogic/PlayerActions](https://github.com/MUnique/OpenMU/tree/master/src/GameLogic/PlayerActions) — folder names = feature list
