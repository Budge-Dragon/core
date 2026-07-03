# mu-core — Expansion Release Model

The strategic frame for *how* mu-core ships: not one monolithic "finished game,"
but a sequence of **episodic expansions** (WoW-style), each a small, doable,
independently-releasable chunk. This doc is the durable home for that idea and for
the content backlog it organizes. It is vision, not a build schedule — the concrete
per-wave engineering plan lives in `docs/WORKPLAN.md` (do not fold this into it).

Companion docs: `Preserve.md` (identity to keep), `Ideas.md` (modernization lenses).

---

## The idea

Ship the game as a series of **expansions**. Each expansion is:

- **One region / map cluster** (its maps, terrain, monsters, spawns).
- Wrapped in **original lore and story** — *not* classic MU Online's lore. Classic
  MU maps, mobs, and items are the raw material; the narrative, quests, and boss
  identities are **ours**, written fresh.
- A **raised level cap**.
- Its own **new/unlocked content**: items, quests, events, shops.
- A **capstone boss** with a meaningful reward.
- A **progression gate**: defeating the capstone boss **unlocks the next
  expansion**.

## Why this cut is powerful

- Turns an impossibly large game into **small, realistic, shippable chunks** — one
  region at a time, each playable end-to-end on its own.
- **Bounded scope per release.** An expansion's content is finite and enumerable,
  so "done" is definable.
- **Built-in pacing.** The capstone boss is a natural gate; players progress region
  by region, and development proceeds the same way.
- **Grounded in the data layer.** Each expansion maps directly onto the static-data
  the core already models (maps, monsters, items, drops, skills — see
  `data/coverage_report.md`). Deciding an expansion = deciding which of those
  records are live in it.

## The grounding requirement (what makes a chunk real)

Every expansion must ship a **content manifest** enumerating exactly what is
available in it. Without this the chunk is a vibe, not a plan. Each manifest lists:

- **Maps** (which map numbers / regions + terrain)
- **Monsters & spawns** available
- **Items** obtainable (drops, shops, crafts, boss rewards)
- **Quests** (including any custom/story quests introduced)
- **Capstone boss** + its reward
- **Level cap**
- **Events** introduced (invasions, mini-games, etc.)
- **Original lore/story beats** for the region

## Seed sketch (illustrative — lore/order/caps not final)

- **Expansion 1 — Lorencia + starter dungeons.** Intro lore, low level cap, first
  capstone boss. Boss defeat → unlocks Expansion 2.
- **Expansion 2 — Devias.** Its own lore + events, new items, capstone boss →
  unlocks Expansion 3.
- **Expansion 3 — Lost Tower.** Region lore; the **class-upgrade quest** likely
  lands here; capstone boss → unlocks the next region.
- …each subsequent region (Noria, Atlans, Dungeon, Icarus, …) is its own
  expansion with original story, cap raise, and boss gate.

All story is original — the region names are the classic MU geography (the shared
world), but the events, characters, and quest narratives are new.

## Per-expansion manifest template

Copy this block per expansion when it gets scoped:

```
### Expansion N — <region name>
- Level cap: <n>
- Maps: <map numbers / clusters>
- Original lore: <one-paragraph story premise + key beats>
- Monsters/spawns: <sets available>
- Items introduced: <drops / shop stock / crafts / boss reward>
- Quests: <story quests, custom quests, class-upgrade quest if applicable>
- Events: <invasions / mini-games introduced this expansion>
- Capstone boss: <name> — reward: <what it drops/grants>
- Gate: defeating <boss> unlocks Expansion N+1
```

---

## Wanted content systems — and where they land in the model

Owner-flagged systems that are **wanted but currently missing** (flagged
2026-07-03). None is built yet; the "where it lands" column ties each into the
expansion frame above so it gets scheduled inside a concrete release rather than
floating.

| System | Priority | Data status | Where it lands |
|---|---|---|---|
| **Quests + quest-item drops + custom quests** | Must | Greenfield — no `quests.json`; quest-bound drops (Broken Sword, Scroll of Emperor, …) noted deferred in `data/_coverage/drops.json`; NPC #235 (Sevina) already tagged `window=quest`. | Per-expansion story + custom quest lines. Schema must **not** hard-code the classic quest set. Class-upgrade quest ≈ the Lost Tower expansion. |
| **Merchant / NPC store inventories** | **Must** | Partial — `ItemDefinition.price` exists; no per-NPC stock lists. | Per-region shop stock, growing each expansion. Needs a shop-catalog data file + buy/sell service. |
| **Box of Kundun (+1..+5) & higher box tiers** | **Must** | **Blocked on schema** — the v2 `BoxDrop` shape (item-roll-else-money over a *normal*-item pool) **cannot encode an excellent-only box**; Box of Kundun contents are `ItemType=Excellent`. Golden Titan already drops box `14/11 +9` with **no contents record** (a dangling ref today). | Boss / high-tier reward boxes, expansion-gated tiers. Requires a `BoxDrop` excellent/normal discriminator **before** extracting tiers — the same fix clears the Golden Titan dangling drop. |
| **Golden Invasion (tiered) + White Wizard invasion** | **Must** | Greenfield — golden-army monsters 78–83 + their Box-of-Kundun drops are S3-era, excluded from the 0.95d baseline; no White Wizard event data. | Server-wide invasion event slotted into a mid/late expansion. Modernize freely (telegraphed, scaling tiers) — aligns with `Ideas.md` #10 (dynamic open-world events). |
| **Castle Siege → city-control siege (custom)** | Long-term (endgame) | Greenfield — authentic Castle Siege (Valley of Loren, Guardian statues, Jewel of Guardian, lvl-380 options) is an S3+/S4 gap, excluded wholesale by the current spec. | **Deliberate reimagining, not the classic castle.** A periodic siege event over the main cities (**Lorencia / Devias / Noria**); the winning guild controls the city and its **taxes/economy**. A late expansion, once multiple cities are live. Endgame social/PvP structure (`Preserve.md` #8). |

**General direction:** every expansion is a chance to **modernize**, not merely
re-implement classic MU — keep identity (`Preserve.md`), apply the modernization
lenses (`Ideas.md`) where they raise feel without touching economy/progression
nostalgia.
