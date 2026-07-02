# Project Rules

The Four Iron Laws are absolute. They are the physics of this codebase. Every line of mu-core obeys them. No exceptions. Code that breaks a law is wrong and gets rewritten, not patched.

> **Required reading:** [`README.md`](./README.md) — the six portability rules. The Iron Laws below are abstract; the portability rules are their non-negotiable, host-facing consequences. Keep both loaded for every change.

> **Domain reference:** MU Online domain facts (entities, stats, item options, drops, crafting, formulas) are mapped from the OpenMU repo under a strict extract-WHAT-never-HOW protocol — read [`docs/openmu-reference.md`](./docs/openmu-reference.md) before planning any game feature. OpenMU's C#/OOP/persistence structure never crosses into this crate; domain facts are extracted as plain lists, scope is confirmed with the user, and our types are designed from scratch per these rules.

---

## The Four Iron Laws

These are the foundation. Everything else in this file follows from them. No exceptions, at any scale.

### 1. Hexagonal Architecture (Ports & Adapters)

mu-core **is** the hexagon. Three layers:

- **Core (this crate):** Pure domain logic — entities, stats, combat math, items, drops, skills. Zero host dependencies: no engine, no database, no network, no clock, no global RNG. Only `serde` and `rand_core`.
- **Ports:** The crate's public API. Domain types in, domain state + events out; `rand_core::RngCore` as the injected-randomness port; static-data structs as the data-loading port.
- **Adapters (hosts):** Native server, SpacetimeDB module, browser wasm, Unity FFI. Thin translation layers: parse host input into domain types, call core, persist returned state, deliver returned events. No game rules in a host, ever.

**The Dependency Rule:** `Hosts -> mu-core`. Never reversed. Core never names a host concept — no table handles, no engine object IDs, no network frames, no persistence rows. **No information leaks across layers** — layers cross only through declared ports; reaching across is the violation even if it compiles.

**Recursive application.** This rule fractals downward. *Inside* core, the module boundaries (`entities`, `components`, `services`, `events`, `rng`, `data`) obey the same dependency rule between themselves (see *Core module shape* below). The no-leak rule applies at every scale.

**Litmus test:** Could you swap SpacetimeDB for Postgres, Unity for Godot, the browser for a TUI — and keep core and its public API untouched? If no, something leaked.

### 2. Enums Everywhere

Every domain type with more than one possible shape is a Rust enum; each variant is **flat** — only the fields that variant needs. If a field would be complex, model it as its own named type. `Option<T>` is reserved for genuine domain optionality, never as an implicit state flag.

```rust
// YES
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttackOutcome {
    Missed,
    Hit { damage: Damage },
    Critical { damage: Damage },
    Killed { damage: Damage, drops: Vec<Drop> },
}

// NO — optional soup, illegal states representable
pub struct AttackOutcome {
    pub hit: bool,
    pub damage: Option<u32>,
    pub critical: Option<bool>,
    pub drops: Option<Vec<Drop>>,
}
```

Serialized domain enums are internally tagged (`kind`) with `snake_case` variant names — hosts and non-Rust clients read a stable, flat wire shape. (Examples in this file elide the doc comments real code carries — `missing_docs` is on.)

### 3. Make Illegal States Unrepresentable

**If a function runs, its preconditions are already guaranteed by its types. Core assumes correctness. Validation lives at host boundaries — never internally.**

Every type forbids invalid combinations at compile time. Fields that exist in only one state live on that variant only. Newtypes for semantic units — `Tick(u64)`, `Level(u16)`, `Zen(u64)`, `MonsterId(u32)` — so incompatible values cannot mix.

**No type-system suppressors anywhere.** Every mechanism that bypasses the type checker or the exhaustiveness proof is banned. Most are enforced at the lint layer (workspace `[workspace.lints]` + `clippy.toml`, build-failing in CI via `-D warnings`); each bullet names its enforcement, and the review-enforced ones carry the same veto:

- `.unwrap()` / `.expect()` — banned in core (`clippy::unwrap_used`, `clippy::expect_used`; tests exempt via `clippy.toml`). An unwrap is a confession the producer's type is wrong — fold the absence into an enum variant, or re-shape the producer so presence is proven by the type.
- `panic!`, `unreachable!`, `todo!`, `unimplemented!` — banned (`clippy::panic`, `clippy::unreachable`, `clippy::todo`, `clippy::unimplemented`). "Should never happen" is a type-design failure.
- Wildcard `_ =>` arms on enum matches — banned (`clippy::wildcard_enum_match_arm`; the single-remaining-variant case is caught by `clippy::match_wildcard_for_single_variants` from pedantic). Rust's `match` is the exhaustiveness proof; the wildcard defeats it. A new variant must break the build until every dispatch handles it. Partial folds enumerate their ignored variants as explicit or-patterns (`Missed | Hit { .. } => ...`) so the compiler still checks totality.
- `#[non_exhaustive]` on domain enums — never; it re-opens the wildcard hole for every consumer (review-enforced — no lint covers it).
- Inline `#[allow(...)]` — banned (`clippy::allow_attributes`). Inline `#[expect(...)]` — same ban, review-enforced: no lint covers it, and it silences restriction lints without tripping `allow_attributes`, so treat a new `#[expect]` in core as a HARD REJECT in review. If a lint legitimately does not fit, change the config layer (workspace lints, `clippy.toml`) — never the call site.
- Slice indexing (`v[i]`) — banned (`clippy::indexing_slicing`; tests exempt via `clippy.toml`). `unwrap_or` / `unwrap_or_default` on lookup-shaped expressions (`map.get(k)`, `v.first()`, `iter().find(p)`) — same ban, review-enforced. Either way the fix is upstream: re-shape the producer into a **total structure** whose type proves every queried key has a value (the lookup returns `T`, not `Option<T>`), or fold the absence into an enum variant the consumer matches. Defaults for genuinely-optional input at host parse boundaries are the only legitimate use.
- Lossy `as` casts — pedantic cast lints flag them; use `From`/`TryFrom`, converting at boundaries.
- `unsafe` — forbidden crate-wide (`unsafe_code = "forbid"`). FFI unsafety lives in the FFI host crate.
- `Default` or zeroed values fabricated to satisfy a signature — absence is a variant, not a default (review-enforced).

Suppressor-bypass attempts at the producer side are no exception. The fix is upstream: re-shape the type, split the variant, fold the absence into the discriminator. The runtime contract is enforced by structure, not by check.

**Banned in core:**
- Defensive checks on already-typed values
- `"should never happen"` branches (including `debug_assert!` compensating for weak types)
- Boolean flags controlling flow — model as enum variants
- `Option` fields used as implicit state conditionals
- Runtime asserts compensating for weak types

Reaching for a runtime check inside core means the type is wrong. Split into variants.

### 4. Design Discipline — Solve More With Less

**Write code that matches the shape of the problem.** This rule is **anti-defensive, not anti-line-count.** A junior dev writes a long algorithm with manual `if`s for every edge case; a pro writes precise code that handles all of it through a better mental model. The pro's version is sometimes shorter — and sometimes longer — but always *precise* rather than *defensive*. Short code is evidence of understanding, never the goal.

**More code is fine — even preferred — when it:**
- Adds an enum variant that makes an illegal state unrepresentable.
- Splits one branchy function into per-variant handlers with cleaner execution paths.
- Expands a type to forbid invalid combinations at compile time.

**Less code is the goal only for defensive cruft:**
- Guards on already-typed values.
- "Should never happen" / unreachable branches.
- Special-case `if`/`match` ladders that exist because the type is wrong.
- Wrappers, indirections, and flexibility knobs added "just in case."
- Optional fields used as implicit state flags.

**Other principles:**
- Each module exposes the **minimum**; invalid states should be unrepresentable, not guarded against.
- **No abstraction, indirection, or flexibility** exists until it has already earned its place by removing more complexity than it adds.
- **Clarity outranks brevity.** Compression that hurts readability is the opposite of this principle. Never chase line count for its own sake.

**Bad code is a symptom — the fix is at the root.** The visible mess (an awkward branch, a cast, a helper, a duplicate block) is never the bug. The bug is upstream: a wrong type, a missing variant, a leaked layer, a broken abstraction. This applies to **everything written in this repo** — code you touch, plans you draft, solutions you propose. If a plan needs a workaround, the frame is wrong; remodel before writing. If a solution feels forced, the shape is wrong; stop and rethink. Patching the symptom is forbidden at every scale.

Symptoms that the model is wrong:
- A growing ladder of `if`/`match` branches for "special cases"
- Utility helpers whose only job is to paper over a shape mismatch
- Wrapper layers, adapters-of-adapters, or pass-through indirections added "for flexibility"
- Configuration knobs and optional flags accumulating on a single function
- Copy-pasted blocks that *almost* match but diverge in one detail driven by a missing enum variant
- Defensive `unwrap` / `clone` / nullable-check ladders silencing the compiler instead of fixing the type
- **A module reaching into another module's internals.** Missing-port symptom — fix the port, not the call site. (See *No information leaks* below.)
- **Functions with long parameter lists** (≤5 is the soft target; 6+ triggers scrutiny — see *Deep modules, not shallow ones* below). Shallow-module symptom — deepen the module instead of bundling params into config structs.

The fix is always upstream: re-model the types, split or merge variants, move the responsibility to the module it belongs in. A better frame deletes *defensive* code; it does not necessarily delete total code.

**Forbidden phrases** — each is a confession the root was not fixed: *"quick fix"*, *"for now"*, *"good enough"*, *"we can clean up later"*, *"clean up later"*, *"as a first step"*, *"first step"*, *"minimal version"*, *"stub for X"*, *"stub"*, *"workaround"*, *"temporary"*, *"will refactor later"*, *"refactor later"*, `// TODO` / `// HACK` / `// FIXME` left behind. If you catch any of these in your own draft (code, plan, brainstorm option, PR description), STOP — the proposal is not an option; either deliver the complete root-level fix or split into properly-scoped complete waves. Debt accumulates like cancer; one patch breeds the next. **Zero accumulation. No exceptions.**

---

## Supporting Rules

These follow from the four laws above.

**Parse, don't validate.** At every host boundary (network payloads, DB rows, engine calls, static data files), the host parses raw input into a domain type once — `TryFrom` / `parse_*` functions returning `Result<Domain, ParseError>`. Downstream core code receives the parsed type and never re-checks. Smart constructors guard every domain invariant (`Level::new(u16) -> Result<Level, StatError>`); if a value exists, it is valid.

**Compile-exhaustive dispatch.** Rust's `match` is the totality proof — but only if nothing defeats it. No `_ =>` wildcard arms on domain enums, no `#[non_exhaustive]` domain types, no if-ladders with a bare terminal `else`. A new variant must break the build until every dispatch handles or explicitly ignores it (explicit or-patterns, not wildcards). Enforced by `clippy::wildcard_enum_match_arm`.

**Events, not effects.** Every observable outcome of a service — damage dealt, item dropped, level gained, skill rejected — is a value in the returned events. Core never logs, never sends, never persists; it has no I/O to do it with. Hosts own event delivery, envelopes, wire versioning, and persistence at their boundary. A service whose outcome isn't in its return value is leaking through a side channel.

**Determinism.** Every service is a deterministic function: `(state, input, &mut impl RngCore) -> (new state, events)`. Same inputs + same RNG seed = same outputs, on every target — native, wasm, FFI. This is what makes the simulation replayable, testable, and host-agnostic. Advancing the injected RNG is the only sanctioned mutation of an argument.

**No hidden state.** No `static mut`, no `thread_local!`, no lazy statics, no `OnceCell`/`OnceLock` registries, no interior mutability (`RefCell`/`Cell`/`Mutex`) in domain types. All state flows through parameters and return values. Tick-based time only: `Tick` values come in as input; core never reads a clock (lint-enforced: `SystemTime`/`Instant` are disallowed types).

**Early returns.** Guard clauses first — `let .. else`, `?` on `Result` at boundaries. No deep nesting. No `else` after `return`. Happy path at lowest indentation.

**Naming.** Standard Rust: `snake_case` functions/modules/fields, `PascalCase` types and enum variants, `SCREAMING_SNAKE_CASE` consts. Serialized `kind` tags are `snake_case` via `#[serde(rename_all = "snake_case")]`.

**Comments.** Default to none beyond the required module/item doc comments (`missing_docs` is on). Code is the source of truth; identifiers carry intent; types carry shape. Add an inline comment only when the *why* is non-obvious (a hidden constraint, a subtle invariant). Never explain *what*. Never reference callers, related files, or "this exists because of X" — that rationale belongs in the PR/commit, not the source. **One narrow exception:** a comment may name an *open* wave ID as a forward-pointing anchor for follow-up work that lives in another wave's scope (e.g. `// W6h widens this payload`). The anchor must be deleted at that wave's closure — leaving a closed-wave reference behind is rot and is forbidden.

**No information leaks between responsibilities — universal.** This rule is the operational expression of **Iron Law 1 applied recursively** at every scale: not just between core and hosts, but between modules within core. Each module owns **one concern** and is unaware of every other module's internals; modules cross only through declared interfaces (function signatures, parameter types, return types). If a module needs something from another, the answer is a port, not a reach. Even if it compiles, reaching across is the violation.

**Deep modules, not shallow ones — long parameter lists are a symptom.** A long parameter list means the module is **shallow** (wide interface; callers pay cognitive load while the module hides nothing). Fix is to **deepen the module** (absorb complexity behind a smaller interface — module hides more, caller knows less), never to bundle params. **Banned cosmetic patches** — each re-shapes the call site while leaving the module unchanged:

- **Extract Parameter Object / config struct** — lifting positional params into `fn f(cfg: Config)` where `Config` is a grab-bag.
- **Builder pattern** — fluent chained-method API (`X::builder().a(a).b(b).build()`) wrapping the same wide interface.
- **Classitis** — splitting one coherent operation across multiple types/modules that ping-pong intermediate state.

The real fix is upstream: a missing enum variant absorbs toggle/mode params (Iron Law 2); the operation is recomposed so data never crosses; the params were wrong-layer (Iron Law 1). **Litmus:** if a wrapper cleans the call site without changing the module, the smell is unchanged. **Soft anchor:** ≤5 params is the target; 6+ triggers scrutiny, not rejection — ships only when every param is a distinct non-bundleable domain entity at the right layer and no upstream reshape shrinks the count. Bundling is never the fix.

---

## Core Module Shape

Core is the pure hexagon (`core/src/`). Inside it, the same no-leak rule applies between modules:

- **`components/`** — serializable value types (stats, vitals, positions, inventories) with invariants held by smart constructors. Data only; no rules.
- **`entities/`** — aggregates composed from components (characters, monsters, world items). Data only; no rules.
- **`data/`** — static game data shapes (monster defs, item defs, drop tables, skill defs) and the total lookup structures hosts fill. Data only.
- **`events/`** — outcome enums returned by services. Data only.
- **`services/`** — **all behavior.** Pure decision functions over entities/components/data: combat math, drop resolution, leveling, skill application. The only module with logic.
- **`rng/`** — the injected-randomness seam around `rand_core::RngCore`. Plumbing only; no domain knowledge.

Dependency rule inside core: data-shaped modules (`components`, `entities`, `events`, `data`) never import `services` or `rng`. `services` composes everything. `rng` knows only `rand_core`. A component never rolls dice; an entity never decides; an event never computes.

## Host Adapter Shape

Future host crates (`hosts/`) are vertical adapter slices, each with the same internal discipline:

- **handlers** own transport (SpacetimeDB reducer, HTTP route, FFI entrypoint, JS binding). Parse inbound payloads, call core services, return results. No domain decisions inline — ever.
- **codec / schema** parses raw bytes/rows into domain types once. Aware of nothing else.
- **persistence** owns reads and writes of core state. Unaware of transport and domain decisions.
- **delivery** routes returned events outward (log, packet, table update, callback). Unaware of how events were produced.

A host never re-implements a rule; persistence never leaks into core; engine and DB IDs stay in the host, mapped to domain newtype IDs at the boundary.

---

## Agent Orchestration

When a task produces or modifies code, spawn agents — you orchestrate, you don't write code yourself. For questions/explanations, respond directly.

### Agent Roster

| Agent | Domain |
|---|---|
| `prompt-optimizer` | *(optional)* Refines vague/ambiguous prompts before other agents run |
| `bdd-tdd-spec-writer` | BDD scenarios + TDD test bullets before implementation |
| `core-architecture-guardian` | Ensures domain logic lives in core services, purity and determinism hold, and no host concept leaks in. Runs in plan or code mode. |
| `state-machine-agent` | Designs state as enums with pure transition functions |
| `deep-module-guardian` | Module-depth auditor (Ousterhout). Flags shallow modules, pass-throughs, state duplication, temporal decomposition, complexity pushed to callers, premature generalization. Planned scaffolding with a named wave gets a pass. Runs just before `canon-guardian` (code mode). |
| `canon-guardian` | Industry-canon enforcer. When code implements a known concept (state machines, entity/component modeling, drop tables, fixed-point math, event patterns, etc.), verifies the textbook canonical form — not a custom invention, half-implementation, or wrong pattern. Runs in plan mode alongside `core-architecture-guardian`, and in code mode after `deep-module-guardian`, before `debt-guardian`. |
| `debt-guardian` | Zero-debt enforcement. Traces every symptom to its root cause and demands the root-level fix. When a fix genuinely requires a future wave, creates a formal debt record with resolution plan. Manages the debt backlog. Runs just before `rules-guardian`. |
| `rules-guardian` | Final audit against these rules. Always runs last. |

Host-crate builder agents (data adapters, UI, routes, server slices) are deliberately absent — they get ported from the source roster (`../project-04/real-time-players-app/.claude/agents/`) when host crates exist.

### Pipelines

Pick the matching pipeline. Run top-to-bottom. `rules-guardian` always last. `prompt-optimizer` only when the request is genuinely vague — unclear scope, conflicting requirements, can't tell what "done" looks like. Skip it for concrete, specific, or well-scoped asks.

- **Core Domain Feature:** `bdd-tdd-spec-writer` -> `core-architecture-guardian` (plan) -> `canon-guardian` (plan) -> `state-machine-agent` -> implementation -> `core-architecture-guardian` (code) -> `deep-module-guardian` -> `canon-guardian` (code) -> `debt-guardian` -> `rules-guardian`
- **Refactor/Cleanup:** `core-architecture-guardian` -> `deep-module-guardian` -> `canon-guardian` -> `debt-guardian` -> `rules-guardian`
- **Bug Fix:** `bdd-tdd-spec-writer` -> relevant implementation agents -> `deep-module-guardian` -> `canon-guardian` -> `debt-guardian` -> `rules-guardian`

### Orchestration Rules

- You do not do an agent's job yourself. Each agent owns its domain.
- Never spawn an agent whose domain is not touched by the task.
- If an agent's output invalidates earlier work, re-run affected agents.
- When unsure which pipeline, use the fuller one.

---

See also [`README.md`](./README.md) for the portability rules and development commands.
