## Card E-S1b — lane E: Oracle T1/T8 rerun with paced probes

TODO: S1-E-01. Rebase on v0.2/product-loops first: 7106fa2c6 paces oo-load probes against the read duty budget (your
proposal). Re-run `OO_TEST_LANE=e python3 tools/oo-test/t1.py --engine oracle --versions 23,21,11 --run-id ...` with the
same thick-worker image, all three versions on the same binary. Exit: T1 and T8 pass on all three (duty ≤ 25% in every
phase). If 21c still exceeds the budget, find the unpaced source reads (planning, digest, metadata) and pace them in
the Oracle engine (your scope) — never raise the budget. Commit as you go; report per version and phase.
