---
name: mu-core-context
description: What mu-core is and the quality bar it is held to, for canon review
metadata:
  type: project
---

mu-core is the pure domain core of a MU Online rewrite (hexagonal; hosts depend on core, never reversed). Only deps: serde + rand_core. The bar is "top-industry-standard, canon-correct, deeply-thought structure."

**Why:** The user demands textbook-canonical modeling and rejects bespoke variants where a named pattern exists.

**How to apply:** Judge against the Four Iron Laws in [[review-standard]] scope plus Rust API Guidelines. Known-good canon already in the crate to hold others to: `components/interval.rs` shows the correct serde `bound(...)` technique (impl-level bounds, not struct-level); `services/chance.rs` `WeightedTable` is the canonical derived-total cumulative table; `rng/mod.rs` `uniform_below` is textbook Lemire; `data/atlas.rs` is the parse-don't-validate referential-integrity loader. As of the v2 data/entity rebuild, `entities/` and `events/` are intentionally empty (combat/drop/leveling logic is deliberately future work) — do not flag their emptiness as half-implementation.
