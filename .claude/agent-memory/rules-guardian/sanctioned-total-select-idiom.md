---
name: sanctioned-total-select-idiom
description: Review precedent — the linear-scan-with-const-terminal index-select idiom is an accepted total form, not a disguised suppressor
metadata:
  type: project
---

The "pick element N from a fixed array by a drawn index" idiom in this crate is a
linear scan that returns the matched element, ending in a genuine domain-value
terminal (e.g. `Facing::POS_X`, `list.first()`), NOT `unreachable!`/`unwrap`/slice
indexing. Seen in `services/chance::{draw_cardinal, pick_one, weighted_pick}`.
(W-MOV consolidated the former `spawn::draw_facing` into `chance::draw_cardinal`,
now the single shared cardinal-heading draw for both spawn-without-authored-facing
and monster wander drift — spawn.rs and monster_ai.rs both import it.)

**Why:** Selecting by a runtime-bounded index without `v[i]` (banned indexing) or
`.get().unwrap()` (banned unwrap) forces a scan; Rust can't prove the loop always
returns, so a terminal expression is structurally required. Resolving it with a
real, valid domain value (a genuine cardinal / the proven-present head) is total
and honest — the terminal is unreachable given the bound but tells no lie and
asserts nothing. Reviewed and ACCEPTED for wave W-ENT (draw_facing terminal
explicitly audited).

**How to apply:** Do NOT flag this terminal as a fabricated `Default` or disguised
suppressor in future reviews. It passes Rule 3/Rule 4. The banned alternatives it
replaces are `CARDINALS[target]`, `.get(target).unwrap()`, wildcard-arm match, and
`unreachable!()`. Only flag if the terminal is a zero/`Default::default()` papering
over a real absence that should be an enum variant.

**W-INV instances (2026-07-04, reviewed ACCEPT):** the idiom also covers a
fallible-constructor terminal, not just index-select. `item_instance::slot_bit`
does `match NonZeroU8::new(1u8 << slot_index.saturating_sub(1)) { Some(bit)=>bit,
None => NonZeroU8::MIN }` — the `None` arm is a valid domain value (bit for slot
1), and `slot_index()` is a `const fn` provably `1..=6` (item_options.rs), so the
shift is `0..=5` and never overflows/panics. Same shape as `draw_option_level`
(terminal `OptionLevel::L1`) and `draw_ancient_bonus` (terminal
`AncientBonusLevel::One`) in `services/item_roll`. The doc-comment phrase
"unreachable zero input, never a panic" is honest idiom documentation, NOT a
banned "should never happen" branch (no assert/unwrap/panic). Passes.
