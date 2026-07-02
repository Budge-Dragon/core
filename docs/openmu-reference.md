# OpenMU Domain Reference Protocol

The OpenMU repo ([github.com/MUnique/OpenMU](https://github.com/MUnique/OpenMU)) is our **domain reference only** — a map of what exists in MU Online: entities, attributes, stats, item options, drop mechanics, crafting recipes, formulas. It is never a design or architecture reference.

## Strict Rules

1. **Extract WHAT, never HOW.** Use these files to answer "what fields/stats/mechanics exist?" — never copy their class design, inheritance, patterns, or architecture. **This includes a class's *field layout*, not just its methods: a reference's data model IS its architecture** (see [The data-model trap](#the-data-model-trap--a-references-data-model-is-its-architecture) below). Copying `ItemOptionDefinition`'s or `DropItemGroup`'s fields into a renamed struct is copying HOW.
2. OpenMU is C# OOP with EF persistence baked in. We are NOT. This project is a pure Rust logic crate: plain data structs + free functions, no OOP, no inheritance, no persistence concerns, no host dependencies.
3. All design decisions follow OUR rules ([`CLAUDE.md`](../CLAUDE.md), [`README.md`](../README.md)): tick-based, injected RNG, no I/O, events returned not dispatched, serializable plain data. If OpenMU's structure conflicts with our rules, our rules win — always.
4. When mapping an entity: list the domain facts from the reference (e.g. "an item has: level 0–15, durability, luck flag, skill flag, excellent options, ancient set, sockets"), then design our own struct from scratch per our conventions.
5. Ignore anything in OpenMU related to: persistence, networking, plugins, views, EF navigation properties, virtual/ICollection patterns, GUIDs-as-identity.

## The data-model trap — a reference's data model IS its architecture

*Corrective lesson from the v1→v2 data purge. Worked example: the OpenMU-purge audit and the per-domain kill maps in the v2 schema spec, [`docs/specs/2026-07-03-data-schemas-v2.md`](specs/2026-07-03-data-schemas-v2.md); the flagged-value backlog it produced is [`docs/debt/openmu-default-values.md`](debt/openmu-default-values.md).*

Rule 1 was obeyed for **behavior** and violated for **data shapes**. The WHAT/HOW line does not stop at methods — a class's **field layout is HOW**, exactly like its algorithms are. Extracting "what fields exist" from `ItemOptionDefinition`, `DropItemGroup`, or `AttributeRelationship` and renaming them into Rust structs is not extraction; it is transcribing OpenMU's persistence-and-configuration model one identifier at a time. OpenMU's DataModel is normalized for EF/GUID persistence and generic runtime configuration — that normalization is architecture, and it is precisely what must not cross. The domain fact is the **game concept** — an item's damage, a monster's resistances, a chaos recipe, an ancient set — never the reference's decomposition of it.

**Litmus:** if one of our data types shares a name, a field set, or a shape with a reference class, that is the leak — even when every field holds a real game value. A GUID-keyed reusable group, a generic option-definition record, an operator/scaled-by/aggregate vocabulary all describe how OpenMU *stores and evaluates* facts, not the facts. And the tell is always the same: the reference class is **generic** (one shape configured many ways) while the pre-S3 game concept is **specific** (a fixed, closed set of facts). A model whose every instance is one of three degenerate cases is the reference's model, not the game's.

The purge deleted and rebuilt four of these leaks from the game up:

- **Stat catalog + attribute-relationship evaluator.** v1 mirrored `StatAttributeDefinition` / `AttributeRelationship` (`Stats.cs`, `LevelDependentDamage.cs`) as `stats.json`, a `StatId` catalog, and an f64 operator-algebra evaluator (`Operator` / `ScaledBy` / `Aggregate` / `PowerUp`). Deleted whole. v2 has **no** stat catalog: every stat is a Rust type/field, every derived stat is a bespoke exhaustive per-class function with named integer coefficients, and the one resolved contribution is a single closed `CombatBonus` enum.
- **DropItemGroup model.** v1 mirrored `DropItemGroup` (`DropItemGroup.cs`) as `drop_groups.json` + `DropGroupId` — GUID-keyed reusable groups with f64 chances, attachable to monsters/maps, resolved by a guaranteed-first subtractive walk. All 33 records collapsed into three degenerate shapes. Deleted. v2 models the actual facts: a global per-kill roll policy, per-fact special-drop records keyed by the game's own identities, box contents keyed by the box item, formulas as pure functions.
- **Generic chaos ingredient-matcher.** v1 transcribed `SimpleCraftingSettings` / `ItemCraftingRequiredItem` (`ItemCrafting.cs`) field-for-field: `MixBehavior` / `MixInput` / `ItemMatch` / `MixItemAction` / `ResultChances` plus a behavior-dispatch column and recipe ids. Pre-S3 MU has no configurable mixer. Deleted. v2 is a kind-tagged `ChaosRecipe` enum, each family carrying only its own facts, recipe identity recovered by matching the placed ingredients.
- **ItemOptionDefinition machinery.** v1 mirrored `ItemOptionDefinition` / `ItemOption` / `IncreasableItemOption` / `ItemSetGroup` as `item_options.json` (22 records) and `item_sets.json` (81 records) behind a generic `OptionEntry` / `OptionType` / `LevelType` shape. Deleted. v2: the option vocabulary and its magnitudes are closed Rust enums plus named formulas in services; only the 36-record ancient-set roster and a small roll-policy config survive as data.

**Corollary to the workflow below:** step (a) lists domain facts as *game concepts in plain language* ("a chaos recipe turns N items of level L into item X at rate R"), never as a field-map of a reference class. If your fact list reads like the class's properties, you extracted HOW — throw it out and describe the game instead.

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
