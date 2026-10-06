# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a252`. Fixed commit `9df074813572569950d85bbe951069b4db83facc`.

Verdict: **APPROVE_LIMITED**.

`parent_acceptance` is false. This is not a completed parent goal and not a release. M1-D14F stays open.

## What was reviewed

`provision-mapping-reader` at the fixed commit. The archive was that commit. No review file was copied onto it. The a249 nested review is the author's same session and is not this verdict.

The live path was public `init-pg`, public `import-pg`, `install_column_acl_set`, then the product command. No fixture `GRANT` supplied the mapping privileges. Before the command, catalog load returned 42501. After `installed`, catalog load and the compiled clerk and peek queries succeeded, and the later calls were `replay`.

## Limits that stay open

Authentication of the administrator and of the login. A non-superuser grantor. The audit WAL, row-level security, and publication. Tenant isolation: the login receives table `SELECT` on `_oo.physical_mappings`, so it can read mapping rows outside the policy set, including `private_note` and `ratio`. This proof records the column-privilege listing and does not itself select those rows. The refused partial `USAGE` on `oo_a249_extra` was left in place. The `query-pg` binary was not the subject of this proof.

## Proof

Smoke of the fixed archive, before any live run, exit 0. Final proof exit 0 on Rust 1.94.0 and PostgreSQL 17.10. Oracle: `live/oracle.txt`. Outcome: `live/outcome.json`. Clippy `-D warnings` exit 0. No failed attempt.

## Cleanup

`cleanup/run.sh` removes only `oo-a252-smoke`, `oo-a252-proof`, `oo-a252-pg`, `oo-a252-hash`, the a252 cargo, target, and pgdata volumes, network `oo-a252-eef831aa-net`, and `/tmp/oo-a252-eef831aa-dba4-4af2-a78b-541bc9e0a252`. It does not prune and does not touch another owner. `cleanup/CLEANUP.txt` records that run. The inventory container writes `SHA256SUMS` and is removed after that write. The inventory has 167 files.
