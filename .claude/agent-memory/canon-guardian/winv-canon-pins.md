---
name: winv-canon-pins
description: W-INV plan-review canon rulings for item instances, option-roll, inventory grid, durability — reuse when a later wave re-touches item instances
metadata:
  type: project
---

# W-INV canon pins (inventory & item instances, plan phase)

Plan-review rulings for the Inventory & Item Instances wave. Verdict was
APPROVE-WITH-REQUIRED-CHANGES. Sibling of [[wcmb-canon-pins]], [[weffect-canon-pins]].

**Why:** these settle canon tensions that W-CRAFT, Jewel-of-Life leveling, stat
aggregation, and the Character-merge wave will re-open when they touch item instances.

**How to apply:** treat as decided precedent when reviewing any later item-instance work.

## Required-change rulings (canon)
- **Excellent option set = non-empty DISTINCT set, NOT `OneOrMore<T>`.** OneOrMore
  permits duplicate slots — illegal per facts doc 2:75 ("no duplicate options").
  Canonical form is the authentic non-zero 6-bit client bitmask keyed by the existing
  `ExcellentArmorOption/ExcellentWeaponOption::slot_index()` (1..=6). Mask!=0 proves
  first-excellent-guaranteed; bits forbid duplicates. Order is not a domain fact.
- **Luck is SOURCED-ABSENT on jewelry** (facts 2:32: jewelry PossibleItemOptions =
  health-recover + excellent, NO Luck). Roll service must gate luck by a per-kind
  total `grants_luck(ItemKind)` (weapons/armor/shields/wings yes; ring/pendant/pet/
  consumable/jewel/orb/scroll no), else jewelry rolls luck (inauthentic) AND
  non-eligible kinds consume RNG words (breaks zero-words determinism). Answer to
  open-q #6: don't roll it; no `// W-SRC:` needed (source affirmatively absent).
- **Don't store definition-derivable facts on the instance.** `damage:WeaponDamageKind`
  on an excellent Weapon set is denormalized — resolvable from ItemKind (weapon/staff)
  or definition.excellent (pendant). Project precedent: staff_rise is computed not
  stored (item_definitions.rs:126). Keep only the armor/weapon discriminator.
- **Inventory + Durability need validating deserialize** (wire mirror + `try_from`
  re-proving non-overlap/in-bounds and current<=max), mirroring PoolWire/WorldPos.
  Deserialize IS the parse boundary; "preserved by ops only" leaves reload unguarded.

## Settled design questions
- #3 Durability: dedicated u8 `Durability{current<=max}`, NOT `Pool` (u32 can't prove
  the 255 wire cap — Iron Law 3). Justified specialization, not classitis. Per-kind
  meaning (wear/stack/ammo) stays as MAX-computation in the roll service, not in the
  type (all three are the same gauge with identical ops).
- #4 Luck/Skill: two-variant enums correct. bool banned (IL3); list-membership
  re-skins OpenMU ItemOptionLink (anti-laundering target). Both facts are binary.
- #5 `assemble` stays fallible taking ExcellentCategory proof input — reload path is
  untrusted; infallible would leave set-matches-category unguarded.
- #7 Grid: `Vec<PlacedItem{anchor,footprint,item}>` is canonical for 8x8 serializable
  grid; occupancy bitmap would DUPLICATE state (list+index desync) — rejected.
- #1 component-local Cell/Footprint + service-boundary GridSize translation approved;
  eventual endpoint is relocating grid geometry into components/ (like W-MOV MapNumber),
  foreclosed this wave (game_config.rs off-limits).
- #2 component-owned placement math is the deeper module (encapsulates the non-overlap
  invariant like Pool owns its arithmetic); dumb-data + service-overlap leaks grid guts.

## Code-phase pin
- Excellent extra-option draw must be distinct-sampling from REMAINING options
  (bounded partial-shuffle), never rejection-resample-until-distinct (variable RNG
  words breaks the fixed draw-order determinism contract).

## Code-phase verdict (implementation) — PASS, clean
- Implementation matched every plan pin. `take_one` = swap_remove Fisher-Yates on a
  shrinking pool (not resample); draw order first-guaranteed→extra-roll→remaining-pool
  matches §E.2. Shipped `game_config.json` option_roll caps are AUTHENTIC:
  `max_excellent_options_per_drop=2` (=OpenMU MaximumOptionsPerItem, facts 2:70/75),
  `max_dropped_option_level=3` (facts 2:64/5:44). Tests inject 3/L4 extremes only.
  Excellent slot_index 1..6 arrays match facts 2:71-72 exactly.
- Reuse boundary for later waves: `services/item_roll::roll_durability` calls
  `item_rules::max_durability(base, enhance, rarity)` — that fn adds an
  EXCELLENT/ANCIENT durability bonus. Facts 5:45/46 say only "durability = max of one
  piece" (no rarity bonus stated). This is PRIOR-WAVE (item_rules) math, consumed not
  invented by W-INV; canon "full at drop" (current==max) holds. Route the rarity-bonus
  authenticity to source-guardian if it re-opens; NOT a W-INV defect.

## I1/I2/I3 debt close-out (code phase) — PASS
- **Two-handed `hand_occupation` (services/inventory.rs) authentic.** TwoHands =
  `Weapon{TwoHanded}` | Bow | Crossbow; everything else OneHand. Bow/Crossbow carry
  NO `handling` field (item_definitions.rs:82-114) — two-handedness is structural
  (no shield with a bow), not a fabricated column. Melee `WeaponHandling` abstracts
  OpenMU's width rule (facts 1:50 `IsTwoHandedWeaponEquipped=1 for 2-wide non-bow`).
  Staff-as-one-handed is a SIMPLIFICATION honestly flagged via `// W-SRC:` (authentic
  staves are width-based; some 2-wide = two-handed) — no data field invented. OK.
- **Scope boundary (not a defect):** OneHand+OneHand permits any two one-handers;
  authentic dual-wield is class-gated (`DoubleWieldWeaponCount=1` only 1-wide melee
  groups 0-2, facts 1:50). Belongs to a future equip class-restriction wave, not I1.
- **`is_excellent_capable` gate (I3) canonical + authentic.** Gates the candidate pool
  at `loot::item_drop` BEFORE `OneOrMore` sampling (weighted-selection canon: constrain
  set before sampling), derived from the single `excellent_category` oracle (no dup kind
  list). Excellent-capable families = physical weapons + bow/crossbow + staff, armor +
  shield + ring, pendant — matches facts 2:32,71-73 exactly; wings/pets/jewels/ammo/
  consumables/orbs/scrolls/transformation-ring excluded (wings roll own option + Luck,
  not excellent). `None => RarityRoll::Normal` arm honestly documented unreachable +
  total, no stale I3 anchor. Threading an ExcellentCategory proof into the serde
  `Drop::Item` event would denormalize a derivable fact — re-derive is correct.
- **OPEN authenticity gap → route to source-guardian (pre-existing, NOT an I3 defect):**
  OpenMU excellent drop requires monsterLevel>=25 AND computes the pool at
  (monsterLevel-25) (facts 2:75,142; 5:46,168). mu-core `item_drop` draws excellent from
  the SAME `[level-11,level]` window as Normal — no -25 shift, no >=25 gate. So excellent
  drops still pull higher-base items than authentic. A future excellent-drop wave reopens.
- **`reconcile_equipment` mirrors `ItemInstance::reconcile`** (item_instance.rs:63):
  equip-time block + reload reconcile is the textbook pair for a cross-slot invariant
  that ALSO spans instance×definition (handedness lives in the def, resolved via Atlas),
  so it can't be a pure `Equipment` type invariant. Non-serde boundary error enum with
  Display+Error — justified parse-boundary error type (IL4 exception). Canonical.

## Verified-clean (strengths)
- Anti-laundering clean: no ItemOptionDefinition/ItemOptionLink/DropItemGroup/
  ItemSetGroup generic machinery; rolled facts are typed fields; ancient = per-piece
  AncientBonusLevel only (no set-membership; no ancient data pre-S3).
- RarityRoll sum + assemble proof-input is canonical make-illegal-states.
- Reuses roll_per_10000/uniform_below and item_rules::max_durability — no reinvention.
