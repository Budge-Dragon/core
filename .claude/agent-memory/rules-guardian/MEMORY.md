# Memory Index

- [Sanctioned total-select idiom](sanctioned-total-select-idiom.md) — linear-scan + const-terminal index select is accepted, not a suppressor (W-ENT precedent; draw_cardinal in W-MOV)
- [Sanctioned Result-collapse & narrow](sanctioned-result-collapse-and-narrow.md) — Err(_) construction-collapse and TryFrom saturating unwrap_or are total folds, not banned wildcards/lookup-unwrap (W-MOV)
- [Sanctioned test-harness idioms](sanctioned-test-harness-idioms.md) — or_abort infallible-load confinement and counter-map unwrap_or(0) in core/tests are sanctioned, not suppressors (W-MOV sim suite)
- [Sanctioned xtask scanner & exemption](sanctioned-xtask-scanner-and-exemption.md) — xtask ban-scanner is dev-tooling exempt from Iron-Law lints; its direct-receiver-only lookup-unwrap_or gap is a nit not a veto (W-HARDEN)
- [W-SRC provenance-comment convention](w-src-provenance-comment-convention.md) — `// W-SRC:` inline comments are sourced-fact citations (accepted), not open-wave anchors; flag only if the claim is factually false
