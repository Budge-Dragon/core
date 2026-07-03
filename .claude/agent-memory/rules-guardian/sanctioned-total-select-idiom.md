---
name: sanctioned-total-select-idiom
description: Review precedent — the linear-scan-with-const-terminal index-select idiom is an accepted total form, not a disguised suppressor
metadata:
  type: project
---

The "pick element N from a fixed array by a drawn index" idiom in this crate is a
linear scan that returns the matched element, ending in a genuine domain-value
terminal (e.g. `Facing::POS_X`, `list.first()`), NOT `unreachable!`/`unwrap`/slice
indexing. Seen in `services/spawn::draw_facing` and `services/chance::{pick_one,
weighted_pick}`.

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
