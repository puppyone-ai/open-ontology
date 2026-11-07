## Card E-S5g — lane E: DM8 SourceEngine on arm64; Db2 T0 and C5

TODO: S5-E-03. Rebase first: Db2 is now registered in `oo doctor` and the matrix (coordinator). Commit trailers:
`Plan:` only — no Co-Authored-By. Commit after each step.
1. Db2: product T0 through `oo doctor` via the lane-H T0 hook (`tools/oo-test/t0.py`), and three C5 cycles through
   `oo-load --c5` with explicit verdict rows.
2. DM8 (owner-accepted license, native arm64 image you built): SourceEngine for DM8 (feature-gated module; DM8 speaks
   an Oracle-like dialect — reuse what fits; driver via the thick-worker pattern with DM's own client if no acceptable
   Rust driver exists, cargo deny/audit for any crate), oo-load dispatch, matrix entry, T0, T1 (Agent DB comparison,
   concurrent writer), T8 with explicit verdict rows.
