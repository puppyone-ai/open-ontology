# FINDINGS

Bound to product commit `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3`. A green test does not accept SP08.

## Held by the code that was tested

The live test calls `oo_builder::compile_role_pack`. The seal compiler id is `oo-builder/role-pack/v1`. Manifest `45ef429ea22edc9f6278c2b3fa8038a6813845c57c009501bb2f58f2296ec5bd` is one of the nine stored files. A visibility change changes `source_hash` and deletes the previous gate evidence. The candidate returns to `candidate_built`. An active candidate refuses a new source hash with `seal_conflict` and keeps its pack digest.

Missing, `not_run`, a second disagreeing pass, a stale fence, and a fixture certificate whose authority hash does not match the install all keep the test-scope plan at `not_ready`. Those runs were candidates B and C. They never became active.

G8 admissible evidence in this test is a local recompile: samples omitted, publication not claimed, PII database scan not performed, `production_admissible=false`. The caller's own status string is ignored. G9 admissible evidence is the fixture authority hash recorded at install. The remaining G1–G13 rows in the live test are `test_issuer` reports. A `test_issuer` row whose gate id is `credential` stores `credential_verified=false`.

Production plan and production activate on a candidate that already had every test-scope gate admissible still returned `production_blocked`. The pointer stayed null. The pure helper that maps an empty reason list to `ready` is covered by `an_empty_production_list_is_ready_and_a_named_gap_is_not`. The live production path on candidate A was not that empty list. It named `credential_missing` and `fence_not_production_admissible`.

Activation of D committed only after D was `gates_passed`, then `applying`, with the old pointer still null through the applying status. The winner receipt is `test_scope_not_production`. The loser became `failed` with pointer `ver-d` and `half_published=false`. Replay of D returned the same audit id. Rollback moved the pointer back to inactive D and left both pack hashes unchanged. A second rollback replayed. Abort and retire replay in their target states. Abort of the active candidate is `seal_conflict`.

The reader view showed `ver-d` and false values for `production_ready_certificate`, `full_sp08_accepted`, `public_deployment_claimed`, `customer_ownership_claimed`, and `standby_configured`. The reader cannot update the pointer and cannot select the candidate table (`42501`).

After `docker restart` of `oo-a359-pg`, a new CLI process reported D still active, `half_published=false`, `certificate_class=not_issued`, `production_blocked=true`, and empty `test_reasons`. The 21 production reasons include every G gate as not production-admissible, plus credential, integrity, authenticated owner, customer, runsc, fence, and replica. `sessions_terminated_by_tool` is false.

`product_mode_can_activate: true` is rejected in `cli::tests::waiver_and_product_mode_do_not_write` before a database write. Top-level waiver keys are recognized in `canon::tests::waiver_keys_are_recognized`.

## What this slice leaves false

These flags are false on the final status and were false on the activation path's standing claims:

- `production_ready_certificate`
- `owner_authenticated`
- `credential_verified`
- `integrity_verified`
- `customer_gate_verified`
- `runsc_verified`
- `full_sp08_accepted`
- `public_deployment_claimed`
- `customer_ownership_claimed`
- `standby_configured`

There is no PITR proof, no standby setup, no customer ownership transfer, and no public deployment. The ops surface in docs/11, `oo ontology publish` and `oo ontology rollback`, is not this crate's CLI. This packet does not accept SP08 or the parent goal.

`certificate_class` on the activation receipt is `test_scope_not_production`. `certificate_class` on status, including the post-restart CLI status, is `not_issued`. Test mode did not emit a production-ready certificate.

## Limits of this proof

The crash window is the durable `applying` state with the pointer untouched, then `recover` to `failed`. The test did not kill a PostgreSQL backend between the two commits. `StoreError::CommitUnknown` was not fault-injected.

The restart was a host `docker restart` plus one new CLI container. The test process did not receive `docker.sock`. The ready check slept at most 12 times, 5 seconds apart, until `pg_isready` succeeded. That is the harness, not an 18-minute observation loop.

Three schema drops of `oo_release_control` happened before the final install. They are recorded under `pg/schema-reset-after-*.txt`. Roles and the volume stayed until the owned cleanup after the final status. The volume is now removed, so a later reader cannot re-query those rows. The proof of the final rows is the test log, `observation.json`, and `linux/restart-status/stdout.json`.

`linux/build-1/console-tests-passed.log` passed 13 tests and then exited 1 because the package binary was missing. The pointer-label assertion and the final `BUILD_OK` are the later `linux/build-1/console.log`. `stdout-before-self-pointer.json` is the status from before that label fix. It is kept. It is not the final status.

Clippy is not in the pinned image for toolchain 1.94.0. The final container downloaded the component, then set `CARGO_NET_OFFLINE=true`. The download is not an image pull. `--pull never` was set. The image id matches the pinned digest.

The child `Cargo.lock` is the lock `cargo fetch --locked` accepted after an edge rewrite. Its registry checksums are a subset of the root lock. A generated lock that selected newer checksums was rejected and kept as `Cargo.lock.rejected`. Root manifests were not edited. `cargo update` was not used to refresh the root lock.

The first smoke exited 0 without running `rustc`. The toolchain proof is `linux/smoke-attempt-2`.

The unit decision allows an empty production reason list to be `ready`. This slice still does not mint a production certificate, and the ordinary production workflow in the private database stayed blocked.
