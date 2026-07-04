---
name: inv-wave-design-findings
description: W-INV (inventory/item-instances) spec findings — illegal-state encoding of rolled options, roll-service RNG-order contract, container port shape, policy-injection test strategy
metadata:
  type: project
---

# W-INV design findings (spec phase)

Spec for the Inventory & Item Instances wave: `ItemInstance`, the drop-time
option-roll service, `WorldItem`, `Inventory` grid + `Equipment` slots, and their
events. Composes already-built `components/item_options.rs`, `levels.rs`,
`item_quality.rs`, `units.rs`, `data/option_roll.rs`, `services/item_rules.rs`,
`events/loot.rs::Drop::Item`. Sibling of [[effect-wave-design-findings]] and
[[combat-wave-design-findings]].

**Why:** first agent in the Core Domain Feature pipeline; these are the design
tensions downstream agents (architecture/canon/state-machine) must settle.

**How to apply:** reuse when W-INV re-opens or when a later wave (W-ENT merge,
W-CRAFT, W-SHOP, stat aggregation) touches item instances.

## Load-bearing findings
- **Rarity payload is the discriminator that makes illegal states unrepresentable.**
  Excellent options nest ONLY in an `Excellent` variant; `AncientBonusLevel` nests
  ONLY in an `Ancient` variant. Excellent set = armor|weapon{damage} mirroring
  `ExcellentCategory`, so armor-option-in-weapon-set is a compile error. The
  set-matches-*item* cross-check needs the definition's category — recommend
  passing category as a construction-time proof input (not a stored denormalized
  field), yielding a parse-boundary `ExcellentSetCategoryMismatch` failure for
  reloaded instances.
- **Normal option / luck / +Skill are ORTHOGONAL to rarity** (OpenMU
  `AddRandomOptions` runs on every drop; excellent handled separately). An
  excellent weapon can also carry a normal +dmg option, luck, and skill.
- **Roll service is TOTAL over every `ItemKind`** (jewels/consumables/orbs roll
  nothing → zero RNG words). Rarity + level are INPUTS from `Drop::Item`, never
  re-rolled (loot.rs already decided them).
- **Determinism = a fixed RNG draw-order contract.** Recommended order: normal
  option (+level) → luck → skill → rarity payload. Non-eligible/guaranteed
  branches must consume zero words. Ancient is reachable only via direct
  roll-service input pre-S3 (no ancient set data shipped).
- **Test probabilistic mechanics by INJECTING policy extremes** (`ChancePer10000::
  ALWAYS`/`NEVER`), never by sampling the review-flagged 0.25/0.001 defaults —
  `OptionRollPolicy` is entirely OpenMU-default pending sources (per option_roll.rs).
- **Containers draw NO RNG** — pure `(container, intent) -> (container, outcome)`;
  rejection (occupied/out-of-bounds/wrong-slot/empty) is a real domain outcome,
  container returned unchanged. Footprint (WxH) must enter as intent so the
  container never reaches into `Atlas`/definitions (layer leak).
- **`WorldItem` pairs `WorldPos` + `MapNumber` directly** (no `Placement` — a
  ground item has no facing/movement), honoring "position never travels without
  its map."

## Open tensions handed downstream
- luck/skill as bool fields vs enum variants (Iron Law "no bool flags").
- durability: reuse `Pool` (u32) vs dedicated u8 `Durability` (proves 255 cap).
- durability for box-tier levels (12..15, `enhance_level()==None`) and stackables
  (jewels/potions where "durability" = stack count) — genuine per-kind fact.
- grid coord/footprint types (new `Cell`/`Footprint`) and where per-kind
  eligibility (normal option, luck, equip-slot) lives.
- roll service returns bare `ItemInstance` vs also emitting an event.
- two-handed weapon dual-hand occupancy (cross-slot invariant) — likely deferred.
