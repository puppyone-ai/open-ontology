# FINDINGS

Bound to product commit `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3`. Verdict `APPROVE_LIMITED`. A green private run leaves external gates, full SP08, and the parent goal open.

## Held on the original candidate

The review copy calls the crate's `compile_seal`, which calls `oo_builder::compile_role_pack`. The stored compiler id is `oo-builder/role-pack/v1`. File text is not the stored seal. The test hashes each pack file with `sha2` and compares those digests to the seal. Manifest `45ef429ea22edc9f6278c2b3fa8038a6813845c57c009501bb2f58f2296ec5bd` is one of the nine stored files. Wide and narrow visibility produce different source hashes and different pack digests. An active candidate refuses a new source hash with `seal_conflict` and keeps the pack digest.

Missing G4, `not_run` G4, a stale G1, a foreign G3, a stale fence, a second G4 beside `not_run`, and two fresh fences each leave the candidate at `candidate_built` with a null pointer. The plan reasons are `G4_missing`, `G4_not_run`, `G3_foreign_receipt`, `stale_fence`, `G4_conflict`, and `conflict`.

G8 with caller status `fail` or `not_run` is still `admissible` when the recompile matches. Detail is `role_pack_samples_omitted`. `pii_database_scan` is `not_performed`. `production_admissible` is false. G9 admissible evidence is the installed fixture authority hash. The other G rows in this live proof are `test_issuer` reports.

A checkpoint with `sessions_terminated=true` is stored as `fabricated_session`. `sessions_terminated_by_tool` stays false.

Production plan and production activate on a fully test-armed candidate return `production_blocked`, including `credential_missing` and `fence_not_production_admissible`. The pointer stays null. The candidate stays `candidate_built` until a test-scope plan.

`enter_applying` sets `applying` and leaves the pointer null, with `half_published=false`. `recover` from `applying` sets `failed` and leaves the pointer null. A second `recover` replays. A later test plan returns `gates_passed`.

Two candidates that both expect a null base both enter `applying`. The winner commit sets the pointer, writes audit events `replica_ready` and `activated`, and uses `certificate_class=test_scope_not_production`. The loser commit returns `cas_conflict`, state `failed`, and the pointer stays on the winner. A second activate replays the same audit id. Rollback of a later candidate restores the previous pointer and the previous active row in one outcome, leaves both pack digests, and uses `certificate_class=not_issued`. A repeat rollback replays.

Explicit `test_issuer` rows named `credential`, `integrity`, `authenticated_owner`, `customer`, and `runsc` keep `credential_verified`, `integrity_verified`, `owner_authenticated`, `customer_gate_verified`, and `runsc_verified` false. Their `certificate_class` is `not_issued`.

`oo_release_reader_login` reads `publication_status` at `rv-d` with `production_ready_certificate`, `full_sp08_accepted`, `public_deployment_claimed`, `customer_ownership_claimed`, and `standby_configured` false. `UPDATE` on `active_pointer` and `SELECT` on `candidate` return SQLSTATE `42501`. A status call with CA bytes `not-a-certificate` returns `StoreError::Tls`.

After `docker restart` of `oo-a366-pg`, a new process running the archive binary reports `rev-d` `active` at `rv-d`, `half_published=false`, `certificate_class=not_issued`, `production_blocked=true`, and empty `test_reasons`. The production reason list names every G gate as not production-admissible, then the five external names, the fence, and the replica.

`product_mode_can_activate: true`, `all_accepted`, and `allaccepted` exit 1 with empty stdout before a database write. That is the required narrow boundary check.

## What stays false

On the post-restart status:

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
- `sessions_terminated_by_tool`

`test_mode_cannot_produce_production_certificate` is true. Activation class on the winner receipt is `test_scope_not_production`. Status class, including the new process, is `not_issued`.

There is no PITR proof, no standby, no customer ownership transfer, and no public deployment in this packet. `docs/11-deployment-and-operations.md` covers one-click start, Compose, the `oo ontology` control plane, upgrade, backup, observability, runbook, Kubernetes, and object storage. Those surfaces are outside this crate. The author packet a359 records the same open boundary. This review does not close it and does not change acceptance counts.

## Limits of the proof

The crash window covered here is the durable `applying` row, the untouched pointer, and `recover` to `failed`. The tests did not kill a PostgreSQL backend inside the activation commit. `StoreError::CommitUnknown` was not injected.

`replica_ready` is an audit event in the activation transaction. It is not a durable state by itself.

The production path in this slice has no production issuer. The unit helper that maps an empty production list to `ready` is the author's `decide` test, which passed again on the archive. The live production plan and activate on `rev-a` were not that empty list.

The reader check is one view read, two denied statements, and one bad CA. It is not a broader privilege campaign.

The first review Clippy exit was 101 because the review helper took 8 arguments. The 13 tests had already passed on that source. The helper was reshaped and both schemas were dropped before the accepted run. The accepted exits are author clippy 0, author tests 0, author build 0, review clippy 0, review tests 0, docker 0, restart status 0. The Clippy failure is a review-harness defect, recorded under `failed-attempts/`, and the product tree was not edited to clear it.

Clippy is not in the pinned image for toolchain 1.94.0. The builder downloaded that component, then compiled and tested offline. The image was not pulled. `--pull never` was set. The image id matches the pinned digest.

The synthetic workspace manifest exists only in the builder tree. Product blobs match `9d5df65` (`mismatches=0`). Root `Cargo.toml` and root `Cargo.lock` were not edited.

The author observation in a359 stores the activation certificate class. This review's `observation.json` stores both classes. The fresh-process proof is `linux/restart-status/stdout.json`, class `not_issued`.
