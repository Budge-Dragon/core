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
| D1 | Flight state machine + movement service | deferred-scope | W-MOV | W-ENT (eligibility gate needs character entity state) | [movement-flight-wave.md](movement-flight-wave.md) | OPEN |
| D2 | Fixed-point narrowing surface (`mul`/`div`/`NonZeroFixed`/round-saturate) | deferred-scope | W-MOV | W-MOV (first consumer: normalize-to-speed) | [movement-flight-wave.md](movement-flight-wave.md) | OPEN |
| D3 | Walk-grid consumer (grounded-step validation) | deferred-scope | W-MOV | W-MOV (movement service) | [movement-flight-wave.md](movement-flight-wave.md) | OPEN |
| D5 | `Landing.facing` unspecified-arrival policy | deferred-scope | W-MOV | W-MOV (warp / movement service) | [movement-flight-wave.md](movement-flight-wave.md) | OPEN |
| T1 | `narrow_u8` provably-unreachable saturation arm | tech-debt | next `tile.rs` touch | none (opportunistic) | [spatial-foundation-followups.md](spatial-foundation-followups.md) | OPEN |
| T2 | `TileArea::contains` has no live consumer | tech-debt | W-MOV / next `tile.rs` touch | W-MOV (earn a consumer or trim) | [spatial-foundation-followups.md](spatial-foundation-followups.md) | OPEN |
| T3 | `WorldVec::length_sq` has no consumer | tech-debt | W-MOV | W-MOV (lands with normalize) | [spatial-foundation-followups.md](spatial-foundation-followups.md) | OPEN |
| Q1 | Serialized-shape drift-pin tests | quality-improvement | unscheduled | none (schedulable now) | [practices-transfer-quality.md](practices-transfer-quality.md) | OPEN |
| Q2 | Expand proptest invariants | quality-improvement | unscheduled | none (schedulable now) | [practices-transfer-quality.md](practices-transfer-quality.md) | OPEN |
| Q3 | `syn` drift scanner for review-enforced bans + pre-commit hook | quality-improvement | unscheduled | none (schedulable now) | [practices-transfer-quality.md](practices-transfer-quality.md) | OPEN |
| Q4 | CI OS matrix + wasm test-run | quality-improvement | unscheduled | none (schedulable now) | [practices-transfer-quality.md](practices-transfer-quality.md) | OPEN |
