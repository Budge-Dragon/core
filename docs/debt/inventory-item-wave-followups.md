# Debt record: inventory & item-instance wave follow-ups (I1–I3)

- **ID:** I1–I3 (W-INV deferred-scope)
- **Status:** CLOSED (all three discharged 2026-07-04)
- **Owner wave:** per sub-item (W-EFFECT / next `game_config.rs` touch)
- **Created:** 2026-07-04, during the inventory & item-instance work (W-INV).
- **Resolved:** 2026-07-04. All three root fixes landed on `main`; five gates green
  (fmt, clippy `-D warnings`, 414 tests, wasm target, xtask scan); architecture,
  deep-module, and canon guardians PASSED. `DEBT-INDEX.md` rows removed. Close is
  pending the orchestrator's commit (guardian does not run git). Verified against the
  code by the debt-guardian before closing — see per-item resolutions below.
- **Category:** code (architectural / correctness)
- **Severity:** medium
- **Scope:** three items in the *shipped* W-INV code where the committed code is
  complete-and-honest for this wave's boundary but a root-level fix is
  deliberately deferred to a named future wave. Each compiles clean, passes CI,
  and uses no banned suppressor. They are root-cause-tracked here so the deferred
  fix surfaces the moment its owning wave runs and is not lost to a scratchpad
  design doc.

## Why this is deferred-scope, not a violation

Every item below is either a domain rule whose enforcement genuinely crosses an
off-limits service, or a duplication whose clean endpoint is foreclosed by a
frozen file this wave. The current code does not fake, stub, or paper over any
of them — it handles each totally and honestly at W-INV's own boundary. What is
missing is the *root* fix, which lives outside W-INV's writable surface. Tracked
here per the cardinal rule: a known gap that is neither fixed nor formally
recorded is the violation, not the gap itself.

## Sub-items

### I1 — two-handed weapon does not block the paired hand slot

- **Status:** RESOLVED (2026-07-04). Both demanded halves landed together — the
  half-enforced trap the record warned against was avoided. Equip-time:
  `services/inventory.rs::equip` calls `two_handed_conflict(&equipment,
  hand_occupation(def_kind), slot, atlas)`, which rejects both directions (a 2H
  weapon when the paired hand is occupied, and any item joining a hand paired with
  a worn 2H weapon) via `EquipRejection::TwoHandedConflict`; `hand_occupation` reads
  `WeaponHandling::TwoHanded` structurally (bows/crossbows 2H by construction).
  Reload-time: `reconcile_equipment(&Equipment, &Atlas)` performs the
  instance×definition cross-reference the `Equipment` component cannot hold alone
  and rejects a 2H+offhand set with `EquipmentConflict::TwoHandedWithOffhand`. The
  `slot_accepts` deferral note is gone — its doc now points at both enforcement
  sites. Root eliminated: cross-slot occupancy is now a real invariant, blocked at
  equip and re-proven at reload. **Commit pending** (orchestrator owns git).
- **Location:** `core/src/components/equipment.rs` (independent per-slot
  `Option<ItemInstance>` model, no cross-slot invariant);
  `core/src/services/inventory.rs:194` (`slot_accepts` — doc-flagged) and
  `equip` (no paired-hand check).
- **Symptom:** a `WeaponHandling::TwoHanded` weapon equipped into one hand does
  not block the other hand, so the state "2H weapon + off-hand item worn
  simultaneously" is representable. This is the one known illegal state left
  representable (state-machine design §F).
- **Root cause:** two-handed occupancy is a **cross-slot invariant**, and its
  reload-side enforcement is a **cross-reference** check (instance × definition):
  `ItemInstance` carries only `ItemRef`, not its kind, so `Equipment` — which may
  not import `data` — cannot tell at parse time whether a worn item is two-handed.
  The equip-time half (reject equipping a 2H weapon when the off-hand is occupied,
  and reject filling an off-hand while a 2H weapon is worn) is containable in the
  equip service, but shipping only that half yields a **half-enforced invariant**
  (blocked at equip, still representable at reload) — a worse trap than a coherent
  deferral. The honest unit is both halves together.
- **Resolution plan:** when two-handed weapon combat is modeled, (a) add the
  equip-service paired-hand block keyed on `WeaponHandling::TwoHanded` (data
  already carries it), and (b) add a reconcile-with-definitions reload pass — the
  same cross-reference mechanism `ItemInstance::reconcile(category)` already
  models for excellent-set category — that re-proves "no 2H weapon paired with an
  off-hand item" once the defs are in hand. Delete the `slot_accepts` deferral
  note at that point.
- **Owner wave:** W-EFFECT (item-effect / equip-rules wave). **Blocked-by:** the
  reconcile-with-defs reload pass + the two-handed combat rule, both outside
  W-INV's writable surface.

### I2 — `EquipSlot` duplicates `data::game_config::EquipmentSlot`

- **Status:** RESOLVED (2026-07-04). The `ItemRef` precedent was applied: the single
  `EquipmentSlot` (12 variants) now lives in `components/equipment.rs`, and
  `data/game_config.rs:7` re-exports it verbatim via
  `pub use crate::components::equipment::EquipmentSlot;` (frozen public API,
  byte-identical wire). The `EquipSlot` twin and the 12-arm `translate_slot` bridge
  are both deleted (`grep` confirms zero remaining references to either). Root
  eliminated: one slot vocabulary, no cross-layer twin, no hand-written bridge to
  drift. **Commit pending** (orchestrator owns git).
- **Location:** `core/src/components/equipment.rs:19` (`EquipSlot`, 12 variants)
  vs `core/src/data/game_config.rs:91` (`EquipmentSlot`, identical 12 variants);
  `core/src/services/inventory.rs:173` (`translate_slot`, the 12-arm bridge).
- **Symptom:** two byte-identical 12-variant slot enums plus a hand-written
  12-arm translation function that exists solely to bridge them.
- **Root cause:** the dependency rule forbids a `components` type from importing
  `data`, so `Equipment` (in `components`) cannot name `data`'s slot vocabulary
  and a twin is minted. The clean endpoint is the **`ItemRef` precedent applied
  this same wave**: relocate the slot vocabulary into `components` and re-export
  it from `game_config` (`pub use`), collapsing the twin and deleting
  `translate_slot`. Unlike `ItemRef` — whose interim duplication was *resolved*
  within W-INV — this relocation was foreclosed because `game_config.rs` is a
  frozen file this wave.
- **Note:** the duplication is compiler-guarded against silent divergence — the
  exhaustive `translate_slot` match breaks the build if either enum gains or
  loses a variant — so the impact is a maintenance/clarity cost, not a
  correctness hazard.
- **Resolution plan:** relocate the slot enum into `components` (its own module or
  alongside `EquipSlot`), replace `data::game_config::EquipmentSlot` with a
  `pub use` re-export (frozen public API, byte-identical wire), and delete
  `translate_slot` from the equip service. Mirrors the `ItemRef` / `MapNumber`
  relocations.
- **Owner wave:** next `game_config.rs` touch (candidate: W-ENT). **Blocked-by:**
  `game_config.rs` frozen under W-INV's scope boundary.

### I3 — excellent rarity on a non-excellent-capable kind silently degrades to Normal

- **Status:** RESOLVED (2026-07-04). The two services now share the excellent-capability
  predicate. `item_roll.rs` exposes `pub(crate) fn is_excellent_capable(kind)` (the
  `excellent_category(kind).is_some()` capability, without leaking the private
  `ExcellentCat`), and `loot.rs::item_drop` filters the drop pool on it for
  `ItemRarity::Excellent` (`.filter(|def| match rarity { Excellent =>
  is_excellent_capable(&def.kind), Normal | Ancient => true })`). An `Excellent`
  category roll can therefore no longer land on ammo / a consumable / pre-S3 wings /
  a pet. The roll service's `None => RarityRoll::Normal` arm is now provably dead for
  authentic drops; its comment is updated to say so and it remains only to keep the
  roll total over its bare `ItemRarity` input (no panic). Root eliminated: the design
  premise that "an excellent-rarity drop on a no-excellent kind is not producible" is
  now true by construction of the pool. **Commit pending** (orchestrator owns git).
- **Location:** `core/src/services/item_roll.rs:106-113`
  (`roll_dropped_item`, the `excellent_category(kind)` `None => RarityRoll::Normal`
  arm); reachable via `core/src/services/loot.rs:96-119` (`item_drop`).
- **Symptom:** when `roll_dropped_item` receives `ItemRarity::Excellent` for a
  kind with no excellent set, it produces a `Normal` instance — a silent quality
  downgrade (the drop's decided Excellent rarity is discarded).
- **Root cause:** the rarity decision (`loot::item_drop`) is decoupled from
  excellent-capability (`item_roll::excellent_category`). `item_drop` stamps the
  category-rolled `rarity` onto **any** definition picked from the general drop
  pool, with no excellent-capability filter, so an `Excellent` category roll can
  land on ammo / a consumable / pre-S3 wings / a pet. The roll service — total
  over its bare `ItemRarity` input — cannot panic and cannot fabricate an
  excellent set, so degrading to `Normal` is its only honest handling. The design
  premise (§E.2: "an excellent-rarity drop on a kind with no excellent set is not
  producible") is **factually incorrect** — `loot::item_drop` does produce it.
- **Resolution plan:** in the loot service (off-limits this wave), filter the
  excellent drop pool to excellent-capable kinds — or otherwise constrain
  `Drop::Item` so `Excellent` on a non-excellent kind is unrepresentable — making
  the roll service's `None` arm provably dead so it can be removed. The
  excellent-capability predicate already exists as
  `item_roll::excellent_category`; the two services must share it or the loot pool
  must gate on it.
- **Owner wave:** W-EFFECT (loot wave — `services/loot.rs` is off-limits under
  W-INV's scope boundary). **Blocked-by:** the loot-side excellent-pool filter /
  rarity-capability contract.

## Discharge

Each sub-item is discharged when its root fix lands (the paired-hand invariant for
I1, the slot-vocabulary relocation for I2, the loot-side excellent-capability
filter for I3) and its `DEBT-INDEX.md` row is removed. The record is closed when
all three are discharged.

**Discharged 2026-07-04.** All three root fixes landed on `main` and were verified
against the code by the debt-guardian; the I1/I2/I3 rows are removed from
`DEBT-INDEX.md`. This record is CLOSED. One authenticity gap in the loot code the
I3 fix touched — the excellent-drop level window — was surfaced separately during
review; it is **pre-existing W-CMB loot behaviour, not introduced by I1–I3**, and is
tracked on its own row (`EXC-DROP-WINDOW`, see
[excellent-drop-level-window.md](excellent-drop-level-window.md)), not folded here.
