## Card H-S5b — lane H: Oracle T1 under D17 and the product T0

TODO: S5-H-01, S5-E-04. Rebase on v0.2/product-loops first: D17 (plan §9) is in — zoned timestamps are compared as UTC
instants (tools/oo-test/t1.py compare_oracle). Lane T (native) is working T7 in tools/oo-test audit files in parallel —
do not edit audit.py/T7 code.
1. Rerun Oracle 23.5, 21.3 and 11.2 T1/T8 (full fixture) through `oo-test matrix --hook T1=tools/oo-test/t1.py`;
   merge the rows; regenerate the matrix (`--check`, `--self-test`).
2. Product T0: your report says T0 rows are fixture skips because no product-doctor hook exists. Add a T0 hook that runs
   the product `oo doctor` (crates/oo-cli admin doctor) against each P0 version's fixture and records pass/fail from its
   JSON (probe, version capability, grants, sync-mode readiness) — run it on every P0 version.
Report the regenerated version × capability table.
