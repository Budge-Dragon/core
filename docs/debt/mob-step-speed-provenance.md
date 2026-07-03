# Debt record: invented monster per-step distance (`MOB_STEP_SPEED`)

- **ID:** MOB-SPD (data provenance — in-code constant)
- **Status:** OPEN
- **Owner wave:** W-SRC (source-verification pass)
- **Created:** 2026-07-03, during the movement / flight wave (W-MOV).
- **Category:** code (provenance)
- **Severity:** medium
- **Location:** `core/src/services/monster_ai.rs:30` —
  `const MOB_STEP_SPEED: Fixed = Fixed::from_raw(UNITS_PER_TILE)`.

## Symptom

A monster's per-action step distance is fixed at one whole tile by an in-code
constant. No `/data/*.json` field carries a monster step distance: `move_range`
is the territory (leash) radius, and `move_delay_ms` is the action cadence —
neither is a movement grain. The value is therefore authored in Rust, and its
own doc comment names it "an invented movement grain, held here as the single
source of truth."

## Root cause

Classic MU per-step movement distance is unmodeled in the v2 dataset. The value
is a code-side default that has not been diffed against an authentic classic
source. This is the **same species** as the `review`-flagged values tracked in
[openmu-default-values.md](openmu-default-values.md) (W-SRC) — a shipped default
whose provenance is doubtful — but it lives in Rust rather than a data file, so
W-SRC's data-file `review`-string scan would not surface it. Recorded here so it
is discoverable in the same source-verification pass.

## Why not fixed now

W-SRC (the source-verification pass) is not running, and there is no
authentic-source diff available for a monster per-step distance in this wave.
The value also has no dataset field that could carry a `review` provenance flag,
so the flag has to live as a tracked debt row instead. The shipped code is
clean: the constant is correctly typed (`Fixed`), correctly placed (single
source of truth in the movement decision service), and carries no forbidden
phrase — the doc comment states the provenance plainly without "for now" /
"TODO" / "temporary" language.

## Resolution plan (W-SRC)

1. Confirm the classic monster per-move step distance against an authentic
   source. Tile-grid movement makes **one tile per move action** the expected
   answer, so this is likely a confirm-and-keep, not a correction.
2. If confirmed: keep `MOB_STEP_SPEED` and record the confirmation here (a
   shipped default with a standing, recorded flag is acceptable).
3. If the authentic grain differs: correct the constant at
   `services/monster_ai.rs` and re-run `cargo test -p mu-core`.

Kept **out of** [openmu-default-values.md](openmu-default-values.md) so that
record's per-file accounting (185 `review` strings across 13 data files) stays
exact; this in-code constant is cross-referenced to the same W-SRC wave instead
of folded into the data-file scope.

## Discharge

Debt is discharged when the value is either confirmed against an authentic
classic source (kept, with the confirmation recorded here) or corrected at the
extractor-equivalent site (`services/monster_ai.rs`). Remove MOB-SPD from
`DEBT-INDEX.md` at that point.
