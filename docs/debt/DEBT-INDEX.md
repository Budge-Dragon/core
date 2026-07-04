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
| EXC-DROP-WINDOW | Excellent drop draws the `-11` Normal window, missing the `>=25` gate and `-25` pool band | deferred-scope | W-SRC (excellent-drop pass) | authentic excellent-drop level rule + confirm delta 25 (shared with CMB-CONST) | [excellent-drop-level-window.md](excellent-drop-level-window.md) | OPEN |
| EFF-POISON | Poison DoT (3% of target max HP) is authentic but OP — ignores attacker power & defense, weak char can grind down a strong monster | deferred-scope | W-BALANCE (future) | product decision on a modern poison balance model | [poison-damage-balance.md](poison-damage-balance.md) | OPEN |
