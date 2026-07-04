---
name: project-winv-inventory-wave-boundaries
description: What W-INV (inventory, item instances, drop-time roll, containers) legitimately defers vs. what got tracked as debt (I1-I3) — so future reviews treat the deferrals as planned, not fresh violations.
metadata:
  type: project
---

W-INV built item instances (`components/item_instance.rs`), the drop-time
option-roll (`services/item_roll.rs`), `WorldItem` (`entities/world_item.rs`),
`Inventory` + `Equipment` containers with their services, and the inventory
events. Scope boundary: `services/loot.rs` and `data/game_config.rs` were
FROZEN/off-limits (loot is W-EFFECT's; game_config is a data file). See also
[[project-spatial-foundation-wave-boundaries]], [[project-wcmb-combat-wave-boundaries]].

**Audited 2026-07-04 — verdict DEBT-FOUND (tracked, not blocking). Three shipped-code
deferrals tracked as I1-I3 in `docs/debt/inventory-item-wave-followups.md` + DEBT-INDEX;
do NOT re-flag as fresh debt:**
- **I1** two-handed weapon does not block the paired hand (`equipment.rs` per-slot
  model has no cross-slot invariant; `services/inventory.rs:194` `slot_accepts`
  doc-flags it). Deferred because the reload half is a cross-reference check
  (instance carries only `ItemRef`, not kind) needing a reconcile-with-defs pass —
  same mechanism as `ItemInstance::reconcile`. Owner **W-EFFECT**. Equip-time half
  alone would ship a half-enforced invariant (worse) — defer the whole unit.
- **I2** `EquipSlot` (components) twins `data::game_config::EquipmentSlot` + a
  12-arm `translate_slot`. The `ItemRef`/`MapNumber` relocate-and-re-export is the
  endpoint, foreclosed because `game_config.rs` was frozen. Compiler-guarded vs
  divergence (exhaustive match) so low impact. Owner **next game_config.rs touch (W-ENT)**.
- **I3** (my own finding, NOT in the review brief's list) excellent rarity on a
  non-excellent kind silently degrades to Normal (`item_roll.rs:106-113`
  `None => RarityRoll::Normal`). The design's "not producible" premise (§E.2) is
  **factually wrong**: `loot::item_drop` (loot.rs:96-119) stamps the category-rolled
  rarity onto ANY picked pool item with no excellent-capability filter, so it IS
  reachable. Root fix = filter the loot excellent pool. Owner **W-EFFECT**.

**Clean deferrals — ruled NO debt row (feature-not-built, already owned by a named
wave in WORKPLAN, W-INV shipped nothing compromised):**
- Character wiring (Inventory/Equipment not yet fields on `Character`) — W-ENT owns
  the character entity + inventory/zen (WORKPLAN). A trivial future field-add.
- CombatBonus→profile aggregation — W-ENT/W-CMB own it (WORKPLAN + W-CMB scope memory);
  needs the containers W-INV just built.
**Rule applied:** a row is for a *shipped-code* gap/smell/representable-illegal-state
(I1/I2/I3 all live in committed W-INV code). A feature never in scope with a named
roadmap owner is NOT accepted-debt — do not flood the index with not-yet-built features.

**Routed to existing backlog, NOT a new W-INV row:** `item_rules::max_durability`'s
unsourced excellent(+15)/ancient(+20)/per-level durability bonuses are PRIOR-WAVE
math W-INV only consumes — appended to **CMB-CONST** (`combat-constants-provenance.md`,
owner W-SRC), the correct provenance home.

**Confirmed clean:** zero forbidden phrases / TODO / HACK / FIXME in all 9 new files.
The two `// W-SRC:` comments (50% skill = facts 5:44; jewelry-no-luck = facts 2:32) are
sourced — no row. `slot_bit`'s `None` arm (`item_instance.rs:182`) is a total const-fn
fallback for the unreachable zero input (same species as the closed T1 dead-arm) — ruled
acceptable-final-shape, NOT tracked: `NonZeroU8::new` is inherently Option-typed and the
root fix would reshape prior-wave `slot_index()` to a bounded type (out of W-INV scope).
