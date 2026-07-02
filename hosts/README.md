# Host crates (placeholders)

Future crates that embed `mu-core`. Planned:

- `server/` — native game server (SpacetimeDB module)
- `wasm/` — browser build
- `ffi/` — C ABI bindings for Unity

Each host owns its own I/O, persistence, networking, clock, and RNG seed —
`mu-core` stays pure. Add new crates here and register them in the workspace
`members` list in the root `Cargo.toml`.
