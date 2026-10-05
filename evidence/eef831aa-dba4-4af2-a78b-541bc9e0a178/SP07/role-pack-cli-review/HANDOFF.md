# Handoff

Independent review of the role-pack command on `af3308c32fa9e90bb29fab970b691d57dbc35279`, tree `ef9b913640afd14cb87c9e79baaa321ebb205560`, parent `44153c488720cd290e9363763c60b0830717a9d6`. Fixed library base `eff1959bf028a65b20b7fca855d79c23e4b188fd`, tree `2f08d4ecef7ad2f3b617e8a04afcb45f0cf0186f`. `role_pack.rs` blob `1327bad464fe8f510cc7baf8566ad71e3d00df2f`. `Cargo.lock` blob `8364c36f2c70919cd3a1df18a2e57698a44051a8`.

Verdict: **APPROVE LIMITED** the bounded `oo-builder role-pack` command. The requirement record is `REPORT.md`. The line-level notes are `FINDINGS.md`. The oracle description is `ORACLE.md`. Author evidence `1c71e5a7` is not this proof. No product file was edited. The development ledger was not edited. Nothing was pushed.

The deciding oracle is `linux/probe-2/out/outcome.json`. Clerk legal SQL is `linux/probe-2/out/clerk/files/metrics.md` line 16. Clerk row security is `tables/yard.bale.md` lines 7–8. Pre-overlay smoke is `linux/smoke/`. The failed harness run is `linux/probe/`: clippy rejected a constant assert in the overlay. Probe and probe-2 share clerk digest `0adc296da6c6c882d4886792862b7eece542870e53d3673f3804f64a0a153e76`. Do not read the probe clippy exit as a product failure.

Owned product paths in `af3308c3`, unchanged by this review:

- `crates/oo-builder/src/bin/oo-builder.rs`
- `crates/oo-builder/src/lib.rs`
- `crates/oo-builder/src/role_pack_cli.rs`
- `crates/oo-builder/tests/role_pack_cli.rs`
- `contracts/builder/role-pack-cli-v1.md`

`source/blobs.sha256` lists content hashes and git blob ids for those paths, the unchanged compiler, the lock, and the contracts. `MANIFEST.sha256` lists every file in this directory except itself.

No required change remains for this command. The command forwards `compile_role_pack`, keeps the manifest statuses, and does not echo the body or an extra argument. `structure-draft`, `captured-pg-draft`, and `review-pg-draft` still pass their existing suites.

Domains, the tokenizer, the read API, search, filesystem mount, authentication, the policy adapter, a database re-proof of row security, and publication gates G1–G13 stay open. This is not SP07-W06 approval. A visibility snapshot is not authentication and not a grant. A successful command is not SQL execution.

The overlay under `overlay/` was applied only to a copy of the git archive. `Cargo.lock` was not edited. Slot B and PostgreSQL were not used.

`CLEANUP.txt` records removal of this task's network, cargo volume, target volume, and `/tmp` tree. The builder containers were removed by their launch scripts. The pinned Rust image was left in place. `docker prune` was not run.
