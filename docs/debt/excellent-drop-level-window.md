# Debt record: excellent drop draws the wrong monster-level window (`EXC-DROP-WINDOW`)

- **ID:** EXC-DROP-WINDOW (loot-mechanic authenticity)
- **Status:** OPEN
- **Owner wave:** W-SRC (excellent-drop authenticity pass)
- **Created:** 2026-07-04, surfaced by canon-guardian during the I3 review.
- **Category:** code (behavioural authenticity)
- **Severity:** medium
- **Location:** `core/src/services/loot.rs:100-114` (`item_drop`, the single
  rarity-agnostic `[monster_level - 11, monster_level]` window built from
  `DROP_LEVEL_WINDOW_GAP`). Related constant: `EXCELLENT_DROP_LEVEL_BONUS = 25`
  already exists at `core/src/services/item_rules.rs:171`, but is consumed only by
  `effective_drop_level` (a per-item drop-level surcharge), **not** by the loot
  window.

## Pre-existing, not introduced by I1–I3

This is W-CMB loot code. The I3 fix (excellent-capability pool filter) touched the
same `item_drop` function but is orthogonal: I3 made the excellent pool
*capability*-correct (no excellent set stamped on a kind that has none), while this
gap is about the *level band* the pool is drawn from. Filed on its own row so it is
not mistaken for I1–I3 fallout and does not muddy the closed W-INV record. **Do not
attribute to W-INV.**

## Symptom

`item_drop` builds one window `[monster_level - 11, monster_level]` and draws the
Excellent pool from it, identically to the Normal pool. Authentic classic behaviour
(OpenMU; facts doc 2:75,142 + 5:46,168) requires an excellent drop to:

- **(a) gate** on `monster_level >= ExcellentItemDropLevelDelta (= 25)` — a monster
  below level 25 drops no excellent item at all; and
- **(b) pool** at `(monster_level - 25)` — the excellent candidate band is computed
  25 levels below the monster, not the same `-11` Normal band.

mu-core does neither. After the I3 fix the excellent drop is capability-correct but
still pulls from the wrong level band, and can be produced by sub-25 monsters.

## Root cause

The excellent-drop level rule is a **distinct mechanic that was never modelled** in
the loot service — not a mis-valued constant. `item_drop` treats the drop window as
rarity-agnostic: it computes one `Interval` from `DROP_LEVEL_WINDOW_GAP` and reuses
it for every category. The excellent path has no gate and no separate window
derivation, so the authentic `>= 25` gate and the `level - 25` band are simply
absent. The value `25` (`EXCELLENT_DROP_LEVEL_BONUS`) exists but is wired to a
different formula (`effective_drop_level`), so the mechanic looks "present" at a
grep but is not applied to the draw.

## Distinct from CMB-CONST

[combat-constants-provenance.md](combat-constants-provenance.md) tracks the
*provenance of the value 25* (its verification table lists "Excellent-drop delta 25"
as ACCEPTED-as-OpenMU pending W-SRC). That is a "is this number authentic?" question
with a confirm-or-correct discharge. This record is the *absence of the behaviour* —
the gate and the distinct window are unimplemented in `loot::item_drop`. Folding a
"implement this rule" item into a value-verification table would mislabel the work
and blur CMB-CONST's discharge criterion, so it is filed separately. The two are
cross-referenced: CMB-CONST owns the number, this record owns the mechanic.

## Why not fixed now

The I1–I3 wave closed the W-INV follow-ups; this is a separate W-CMB loot-mechanic
gap on the W-SRC authenticity track (the same pass that owns `DROP_LEVEL_WINDOW_GAP`
and the drop constants). It is being recorded, not patched, so the fix lands as a
scoped whole (gate + separate window derivation + tests) rather than a bolt-on to a
closing wave. The shipped code is honest at its own boundary — the window is a real,
total `Interval`, no suppressor, no fabricated default — it is just narrower in
authenticity than the classic rule.

## Resolution plan (W-SRC excellent-drop pass)

1. In `loot::item_drop`, when `rarity == ItemRarity::Excellent`, gate the whole draw
   on `monster_level >= EXCELLENT_DROP_LEVEL_BONUS (25)` — below the gate the
   excellent category resolves to `Drop::Nothing` (a real bucket, matched before any
   table is built, never a panic), consistent with the existing empty-window path.
2. Derive the excellent pool window at `(monster_level - 25)` rather than
   `(monster_level - DROP_LEVEL_WINDOW_GAP)`. Model the window derivation so the
   band is a function of rarity instead of a single rarity-agnostic `Interval`
   (candidate: a `drop_window(rarity, monster_level) -> Interval` producer, or fold
   the excellent case into `DropCategory::Excellent` so the two windows are distinct
   by construction — a missing-variant reshape, not an `if` bolt-on).
3. Share the single `EXCELLENT_DROP_LEVEL_BONUS = 25` constant from
   `item_rules.rs` (do not mint a second `25`); confirm the gate/delta value against
   an authentic classic source as part of the same W-SRC pass that clears the
   CMB-CONST "delta 25" row.
4. Add a `core/tests/` contract over the real `/data`: a sub-25 monster yields no
   excellent drop, and an excellent drop's candidate band is centred on
   `monster_level - 25`.

## Discharge

Discharged when `item_drop` gates the excellent category on `monster_level >= 25`
and draws its pool from the `monster_level - 25` band (value confirmed against an
authentic source), with a `core/tests/` contract proving both. Remove
`EXC-DROP-WINDOW` from `DEBT-INDEX.md` at that point and clear the linked CMB-CONST
"delta 25" provenance row in the same pass.
