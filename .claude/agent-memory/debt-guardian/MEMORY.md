# Debt Guardian Memory

- [v2 rebuild wave boundaries](project_v2_rebuild_wave_boundaries.md) — what R1-R4 legitimately defer (W-CMB/W-ENT formulas, R3 debt file) vs. actual debt.
- [spatial-foundation wave boundaries](project_spatial_foundation_wave_boundaries.md) — spatial Waves A+B deferrals (D1-D5, T1-T4, Q1-Q4, MOB-SPD) now ALL discharged/closed; no longer in the index.
- [W-INV inventory wave boundaries](project_winv_inventory_wave_boundaries.md) — I1/I2/I3 all CLOSED 2026-07-04 (root fixes landed); durability consts → CMB-CONST; new EXC-DROP-WINDOW row (excellent drop uses wrong level band, owner W-SRC).
- [W-CMB combat wave boundaries](project_wcmb_combat_wave_boundaries.md) — CMB-CONST (W-SRC) tracks 13 combat consts; two root-fix-now issues (bounding_rect fabricated zero, dead resistances() accessor); 6 deviations judged clean.
- [W-HARDEN ban-scanner coverage](project_wharden_ban_scanner_coverage.md) — xtask syn scanner: bans 2/3/4 airtight; ban #1 (lookup-unwrap_or) partial — evades wrapped/adapter lookups + domain accessors, manual review still backstops.
