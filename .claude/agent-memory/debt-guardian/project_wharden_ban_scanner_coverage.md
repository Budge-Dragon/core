---
name: project-wharden-ban-scanner-coverage
description: W-HARDEN's xtask syn ban-scanner — what it mechanically covers vs. its ban-#1 (lookup-unwrap_or) blind spots, so future reviews know exactly where manual review is still the backstop.
metadata:
  type: project
---

W-HARDEN (branch `main`, uncommitted at review time; closes Q1–Q4 + T1) added
`xtask/` — a `syn::visit` scanner (`cargo xtask scan`, run by `.githooks/pre-commit`
and CI) that mechanizes the four review-only Iron-Law-3 bans no clippy lint covers.
See [[project-spatial-foundation-wave-boundaries]] and
[[project-wcmb-combat-wave-boundaries]].

**Fully/airtight mechanically covered (syntactically unambiguous):**
- **Inline `#[expect(..)]`** attribute.
- **`#[non_exhaustive]` on `enum`**.
- **Fabricated `Default`** — `Default::default()` / `T::default()` /
  `<T as _>::default()` / `..Default::default()` / `#[derive(Default)]`.
  `is_qualified_default` correctly spares a bare free fn named `default`.

**Ban #1 (`unwrap_or`/`unwrap_or_default` on a lookup) is only PARTIAL — the
scanner's weak spot:**
- Fires ONLY when the lookup call is the *immediate* receiver of `unwrap_or`,
  and only for method names in `{get, first, last, find}`.
- **Evades (false negatives):** a pass-through adapter between the lookup and
  `unwrap_or` — `v.get(k).copied().unwrap_or(d)`, `.cloned().unwrap_or(d)`,
  `.map(f).unwrap_or(d)`. Also `unwrap_or_else` (not in the ban list).
- **Blind by design:** domain-accessor lookups whose name isn't the std four —
  `atlas.monster(n).unwrap_or(..)`, `atlas.walk_grid(n).unwrap_or(..)`. A
  name-based syntactic scanner can't resolve which methods return `Option`; this
  is an inherent limit, so manual review still owns that shape.
- **Zero false positives confirmed:** the 4 `try_from(..).unwrap_or(TYPE::MAX)`
  narrows (`spatial.rs`, `profile.rs`, `loot.rs`, `ratio.rs`) are NOT flagged —
  receiver method `try_from` ∉ the lookup set. These are boundary narrows, not
  lookups (also noted in [[project-wcmb-combat-wave-boundaries]]).

**Review finding raised (required):** peel pass-through adapters
(`copied`/`cloned`/`map`/`as_ref`/`as_deref`) before the lookup-name check to
close the wrapped-lookup surface, and correct the Q3 closure in
`docs/debt/practices-transfer-quality.md` — it claims parity with clippy
enforcement, an overclaim for ban #1. **Do NOT trust the scanner as sole proof
for ban #1** when auditing future lookups.

**How to apply:** on any future `tile.rs`/services touch, still hand-check
`unwrap_or`/`unwrap_or_default` on domain accessors and adapter-wrapped std
lookups — the scanner won't catch them. The other three bans you can trust to CI.
