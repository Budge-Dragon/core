# Debt Index

The single discoverable list of every open debt / deferral in mu-core. Each row
points to the formal record that carries its root cause, resolution plan, and an
explicit **Blocked-by**. A row leaves this table only when its record is closed.

Kinds: **deferred-scope** (root fix belongs to a named future wave, current code
is clean) · **tech-debt** (works today, guardian-flagged, fix when the area is
next touched) · **quality-improvement** (a practice/tooling gap, not debt from
shipped code — recorded so it is not lost).

| ID | Title | Kind | Owner wave | Blocked-by | Record | Status |
|---|---|---|---|---|---|---|
| W-SRC | OpenMU-invented default values in the v2 dataset | deferred-scope | W-SRC | authentic classic 0.75 / 0.95d source files | [openmu-default-values.md](openmu-default-values.md) | OPEN |
| CMB-CONST | Hardcoded/invented combat, exp, drop, durability & displacement constants, not sourced | deferred-scope | W-SRC | authentic classic combat/exp/drop/durability source + confirmation of invented skill magnitudes | [combat-constants-provenance.md](combat-constants-provenance.md) | OPEN |
| I1 | Two-handed weapon does not block the paired hand slot | deferred-scope | W-EFFECT | reconcile-with-defs reload pass + two-handed combat rule | [inventory-item-wave-followups.md](inventory-item-wave-followups.md) | OPEN |
| I2 | `EquipSlot` duplicates `data::game_config::EquipmentSlot` | deferred-scope | next `game_config.rs` touch (W-ENT) | `game_config.rs` frozen this wave | [inventory-item-wave-followups.md](inventory-item-wave-followups.md) | OPEN |
| I3 | Excellent rarity on a non-excellent kind silently degrades to Normal | deferred-scope | W-EFFECT | loot-side excellent-capability drop-pool filter | [inventory-item-wave-followups.md](inventory-item-wave-followups.md) | OPEN |
| EFF-POISON | Poison DoT (3% of target max HP) is authentic but OP — ignores attacker power & defense, weak char can grind down a strong monster | deferred-scope | W-BALANCE (future) | product decision on a modern poison balance model | [poison-damage-balance.md](poison-damage-balance.md) | OPEN |
