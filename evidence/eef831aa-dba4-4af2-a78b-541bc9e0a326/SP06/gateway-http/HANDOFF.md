# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a326`. Read `CONTRACT.md`, `REPORT.md`, and `FINDINGS.md`.

The loopback HTTP consumer is in commit `51d08e59ddb3c92fc0b9f2958966b56e19032b7f`. `linux/proof-3/` is the immutable `git archive` of that commit: library tests, eight loopback tests, Clippy `-D warnings` for the whole `oo-gateway` package, and one private-PostgreSQL test all exited 0. `inside_ok=2026-10-07T13:15:24Z`. Container `eaf43e20d755`. Earlier red attempts remain in `linux/smoke/`, `linux/compile/`, `linux/compile-2/`, `linux/compile-3/`, `linux/proof/`, and `linux/proof-2/`.

The root lock gained axum 0.8.9, axum-core 0.5.6, httpdate 1.0.3, matchit 0.8.4, and mime 0.3.17. Existing checksums stayed. Hyper kept `h2` and `want` and added `httpdate`. The a319 verdict remains the baseline approval. It is not a release, and this handoff is not one either.

The binary serves loopback HTTP only. Identity comes from the hashed bearer and the accepted token store. `POST /v1/sql` is 501. External mTLS, OAuth, and an IdP are absent. A later card that wants a public gateway, runsc, or SQL execution needs its own frozen unit.

`source/proof-51d08e59/` holds the proved sources. `source/blobs/` is the earlier snapshot and was not overwritten. `CLEANUP.txt` records removal of `oo-a326-pg`, volumes `oo-a326-eef831aa-cargo`, `oo-a326-eef831aa-pgdata`, and `oo-a326-eef831aa-target`, network `oo-a326-eef831aa-net`, and the host directory that held the passwords and CA key. No prune. The decision file `execution/decisions/gateway-http-dependency-baseline.md` was left `UNAPPROVED`.
