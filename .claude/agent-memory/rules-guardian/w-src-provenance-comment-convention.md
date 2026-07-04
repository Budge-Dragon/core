---
name: w-src-provenance-comment-convention
description: Review precedent — `// W-SRC:` inline comments are sourced-fact provenance citations (accepted), not open-wave anchors subject to the delete-at-closure rule
metadata:
  type: project
---

`// W-SRC: ...` inline comments are the crate's house-style **provenance
citation** for a magic constant or an invented/sourced-absent domain fact — they
record the *why* (the source the value came from), which Rule 13 permits as a
non-obvious hidden constraint. Established across the codebase:
`services/{experience,skills,loot,combat,item_roll}.rs` (e.g. combat.rs:114
"1.2× max — CONFIRMED against MuEmu 0.97k", item_roll.rs:84 "50% skill chance
(facts 5:44)", item_roll.rs:163 "jewelry grants no luck (facts 2:32)").

**Why:** They are NOT the "open-wave anchor" comments (`// W6h widens this`) that
Rule 13's narrow exception says must be deleted at wave closure. A W-SRC tag marks
the *kind* of comment (a source-verification citation); the load-bearing content
is the citation itself, which stays valid regardless of the W-SRC wave's status.
Citing a source for an otherwise-unexplained constant is exactly the permitted
non-obvious *why*.

**How to apply:** Do NOT flag `// W-SRC:` comments as closed-wave rot or as
"explaining what". Only flag a W-SRC comment if its *claim is factually wrong*
(see the W-INV I3 case: item_roll.rs:110-112's non-W-SRC comment "an
excellent-rarity drop on a kind with no excellent set is not producible" is
contradicted by debt record I3, which proves `loot::item_drop` does produce it —
that comment should be corrected; SUGGESTION, non-blocking since the code is a
total fold and the root is tracked).
