## Card M-S4e — lane M: MySQL/Oracle publish cycles, retention, lite seven healthy

TODO: S4-M-01 (remaining exits of M-S4/M-S4c steps 3–5). Rebase on v0.2/product-loops first (your D15 activation and
live Gateway cycles 8cb940b23…028f5ae2b are integrated; evidence `m-s4d-live-16`). Same grants as M-S4b. Commit after
each step; do not stop at a milestone.
1. One MySQL 8.4.7 and one Oracle 23.5 cycle through `oo-load sync` → `oo-load publish` with the live Gateway client
   (reuse `tools/oo-load/test-gateway-cycles.py`, parameterised by engine).
2. Retention: release retained generations older than the policy only when no ready publication or in-flight query holds
   them; test with a query holding an old generation across a switch.
3. deploy/lite: provision the Gateway login's EXECUTE grant on the retained-proof helper; the scheduler runs
   `oo-load sync` → data-only Builder → `oo-load publish`; after one scheduled cycle `oo status` reports all seven
   services/jobs healthy (Builder as a completed one-shot). Live, with evidence.
4. Docs patch (proposed, in your report): D15 mapping table for docs/product/02 §6 and the lite cycle for docs/product/04.
