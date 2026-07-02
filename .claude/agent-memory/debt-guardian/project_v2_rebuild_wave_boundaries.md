---
name: project-v2-rebuild-wave-boundaries
description: The v2 data-layer rebuild's named-wave scope boundaries — what is legitimately deferred vs. actual debt, so future reviews don't misflag planned scope.
metadata:
  type: project
---

The mu-core v2 static-data rebuild ships in four sequenced waves, each leaving the crate green on all four CI checks (`fmt`, `clippy -D warnings`, `test --workspace`, wasm `cargo check`).

- **R1 — components/**: unit newtypes, level-key enums, geometry, class/element/bonus/item-option vocab. Approved.
- **R2 — data/ + data-reading services/**: `data/*` record types, loader, `Atlas` (dataset-wide referential-integrity proof), curve const tables, `chance.rs` (RNG seam), pure accessors. Unit-tested against INLINE fixtures.
- **R3 — extractors**: adapt `tools/extract/*.py`, regenerate `/data` as v2 JSON.
- **R4 — integration**: rewrite `core/tests/data_files.rs` to load real v2 `/data`.

**Why:** the on-disk `/data` stays v1 until R3, so R2/R3 tests deliberately never touch the filesystem — inline fixtures are the prescribed R2 strategy, NOT a disabled-test hack.

**How to apply — these are legitimate named boundaries, do not flag as debt:**
- **Combat/drop/craft RESOLUTION formulas that consume runtime entities** (PlacedItem, kill aggregates) belong to **W-CMB / W-ENT**, not R2. They must NOT appear as bodyless sigs or `todo!` stubs (both illegal) — they stay as spec sketches. Absence of them in R2 is correct.
- **`docs/debt/openmu-default-values.md`** enumerating OpenMU-invented values is an **R3 / W-SRC** deliverable. Its absence in R2 is not a missing debt record.
- OpenMU-invented values carry a `review` string (JSON field) or a `Review:` doc note (Rust consts); authentic values carry neither. Verify a const's provenance against the relevant `v2-sections-r3/*.md` spec before flagging an unflagged value — the spec labels each value family authentic vs OpenMU-default.
- `entities/` and `events/` stay minimal placeholders until W-ENT.
