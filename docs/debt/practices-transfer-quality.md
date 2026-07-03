# Debt record: practices-transfer quality improvements (Q1–Q4)

- **ID:** Q1–Q4 (quality improvements)
- **Status:** OPEN
- **Owner wave:** unscheduled (cross-cutting; schedulable now, no blocker)
- **Created:** 2026-07-03, from the practices-transfer audit run alongside the
  spatial-foundation work (Waves A + B, branch `spatial-foundation`).
- **Scope:** four testing / tooling gaps surfaced by the practices-transfer
  audit. These are **not debt from shipped code** — no shipped line is wrong;
  they are guardrails the codebase does not yet have. Recorded distinctly so the
  audit's findings are not lost, and kept out of the deferred-scope / tech-debt
  buckets so future reviews do not mistake them for violations in committed code.

## Why these are quality improvements, not debt

Every item below strengthens the *proof* that the Iron Laws hold — wire
stability, invariant coverage, ban enforcement, cross-target determinism —
rather than fixing a defect. None is blocked by a future wave; each is
schedulable as soon as it is prioritized. They are logged here so "we should add
that test / that scanner / that CI leg" survives past the audit conversation.

## Items

### Q1 — Serialized-shape drift-pin tests

- **Gap:** host-facing wire stability (the internally-tagged `kind` enums and
  newtype wire forms that non-Rust clients read) has per-type round-trip tests
  but no explicit **drift-pin** — a test that fails loudly if a serialized shape
  changes, so a rename or field reorder cannot silently break the wire contract.
- **Plan:** add drift-pin tests asserting the exact serialized JSON of a
  representative value per host-facing type (extending the existing
  `serialize_identity_is_stable` style), so any wire-shape change is a red test,
  not a silent break.

### Q2 — Expand proptest invariants

- **Gap:** proptest covers the spatial predicates well
  (`within_range ⇔ distance_sq ≤ r²`, distance symmetry, vector round-trips,
  cone magnitude-invariance) but key non-spatial invariants are unit-tested
  only.
- **Plan:** add property tests for `chance.rs` `WeightedTable` (selection
  distribution / total-weight invariants), the RNG seam's `uniform_below`
  (no modulo bias), and newtype wire round-trips across their full valid ranges.

### Q3 — `syn` drift scanner for the review-enforced bans + pre-commit hook

- **Gap:** several bans are **review-enforced only** — no clippy lint covers
  `unwrap_or` / `unwrap_or_default` on lookup-shaped expressions, inline
  `#[expect(...)]`, `#[non_exhaustive]` on domain enums, or `Default`/zeroed
  values fabricated to satisfy a signature. These rely entirely on a human
  catching them.
- **Plan:** a `syn`-based AST scanner that flags these patterns mechanically,
  wired into a pre-commit hook (and CI), so the review-enforced bans get the
  same build-failing enforcement the clippy-covered ones already have.

### Q4 — CI OS matrix + wasm test-run

- **Gap:** CI runs a single OS and compiles wasm without running it. Determinism
  ("same inputs + same seed = same outputs on native, wasm, and FFI") is a core
  guarantee but is not actually *exercised* across targets — only asserted.
- **Plan:** expand CI to a linux / macOS / windows matrix and add a wasm
  **test-run** leg (not just `cargo check`), so cross-platform bit-for-bit
  determinism is proven by execution, not assumed.

## Discharge

Each item is closed independently when its guardrail lands (test, scanner+hook,
or CI leg) and is removed from `DEBT-INDEX.md`; close this record when all four
are in place. Because none is blocked, any of these can be pulled forward into a
wave that is already touching the relevant area (e.g. Q1/Q2 alongside any type
change, Q3/Q4 as standalone tooling work).
