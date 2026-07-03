---
name: sanctioned-result-collapse-and-narrow
description: Review precedent — Err(_) construction-collapse and TryFrom saturating-narrow are total folds, NOT the banned domain-enum wildcard or lookup-shaped unwrap_or
metadata:
  type: project
---

Two idioms in `core/src` look like suppressors but are sanctioned total folds.
Do NOT flag either in future reviews (both audited and ACCEPTED in W-MOV).

**1. `Err(_)` construction-collapse.** `match Smart::new(x) { Ok(v) => v, Err(_)
=> <valid domain fallback> }`. Examples: `movement::commit` and
`monster_ai::face_toward` fold `Facing::new(step)`'s `Err(_)` to the prior
facing (a directionless/zero step keeps facing); `spatial::WorldVec::normalized_to`
folds `NonZeroFixed::new`'s `Err(_)` to `Displacement::NoDirection`;
`units::{Level,ChancePer10000,Percent}::clamped` fold `try_from` `Err(_)` to a
saturating bound.
- This is NOT the banned `wildcard _ => on a domain enum`. The match is over
  std `Result` (exactly Ok/Err) — the exhaustiveness proof is intact; `_` only
  ignores the *error payload*. A new domain-error variant does not silently slip
  through a dispatch, because the dispatch isn't matching the error's variants.
- Legit because the fallback is a real, valid domain value (prior facing /
  no-direction / saturated bound), and the reachable error is the expected one
  (zero vector, out-of-range). Matching the specific variant explicitly would
  force handling the unreachable sibling with an `_`/panic anyway — banned.

**2. `TryFrom` saturating-narrow via `unwrap_or`.** `i128::try_from(x)
.unwrap_or(i128::MAX)` (spatial.rs `i128_from_u128_saturating`). The banned
`unwrap_or` targets **lookup-shaped** producers (`map.get(k)`, `v.first()`,
`iter().find()`) whose `Option` means a missing key. A numeric `TryFrom`
conversion is not a lookup — `Err` is overflow, and `unwrap_or(MAX)` is the
defined saturating fallback (correct here because a `u128` input is always ≥0,
so only the positive bound is possible). Sanctioned saturating narrow, not a
lookup unwrap_or.
- Minor consistency nit (non-blocking): sibling narrows `saturate_i64` /
  `tile::narrow_u8` use an explicit `match`. Fine either way; do not veto.

**How to apply:** Neither is a HARD REJECT. Only escalate if an `Err(_)` fold
lands on a fabricated `Default`/zero that papers over an absence that should be
its own enum variant, or if `unwrap_or` sits on a genuine `map.get`/`first`/`find`.
See also [[sanctioned-total-select-idiom]].
