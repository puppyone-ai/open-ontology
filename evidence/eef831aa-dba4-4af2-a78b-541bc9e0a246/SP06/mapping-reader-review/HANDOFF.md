# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a246`. Fixed commit `1f1028f6161783d6e3bf1e9beb3a23a1161deca0`.

Verdict: **APPROVE_LIMITED**.

`parent_acceptance` is false. This is not a completed parent goal and not a release. SP06-W01 stays open. M1-D14E is not accepted by this review.

## What was reviewed

`oo_gateway::mapping_reader_access::provision_mapping_reader` at the fixed commit. The archive was that commit. The only file added for the proof was `overlay/mapping_reader_access_a246_review.rs`, and it was copied into the private archive after smoke. It is not a product change.

The live path was public `init-pg`, public `import-pg`, `install_column_acl_set`, then `provision_mapping_reader`. No fixture `GRANT` supplied the mapping privileges. Before the call, catalog load returned 42501. After `Installed`, catalog load and one typed customer query succeeded, hidden column and `SELECT *` returned 42501, and the second call was `Replay`.

## Limits that stay open

Authentication of the administrator and of the login. A non-superuser grantor. The audit WAL, row-level security, and publication. Tenant isolation: the login read all 14 mapping rows, including `private_note` and `ratio`, which are outside the clerk policy. The refused partial `USAGE` on `oo_a246_extra` was left in place. The query-pg binary was not the subject of this proof.

## Proof

Smoke of the fixed archive, before the review file was copied, exit 0. Final overlay exit 0 on Rust 1.94.0 and PostgreSQL 17.10. Oracle: `live/oracle.txt`. Outcome: `live/outcome.json`. Failed harness compile: `attempt-1/`. Clippy `-D warnings` exit 0.

## Cleanup

`cleanup/run.sh` removes only `oo-a246-smoke`, `oo-a246-proof`, `oo-a246-pg`, `oo-a246-hash`, the a246 cargo, target, and pgdata volumes, network `oo-a246-eef831aa-net`, and `/tmp/oo-a246-eef831aa-dba4-4af2-a78b-541bc9e0a246`. It does not prune and does not touch another owner. `CLEANUP.txt` records that run. The inventory container writes `SHA256SUMS` and is removed after that write.
