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
| MOB-SPD | Invented monster per-step distance (1 tile), not sourced | deferred-scope | W-SRC | authentic classic monster per-step-distance source | [mob-step-speed-provenance.md](mob-step-speed-provenance.md) | OPEN |
| CMB-CONST | Hardcoded/invented combat, exp, drop & displacement constants, not sourced | deferred-scope | W-SRC | authentic classic combat/exp/drop source + confirmation of invented skill magnitudes | [combat-constants-provenance.md](combat-constants-provenance.md) | OPEN |
| T1 | `narrow_u8` provably-unreachable saturation arm | tech-debt | next `tile.rs` touch | none (opportunistic) | [spatial-foundation-followups.md](spatial-foundation-followups.md) | OPEN |
| Q1 | Serialized-shape drift-pin tests | quality-improvement | unscheduled | none (schedulable now) | [practices-transfer-quality.md](practices-transfer-quality.md) | OPEN |
| Q2 | Expand proptest invariants | quality-improvement | unscheduled | none (schedulable now) | [practices-transfer-quality.md](practices-transfer-quality.md) | OPEN |
| Q3 | `syn` drift scanner for review-enforced bans + pre-commit hook | quality-improvement | unscheduled | none (schedulable now) | [practices-transfer-quality.md](practices-transfer-quality.md) | OPEN |
| Q4 | CI OS matrix + wasm test-run | quality-improvement | unscheduled | none (schedulable now) | [practices-transfer-quality.md](practices-transfer-quality.md) | OPEN |
