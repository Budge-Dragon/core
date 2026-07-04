# Hosts

Host adapters are **separate repositories** that consume `mu-core`. This repo
ships the pure core only and contains no host crates. Planned hosts:

- game server — SpacetimeDB module
- clients — Unity (iOS, Android, WebGL) via a C-ABI FFI shim, and browser wasm

A host is a thin translation layer around the core:

1. **Parse** raw input (packets, rows, engine calls) into typed domain intents
   at the boundary — once.
2. **Call** core services with the intent plus current state.
3. **Persist** the returned state.
4. **Deliver** the returned events (log, packet, table update, callback).

Hosts own I/O, persistence, networking, the clock, and the RNG seed. No game
rule is ever implemented in a host, and `mu-core` never depends on a host.
