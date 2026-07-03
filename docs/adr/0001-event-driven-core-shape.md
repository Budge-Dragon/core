# ADR 0001 — Event-driven core: events as output, not event sourcing

- **Status:** Accepted
- **Date:** 2026-07-03
- **Decider:** tech authority, on behalf of the product requirement "the architecture will be event-driven" (product owner is a PM; the technical shape is delegated).
- **Applies to:** every `core/src/services/*` and `core/src/events/*` in every wave.

## Context

mu-core is a full MU Online rewrite: a live, server-authoritative MMORPG world
simulation with modernization touches, running behind a host-agnostic hexagon
(native server, SpacetimeDB module, browser wasm, Unity FFI). The product
requirement is "event-driven." That term is ambiguous; two candidate shapes:

- **Events as output** ("events, not effects"): a service is a pure
  `(state, input, &mut impl RngCore) -> (new state, events)`; events are
  first-class, typed, exhaustive outcome values the host delivers. *This is
  already what `CLAUDE.md` prescribes.*
- **Event sourcing**: `decide(state, cmd) -> events` + `evolve(state, event) ->
  state`; the append-only event log is the source of truth and state is its
  fold. (This is the pattern the sibling `players-app` project uses.)

Both are loosely "event-driven"; they differ in whether state **is** the events
or is produced **alongside** them.

## Decision

The core is event-driven in the **events-as-output** sense. We do **not** adopt
event sourcing in core:

- Services stay pure deterministic transitions returning rich, flat,
  `kind`-tagged, exhaustive event enums (`core/src/events/`).
- There is **no** `decide`/`evolve` split, **no** event-log-as-source-of-truth,
  and **no** event store, replay-fold runtime, or projection engine in core.
- Event delivery, wire envelopes, persistence, and any append-only log live in
  **host adapters**, never in core.

## Rationale (why this shape, for *this* system)

1. **Live-world scale.** Thousands of entities move/fight per tick.
   Reconstructing world state by folding a high-volume event log is
   impractical; snapshot state + event notifications is the standard MMO shape.
2. **Determinism already gives replay.** An Iron Law guarantees
   `seed + input log -> bit-identical state` on native/wasm/FFI. That delivers
   the main other benefit of event sourcing (replay/debugging) with no extra
   machinery.
3. **Network/sync/spectating are served by events-as-output.** The host
   broadcasts the returned events; event sourcing is not required for them.
4. **Persistence fit.** Large mutable game state (inventories, world,
   progression) persists simply as snapshots. The intended SpacetimeDB host
   stores table snapshots mutated by reducers — itself closer to
   direct-transition than to event sourcing.
5. **Hexagon cleanliness.** The event store / replay runtime is a persistence
   concern; keeping it out of core prevents a layer leak. (`players-app`'s
   event-sourcing runtime lives in its app and must not be imported here.)
6. **Cost vs benefit.** Event sourcing's real wins — audit trails, CQRS
   projections, temporal queries — matter for a collaborative business app, not
   a game-world sim; its `decide`/`evolve` ceremony is not repaid here.

## What we keep (the event-driven disciplines that DO apply)

- **Events, not effects** — every observable outcome is a returned event value;
  core has no I/O.
- **Rich, flat, kind-tagged, exhaustive event enums** per service (Iron Law 2/3).
- **Pure deterministic services** — no hidden mutation; RNG injected.
- **Hosts react to events** — deliver, broadcast, persist, log — at their
  boundary.

## Consequences

- Every wave (W-ENT, W-CMB, W-MOV, …) builds services as
  `(state, input, &mut impl RngCore) -> (new state, events)` and populates
  `events/`. This is **cross-cutting and decided here once** — individual
  sessions choose *which* events to emit, never the state-transition shape.
- If a future feature genuinely needs an append-only audit log or match replay
  beyond seed-replay (e.g. trade/economy audit, arena replays), that log is
  built in a **host adapter** over the already-returned events — never by
  event-sourcing core state.

## Revisit if

The product pivots to a design that requires event-sourced world state — e.g.
authoritative rollback netcode demanding per-event state reconstruction. Re-open
this ADR at that point.
