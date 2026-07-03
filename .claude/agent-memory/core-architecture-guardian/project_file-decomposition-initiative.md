---
name: file-decomposition-initiative
description: Ongoing effort to split over-long core source files into directory modules while preserving public API paths and layering
metadata:
  type: project
---

Active initiative: break up over-long `core/src` source files (first target `data/atlas.rs`, ~1216 lines) into directory modules (`data/atlas/{mod.rs, views.rs, resolve.rs, error.rs, drop_pool.rs}`).

**Why:** Owner wants shorter files for legibility. A parallel deep-module review decides WHICH files split; this guardian's angle is hexagon + public-API-path stability so a split changes zero public paths and zero layering.

**How to apply:** On any such refactor, treat `mu_core::data::<module>::X` public paths as a frozen contract — every currently-`pub` item that appears in a public signature must still resolve at the identical path (via `pub use` re-export from the new `mod.rs`). Private helpers move freely into `pub(super)` submodules; verify no helper accidentally becomes `pub`, no submodule imports `services`/`rng`, and the split forms an acyclic module DAG. Note: `core/src/services/spawn.rs` imports `data::atlas::MapHandle` — a real core-service consumer, so that path's stability is load-bearing, not just tests.
