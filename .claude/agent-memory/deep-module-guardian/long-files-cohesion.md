---
name: long-files-cohesion
description: Ruling that mu-core's long source files are deep/cohesive, not shallow; the file-split trigger is concern-separability, not line count
metadata:
  type: project
---

Structural audit ruling (branch spatial-foundation era). The owner worried some source files are "too long." Verdict: none of the long files are shallow; length is cohesion or domain breadth.

**Why:** file length is exactly the trigger CLAUDE.md forbids ("Deep modules, not shallow ones"; Iron Law 4 is anti-defensive, not anti-line-count). Splitting a cohesive deep module to cut lines is classitis.

**How to apply — do NOT re-flag these on length:**
- `data/atlas.rs` (~1216 loc): ONE deep module. `Atlas` struct + `parse` + all `resolve_*`/`index_*`/`check_*`/`require_*` free fns + `AtlasError` + resolved views (`MapHandle`/`Landing`/`SpawnEntry`/`WarpView`/`EnterGateView`) all share the resolved-store knowledge. Builder+product sharing private representation = temporal decomposition if split. AtlasError is 1:1 with each parse proof. Length = the irreducible referential-integrity proof over 14 cross-referencing data files. Only genuinely separable concern: `DropPool` (a per-level drop index, zero coupling to Atlas internals) — optional extraction to `data/drop_pool.rs`, mild win, not mandatory.
- `components/spatial.rs`: interreferential fixed-point 2.5D value algebra (Fixed/WorldPos/WorldVec/Radius/DistanceSq/Facing/ConeHalfWidth/WorldRect/Region). Types meaningless in isolation. Strongest KEEP-WHOLE.
- `components/tile.rs`, `components/units.rs`, `components/levels.rs`, `components/bonus.rs`, `data/classes.rs`, `data/item_definitions.rs`: each is one cohesive vocabulary or one data-file schema. Length = domain breadth (variant counts, per-level exhaustive match arms mandated by Iron Law 3 no-indexing). KEEP-WHOLE.

**Split trigger (proposed convention, not yet in CLAUDE.md):** promote `foo.rs` → `foo/{mod.rs,...}` ONLY when the file holds 2+ modules that hide different things from each other (each has its own interface, neither needs the other's internals). NOT triggers: raw line count, a big single-vocabulary enum, builder+product sharing private repr, a type + its 1:1 error enum, a family of peer newtypes sharing one error. Private child files (interface byte-identical, no new public path) are a permitted navigation aid, never required.

**Test-location rule (proposed):** inline `#[cfg(test)]` = white-box tests of a type's private invariants/serde; `core/tests/` = cross-file/dataset/service contracts against real `/data`. atlas.rs correctly has no inline tests — its whole job IS the cross-file contract, covered by `core/tests/data_files.rs`.
