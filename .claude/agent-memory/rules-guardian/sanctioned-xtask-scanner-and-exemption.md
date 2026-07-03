---
name: sanctioned-xtask-scanner-and-exemption
description: Review precedent — the xtask ban-scanner crate is dev-tooling exempt from Iron-Law lints; its direct-receiver-only lookup-unwrap_or detection gap is an accepted nit, not a veto
metadata:
  type: project
---

W-HARDEN added an `xtask/` workspace member (`cargo xtask scan`, a `syn::visit`
scanner) that flags the four review-enforced bans in `core/src` with `file:line`
+ non-zero exit: lookup-shaped `unwrap_or`/`unwrap_or_default`, inline
`#[expect(..)]`, `#[non_exhaustive]` on an enum, fabricated `Default`
(`Default::default`/`T::default`/`..Default::default()`/`#[derive(Default)]`).

**xtask is EXEMPT from mu-core's rules — do NOT flag its internals.** It omits
`[lints] workspace = true` (verify: the only "lints" text in `xtask/Cargo.toml`
is a prose comment, no `[lints]` table header) and never enters mu-core's
dependency graph (`cargo tree -p mu-core` shows no `syn`/`walkdir`/`proc-macro2`;
core deps stay serde + rand_core). So xtask may freely use `.unwrap()`/`.expect()`/
`env!`/`.unwrap_or()`. It uses `writeln!(stdout/stderr)` NOT `println!` because
`clippy.toml`'s `disallowed-macros` (std::println/eprintln/dbg family) is GLOBAL
to a clippy run and fires workspace-wide — `writeln!` is not on that list. This
`writeln!` choice is REQUIRED and rule-compliant, not a smell.

**Accepted detection boundary (reviewed, W-HARDEN — do not re-litigate as a
blocker):**
- Scanner scans `core/src` ONLY, not `core/tests`. Correct: tests legitimately
  use these idioms (see [[sanctioned-test-harness-idioms]] — counter-map
  `unwrap_or(0)`), and unwrap/panic are test-exempt via clippy.toml.
- lookup-unwrap_or fires only when the lookup method (`get`/`first`/`last`/`find`)
  is the DIRECT receiver of `unwrap_or`. An intervening `.copied()`/`.cloned()`/
  `.map(..)` (e.g. `map.get(k).copied().unwrap_or(0)`) is NOT caught. This is a
  known false-negative gap — acceptable because the scanner is an explicit
  pre-flight aid ("not the sole gate"; rules-guardian review is authoritative)
  and the direct/canonical shape has teeth. Raised as a NIT, never a veto.
- The four existing `try_from(..).unwrap_or(MAX)` saturating narrows are
  correctly NOT flagged (receiver is `try_from`, not lookup-shaped) — see
  [[sanctioned-result-collapse-and-narrow]].

**How to apply:** Do NOT HARD REJECT xtask for unwrap/expect/writeln — it is dev
tooling. Do NOT demand the scanner catch adapter-chained lookups as a blocker
(it is a documented pre-flight, review is the gate). Only escalate if the
scanner ever enters core's dep graph, gains `[lints] workspace = true`, or a
genuine direct-shape ban lands in `core/src` and the scanner misses it.
