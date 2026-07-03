# Debt record: practices-transfer quality improvements (Q1–Q4)

- **ID:** Q1–Q4 (quality improvements)
- **Status:** CLOSED (2026-07-04, W-HARDEN — all four items landed)
- **Owner wave:** W-HARDEN (cross-cutting tooling/CI wave)
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
  wired into a pre-commit hook (and CI), giving the *std forms* of the
  review-enforced bans a build-failing mechanical pre-flight. This is not full
  parity for ban #1 — a name-based scanner cannot see domain-accessor lookups,
  which stay review-authoritative (see the closure note for the exact boundary).

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

**ALL FOUR CLOSED (2026-07-04, W-HARDEN).** Q1–Q4 rows removed from
`DEBT-INDEX.md`; this record is closed.

- **Q1 — CLOSED.** `core/tests/wire_drift.rs` pins one canonical
  `assert_eq!(serde_json::to_string(&value).unwrap(), "…")` per host-facing wire
  type (the `spatial` value types + `Region` circle/rect/cone; the vocabulary
  newtypes/enums `CharacterClass`/`ItemRarity`/`EnhanceLevel`/`AmmoLevel`/
  `Level`/`Zen`/`Exp`/`Interval`/`TileCoord`; `SpecialDrop`; and the W-ENT/W-MOV/
  W-CMB event enums — `AttackOutcome`, `DamageModifiers`, `SkillOutcome`,
  `CastRejection`, `Drop`, `DropResolution`, `ExpAward`, `LevelUp`,
  `KillResolution`, `FlightOutcome`, `StepOutcome`, `WarpOutcome`,
  `MonsterIntent`, `SpawnEvent`). Each string is the real serialization, so a
  rename/reorder/tag change reds exactly that pin.
- **Q2 — CLOSED.** `proptest!` blocks added to the existing test modules of
  `services/chance.rs` (weight-total = exact sum; every pick lands in a real
  bucket; heavier bucket dominates over a large sample; `pick_one` always
  returns a list element + index coverage) and `rng/mod.rs` (a new test module:
  `uniform_below`/`uniform_below_usize` output `< bound`; equidistribution within
  a deterministic band), plus `components/units.rs` (`Level`/`Zen`/`Exp` wire
  round-trips across their valid ranges + out-of-range rejection).
- **Q3 — CLOSED.** New `xtask/` workspace member (`syn` visitor) flags the four
  review-enforced bans in `core/src/**/*.rs` with `file:line` and a non-zero
  exit; `cargo xtask scan` alias (`.cargo/config.toml`); `.githooks/pre-commit`
  runs it; README documents the one-time `git config core.hooksPath .githooks`.
  `xtask` omits `[lints] workspace = true` and never enters mu-core's dependency
  graph. Verified: clean tree exits 0; each of the four injected patterns exits
  non-zero with `file:line`.

  **Scope of ban #1 (honest boundary — NOT full parity).** The scanner is a
  *name-based* mechanical pre-flight, not the equal of a clippy-covered ban. For
  ban #1 it catches the **std lookup-name chains** — `get`/`first`/`last`/`find`
  — including when they hide behind a peeled set of Option-preserving adaptors
  (`copied`/`cloned`/`map`/`as_ref`/`as_deref`/`and_then`/`filter`/`or`/`or_else`/
  `inspect`), so the idiomatic `.get(&k).copied().unwrap_or(..)` and
  `.first().cloned().unwrap_or_default()` are flagged. It **fundamentally cannot**
  catch a **domain-accessor lookup** — `atlas.monster(n).unwrap_or(..)`,
  `atlas.walk_grid(n).unwrap_or(..)` — because those method names are
  indistinguishable from any other call to a name-based AST scanner; ban #1's
  domain-accessor forms therefore **remain review-authoritative**. Bans #2–#4
  (`#[expect]`, `#[non_exhaustive]`, fabricated `Default`) are name/shape-exact,
  so the scanner is authoritative for those. `#[cfg(test)]` modules are skipped,
  matching clippy's test exemption. Net: the scanner is a build-failing
  pre-flight for the std forms of ban #1, not a replacement for review of it.
- **Q4 — CLOSED.** `.github/workflows/ci.yml` rewritten to a
  `ubuntu`/`macOS`/`windows` matrix for fmt/clippy/test + `cargo xtask scan`; the
  existing four portability compile-checks kept as a compile gate; a dedicated
  leg RUNS `cargo test -p mu-core --test wasm_determinism --target wasm32-wasip1`
  under wasmtime. Fallback taken per the plan: the full `cargo test --target
  wasm32-wasip1` is not wasi-buildable (`proptest` → `wait-timeout`), so
  `proptest` is gated out of the wasm dev-deps and a proptest-free determinism
  test (`core/tests/wasm_determinism.rs`, fixed seed → fixed hardcoded output)
  is the executed leg. Validated locally under wasmtime 46: native and wasm
  produce bit-identical outputs.
