# Handoff

Verdict: APPROVE_LIMITED.

Reviewed commit: `543580768dec60d127eb428c09e0a9a400b18272`.
Author evidence: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a218/SP06/column-acl-set/`, cleanup `819bfade1766ee55c9b35fbefca4bcf3e9c09525`.
Session: `eef831aa-dba4-4af2-a78b-541bc9e0a229`.
Path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a229/SP06/column-acl-set-review/`.

`install_column_acl_set` on this commit installs one `NOLOGIN` role across two mapped tables, replays only an exact set, and leaves the old `install_column_acl` function as a one-table API. A bad second mapping and a bad second type return before `CREATE ROLE` and leave no role. A `GRANT` event-trigger fault on the fourth grant of a new role rolls that role and its grants back. `query-pg` then reads the allowed columns, `count(*)`, and star, and hidden reads and writes return `42501`.

This is the existing column-ACL component's set extension. It is not a new component and it is not a first review. M1-D14A stays `未验收a218`. SP06-W01 stays open. No acceptance line was edited.

## Evidence

- `REPORT.md`, `FINDINGS.md`, this file.
- `source/COMMIT` and `source/blobs/` are the reviewed bytes. `linux/smoke/archive-blobs.txt` matches `git hash-object` to `git rev-parse`.
- `linux/smoke/` is the pre-overlay Rust 1.94 `policy::tests` run. Exit 0. Six tests passed. The review file was not in the archive. Lock digest `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` unchanged. Clippy was deferred.
- `linux/proof/` is the green live run. Exit 0. Container `0a0d07d8b59945291672226f3d54275ec20b06adc1c5fa11cdd8400699ac2f4c`, 6 GiB, 1 CPU, capabilities dropped, no host port. Clippy `-D warnings` exit 0.
- `linux/proof/attempt-1/` and `linux/proof/attempt-2/` are the failed proof attempts. Both exited 101 before the rollback assertions. The green oracle is the later run.
- `linux/proof/postgres-fault.log` is the server line for the green fault: `P0001` on grant 4, query text naming the second table.
- `live/oracle.txt` is the measured fixture. It has no passwords and records `pre_create_role` separately from `post_grant_*`.
- `pg/` records the private server image, SAN, and capability set. Password files were only under `/tmp/oo-a229-eef831aa-dba4-4af2-a78b-541bc9e0a229/secrets`.
- `SHA256SUMS` is the Rust tool `tools/sha256.rs`, mode `dir`. `SHA256SUMS.sha256` is the hash of that manifest.
- Origin recipes and their hashes are in `origin/recipes.txt`. Harness commit `d7f640d01409b7b476f34bd14b6504f687cacc55`.

## Left open

Authentication, row-level security, publication, and the audit WAL remain open. The grantor in this fixture is superuser `oo_fixture_admin`. A non-superuser grantor is not a mandatory gate for a later pass. The login's mapping `USAGE` / `SELECT` / `EXECUTE` grant is fixture code, not an installer grant. The event-trigger payload did not carry `objid` or `object_identity`; the second-table match used `current_query()`.

## Cleanup

`cleanup/run.log` records removal after evidence commit `3511172ed93da7d9662893d9821b3bde8ea2e9eb`. Containers `oo-a229-smoke` and `oo-a229-proof` were already absent. The script removed `oo-a229-pg`, volumes `oo-a229-eef831aa-cargo`, `oo-a229-eef831aa-target`, and `oo-a229-eef831aa-pgdata`, network `oo-a229-eef831aa-net`, and `/tmp/oo-a229-eef831aa-dba4-4af2-a78b-541bc9e0a229`. Exit 0. No prune. A second check saw no container, volume, or network with this session's a229 labels. The pinned Rust and PostgreSQL images were still present.
