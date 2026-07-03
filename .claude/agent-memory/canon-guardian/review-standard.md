---
name: review-standard
description: How the user wants canon review delivered on mu-core — adversarial, substantiated, no praise-as-filler
metadata:
  type: feedback
---

Deliver canon review adversarially: assume the code is NOT good enough and try to prove it. Praise is worthless unless substantiated.

**Why:** The user explicitly frames these reviews as adversarial and says "only substantiated strengths and substantiated weaknesses count." The global CLAUDE.md also bans agreeable/confirmational filler.

**How to apply:** Every finding must name the exact file + type/function + the concrete defect + the canonical alternative (cite the authoritative source, e.g. Rust API Guidelines C-GOOD-ERR / C-STRUCT-BOUNDS, Knuth TAOCP cumulative tables, Lemire). When a fix claim is load-bearing, substantiate it by compiling a standalone reproduction in the scratchpad rather than asserting. Strengths are allowed but only when specific and provably canon-correct. Grade honestly; the code here is genuinely high quality, so real findings are usually minor/major, not blockers.
