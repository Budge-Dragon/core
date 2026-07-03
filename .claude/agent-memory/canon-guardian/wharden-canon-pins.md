---
name: wharden-canon-pins
description: W-HARDEN tooling review rulings — xtask ban-scanner canon, sanctioned try_from-clamp sites, wire-pin teeth
metadata:
  type: project
---

W-HARDEN added the mechanical enforcement layer for the four review-enforced Iron-Law-3 bans that no clippy lint covers (lookup-shaped `unwrap_or`, inline `#[expect]`, `#[non_exhaustive]` enum, fabricated `Default`) via a `cargo xtask scan` syn AST visitor.

**Why:** These bans were pure review-convention; W-HARDEN gives them build-failing teeth in CI + a pre-commit hook.

**How to apply (stable facts for future canon reviews):**
- The `cargo xtask` shape is canon-correct: `xtask/` is a separate workspace member, invoked via `.cargo/config.toml` `[alias] xtask`, omits `[lints] workspace = true`, and its `syn`/`proc-macro2`/`walkdir` deps never enter mu-core's graph (core stays serde + rand_core). `syn::visit::Visit` is the standard AST-lint approach. Do not re-litigate the pattern.
- These four `X::try_from(..).unwrap_or(MAX)` sites are SANCTIONED saturating clamps, NOT lookup-shaped `unwrap_or` — they are path-calls (`ExprCall`), so the scanner correctly ignores them. Do NOT flag them as the lookup ban: `components/spatial.rs`, `services/profile.rs`, `services/ratio.rs`, `services/loot.rs`.
- Known scanner gap I flagged (verify current state before reusing): the lookup-`unwrap_or` detector only matches when the lookup (`.get/.first/.last/.find`) is DIRECTLY adjacent to `.unwrap_or(_default)`. Any Option adaptor in between (`.copied()/.cloned()/.map()/.and_then()`) evades it — a real false negative in this Copy-newtype-heavy codebase where `map.get(&k).copied().unwrap_or(x)` is the natural form. Empirically reproduced via syn. Fix is to descend the receiver through Option-preserving adaptors before testing for a lookup terminal.
- Q1 wire-drift pins (`core/tests/wire_drift.rs`) are exact-JSON `assert_eq!` with real teeth, but pin ONE variant per internally-tagged enum — an unpinned variant's `kind`-tag rename drifts silently. Related: [[wcmb-canon-pins]], [[wmov-canon-pins]].
