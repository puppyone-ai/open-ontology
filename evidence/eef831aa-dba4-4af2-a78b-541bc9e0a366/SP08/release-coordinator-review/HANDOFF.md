# HANDOFF

Owner of this directory: session `eef831aa-dba4-4af2-a78b-541bc9e0a366`. Verdict on original frozen E53 `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3` is `APPROVE_LIMITED`. `verdict/VERDICT.txt` is that token.

Do not treat this packet as full SP08 acceptance, as parent-goal acceptance, or as a production certificate. External gates `credential`, `integrity`, `authenticated_owner`, `customer`, and `runsc` stay missing. `full_sp08_accepted` stays false. Acceptance counts stay unchanged. The product tree was not edited.

## Result to carry

The independent tests on a fresh `oo_a366` database, plus one new process after `docker restart`, match the frozen contract for this slice. Winner `rev-d` remains `active` at pointer `rv-d`. Activation class is `test_scope_not_production`. Status class is `not_issued`. Pack digest `349b660592d39933061064221839869545e5ddaf0d40a83bbc8d84355dfcbfd1`. Manifest `45ef429ea22edc9f6278c2b3fa8038a6813845c57c009501bb2f58f2296ec5bd`. Review source hash `85d3bdcf6a63f11868d40422868a92948c56392f28c771f4fa5e8dd79c960766`.

The crash coverage is durable `applying`, an unchanged pointer, and `recover` to `failed`. A killed backend inside the activation commit was not injected. PITR, standby, customer ownership, and public deployment were not exercised. `docs/11` publish and rollback are outside this crate.

Author packet a359 is the implementation record. Its `observation.json` stores the activation class. The status class for this review is the restart stdout.

## Raw exits

- `linux/smoke-before/docker-run.exit` = 0 (rustc 1.94.0, before the oracle existed)
- `pg/up.exit` = 0
- `linux/build-1/author-clippy.exit` = 0
- `linux/build-1/author-test.exit` = 0
- `linux/build-1/author-build.exit` = 0
- `linux/build-1/review-clippy.exit` = 0
- `linux/build-1/review-test.exit` = 0
- `linux/build-1/docker-run.exit` = 0
- `linux/restart-status/exit.txt` = 0
- `failed-attempts/build-1-clippy-too-many-arguments/review-clippy.exit` = 101
- `failed-attempts/build-1-clippy-too-many-arguments/review-test.exit` = 0
- `failed-attempts/build-1-clippy-too-many-arguments/docker-run.exit` = 1

`origin/source-inventory.txt` ends `mismatches=0`. `linux/build-1/lock-subset-check.txt` is `registry_checksums_subset_of_root=yes`.

## Cleanup

`linux/cleanup.sh` removes only `oo-a366-status`, `oo-a366-build`, `oo-a366-smoke`, `oo-a366-pg`, volumes `oo-a366-eef831aa-pgdata`, `oo-a366-cargo`, and `oo-a366-target`, network `oo-a366-eef831aa-net`, and `/tmp/oo-a366-eef831aa-dba4-4af2-a78b-541bc9e0a366`. It does not prune and it does not remove other owners. `cleanup-owned.txt` and `cleanup.exit` are the record. The database volume is removed with that cleanup, so a later reader cannot re-query the rows. The proof of the final rows is the test log, `linux/build-1/observation.json`, and `linux/restart-status/stdout.json`.

## Left open

External production issuers, full SP08, the parent goal, PITR, standby, customer ownership, public deployment, and the `docs/11` control-plane publish and rollback surfaces. Frontend work stays with a351, a353, and a352. This review did not open those trees.
