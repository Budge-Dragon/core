# mu-core

Pure game logic for a MU Online rewrite: entities, stats, combat math, items,
drops, and skills as a plain Rust library.

## The core rule: zero host dependencies

The same crate must compile and behave identically everywhere it is embedded:

- **native** — game server / SpacetimeDB module
- **`wasm32-unknown-unknown`** — browser
- **Unity** — iOS (`aarch64-apple-ios`), Android (`aarch64-linux-android`), and
  WebGL (`wasm32-unknown-emscripten`). The pure core must compile for all three
  (enforced in CI); the C-ABI FFI shim that exposes it to C# lands in a future
  host crate.

## Portability rules

1. **No wall-clock time.** The simulation is tick-based; hosts own the clock.
2. **No async or threading.** The core is synchronous, single-threaded logic.
3. **No logging.** Services return events; hosts decide what to log, persist,
   or broadcast.
4. **No engine or DB types/IDs.** Plain Rust types only.
5. **RNG injected via trait.** `rand_core::RngCore` is passed in by the host,
   never a global generator. Deterministic given a seed.
6. **Static game data is defined as structs here.** The core defines the
   shapes and the rules that read them; hosts load the data.
7. **No float math.** All arithmetic is integer or `Q40.24` fixed-point
   (`components::spatial::Fixed`). `f32`/`f64` round differently across
   native/wasm/FFI and break replay determinism.
8. **Client proposes, server decides.** Every service takes a typed *intent*
   plus current state and computes the authoritative *result*; it never trusts a
   client-claimed outcome. Intent types in, distinct result/event types out.

Architecture and coding laws live in [`CLAUDE.md`](./CLAUDE.md) — required
reading before writing any code.

Mechanical enforcement (build-failing under `cargo clippy -- -D warnings`):
`clippy.toml` disallows `SystemTime`/`Instant` (rule 1),
`thread::{spawn, Builder::spawn, scope, sleep}` (rule 2), the `print!`/`dbg!`
macro family (rule 3), and entropy-seeded `HashMap`/`HashSet`/`RandomState`
(rule 5 — nondeterministic iteration order); `[workspace.lints.clippy]` sets
`float_arithmetic` (rule 7). Async, engine/DB types, injected RNG, and the
authoritative intent→result flow (rule 8) are convention, enforced by code
review. The full architectural laws — including the six authoritative-server
invariants — live in [`CLAUDE.md`](./CLAUDE.md).

## Dependencies

`serde` (derive) and `rand_core`. Nothing else is allowed in `core`.

## Layout

- `core/` — the library crate (`mu-core`)
  - `src/entities/` — aggregate game objects (characters, monsters, items)
  - `src/components/` — serializable value types entities compose
  - `src/services/` — pure rule functions (combat, drops, leveling, skills)
  - `src/events/` — outcomes returned instead of side effects
  - `src/rng/` — injected-randomness plumbing
  - `src/data/` — static game data struct definitions
- `hosts/` — future host crates (placeholders, see `hosts/README.md`)

One concept per file; a file grows to a directory module (`foo/{mod.rs, …}`)
only when it holds separable concerns, never for line count. Unit tests live
inline; cross-file/dataset contracts live in `core/tests/`. The full
file-organization rule is in [`CLAUDE.md`](./CLAUDE.md) ("File & module
organization").

## Development

```sh
cargo check                                              # native
cargo test
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings

# Portability gate — the pure core compiles for every deployment target.
# One-time: rustup target add wasm32-unknown-unknown wasm32-unknown-emscripten \
#           aarch64-apple-ios aarch64-linux-android
cargo check -p mu-core --target wasm32-unknown-unknown      # browser
cargo check -p mu-core --target wasm32-unknown-emscripten   # Unity WebGL
cargo check -p mu-core --target aarch64-apple-ios           # Unity iOS
cargo check -p mu-core --target aarch64-linux-android       # Unity Android
```

### Review-enforced ban scanner

Some Iron-Law bans have no clippy lint (`CLAUDE.md`, Iron Law 3): lookup-shaped
`unwrap_or`, inline `#[expect(..)]`, `#[non_exhaustive]` on an enum, and a
fabricated `Default`. The `xtask` dev tool scans `core/src` for them and exits
non-zero with `file:line` on any hit:

```sh
cargo xtask scan
```

Install it as a pre-commit gate once per clone (CI runs the same step, so the
hook is a fast local pre-flight, not the sole gate):

```sh
git config core.hooksPath .githooks
```

### Cross-target determinism (executed, not just compiled)

Determinism is exercised under wasm, so identical results on native and wasm are
proven by execution. Requires a wasm runner (e.g. [wasmtime](https://wasmtime.dev)):

```sh
# One-time: rustup target add wasm32-wasip1
CARGO_TARGET_WASM32_WASIP1_RUNNER=wasmtime \
  cargo test -p mu-core --test wasm_determinism --target wasm32-wasip1
```

(The full test suite is not wasi-buildable — `proptest` pulls `wait-timeout` —
so the property tests run on the native legs and this dedicated proptest-free
test is the one wasmtime runs.)
