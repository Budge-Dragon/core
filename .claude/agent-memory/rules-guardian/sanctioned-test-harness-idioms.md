---
name: sanctioned-test-harness-idioms
description: Review precedent — or_abort infallible-load confinement and counter-map unwrap_or(0) in core/tests are sanctioned, not banned suppressors
metadata:
  type: project
---

`core/tests/` files are separate integration crates — NOT `cfg(test)` — and
`clippy.toml`'s `allow-*-in-tests` exempts banned constructs only *lexically
inside* `#[test]` fns. So non-`#[test]` shared harness code (e.g.
`core/tests/common/mod.rs`) gets the full restriction-lint treatment and must be
suppressor-clean. Two idioms this forces are sanctioned (audited in W-MOV
simulation suite); do NOT flag either in future reviews.

**1. `or_abort` for infallible-but-`Result`-typed loads.** A `fn or_abort<T,E:
Display>(Result<T,E>) -> T` that matches, `writeln!`s the error to stderr, and
calls `std::process::abort()` — used to unwrap file reads, `serde_json`,
`Atlas::parse`, `TickDuration::new`, terrain parses in shared harness fns
(`real_atlas`, `tick`, `load_terrain`). Legit because: (a) it is test-support
code, not core; (b) file I/O is a *genuine* runtime fallibility, not a weak type
— you cannot type-prove a checked-in file exists on disk (this is the host parse
boundary, and the harness plays host); (c) `abort` is NOT in the banned list
(`unwrap`/`expect`/`panic!`/`unreachable!`/`todo!`/`unimplemented!`/slice-index/
lossy-`as`/`unsafe`/fabricated-`Default`) and is clippy-clean; (d) it reports the
error honestly and fabricates no value. It is the DEEP fix vs. the shallow
alternative (threading `Result` through every helper and unwrapping at each
`#[test]` call site). Mirrors how `data_files.rs` confines its `unwrap`s to
`#[test]`-body macro expansions.

**2. Counter-map `map.get(&k).copied().unwrap_or(0)` in test assertions.** A
frequency/histogram accumulator (`BTreeMap<K,u64>` built with `entry().or_insert`)
read back with `unwrap_or(0)` for a key that may be absent. The review-enforced
`unwrap_or`-on-lookup ban (see [[sanctioned-result-collapse-and-narrow]]) targets
*domain-producer* code where absence should be proven impossible or folded to an
enum variant. A local test-side counter where an absent key genuinely and
correctly means "occurred 0 times" is honest and total in meaning — `0` is the
true value, not a fabricated default papering over a bug. Acceptable in test
code; the explicit `match { Some(&c)=>c, None=>0 }` form is equivalent and
optional, not required.

**How to apply:** Neither is a HARD REJECT. Escalate `or_abort` only if it starts
folding a *domain* decision (not an I/O/parse load) or fabricating a value.
Escalate the counter-`unwrap_or` only if it appears in `core/src` domain-producer
code rather than a test accumulator. See also [[sanctioned-total-select-idiom]].
