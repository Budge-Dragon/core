# mu-core

Pure game logic for a MU Online rewrite: entities, stats, combat math, items,
drops, and skills as a plain Rust library.

## The core rule: zero host dependencies

The same crate must compile and behave identically everywhere it is embedded:

- **native** — game server / SpacetimeDB module
- **`wasm32-unknown-unknown`** — browser
- **FFI** — Unity bindings (via a future host crate)

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

Architecture and coding laws live in [`CLAUDE.md`](./CLAUDE.md) — required
reading before writing any code.

`clippy.toml` mechanically catches the common violations of rules 1–3:
`SystemTime`/`Instant`, `thread::{spawn, Builder::spawn, scope, sleep}`, and
the `print!`/`dbg!` macro family. Async and everything else in the rules are
convention, enforced by code review.

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
cargo check -p mu-core --target wasm32-unknown-unknown   # browser target
cargo test
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```
