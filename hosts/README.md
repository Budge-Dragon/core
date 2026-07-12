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

## Anti-cheat duties

Client proposes, server decides. Two layers:

**Core already checks single-action legality — don't redo it.** Distance/range,
reachability, ≤1-tile step + no wall-tunnelling, target/cost/eligibility, all
damage/drop/kill. So "buy from far" and "teleport" are already rejected. The host just
calls core and honours the rejection.

**Host owns rate + identity** (core is clockless, can't see them):

1. Per-tick action budget — one action per tick; advance on the *server's* clock, not
   on message arrival. (Stops the "Flash" cheat: many legal moves sent too fast.)
2. Drop out-of-order / too-frequent requests.
3. Admit only live, authenticated actors (the dead-actor gate).
4. Bind the intent to the authenticated caller.

**SpacetimeDB:** clients can't touch tables — they only call your reducer, which runs
`mu-core` and decides. Each call gives you the caller identity + server timestamp. It
does **not** rate-limit for you — duties 1–2 are reducer code you write.

### PvP (W-PVP)
- Stamp each `CombatProfile.kind` from the entity type — `Player` from a `Character`,
  `Npc` from a `MonsterInstance` — never from client bytes.
- Translate the client's force-attack modifier (CTRL-click) + selected entity into a
  batch index and pass `Designation::Forced { target_index }`; otherwise `Incidental`.
  Single-target skills (`DirectHit`/`Lunge`) MUST supply `Forced` or the cast rejects
  `NoTargetsInRegion`. Keeping the target batch stable between click and resolution is
  a host duty (like rate-limiting).
- On `TargetHit::Killed` / `AttackOutcome::Killed`, map the index to the entity and route
  by kind: `Npc` → `resolve_kill` (exp/loot); `Player` → `resolve_death(victim, at, tick,
  atlas, combat_death_penalty(attacker_kind))` + `respawn` only (both by value). Pass
  **core's computed** penalty — never a host-originated Waived/Applied literal; the
  rule (a player kill costs the victim nothing) lives in core. Reputation is W-PK.
