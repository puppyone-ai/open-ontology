# Handoff

Independent review is next. This file is the author record for session `eef831aa-dba4-4af2-a78b-541bc9e0a190`.

## Commit

Source: `78a60fdeabcada0d024fc6f9a81e0e6b184260e8`

```
gateway: load a PolicyCatalog from PostgreSQL
```

Paths in that commit:

- `crates/oo-gateway/src/policy.rs` (module declaration only)
- `crates/oo-gateway/src/policy_catalog.rs`
- `crates/oo-gateway/tests/policy_catalog_pg_live.rs`
- `contracts/gateway/policy-catalog-v1.md`

Blob SHA-256, also in `source/sha256.txt` and `focused/source-sha256.txt`:

```
5e9d4ea1df97559bda5aeb11f133b7c592bbebe88883124722eb80180ae71c37  policy.rs
2920211ec2278d7d74a8022cdad166a11cf97b3eea3d258bb8cf91c131b0bd9d  policy_catalog.rs
e8dcd30f11a3e58a3a5c5a5f7df213add276566204331f884f8e79e6c2fc90b3  policy_catalog_pg_live.rs
43bea6541f1cc99e5d8163ae6d7fff0137e46246fe43e78e876148222fd59c4e  policy-catalog-v1.md
```

`source/` is `git show` of that commit. Fixed base for the build was `3a251deacd7e86fc3fbe0855be3e1f6ec976c9d6`.

## What a reviewer can treat as shown

Linux arm64 Rust 1.94.0, image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL 17.10, image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`.

`linux/launch.exit` 0 is the archive smoke: 10 existing lib tests, no catalog file. `focused/launch.exit` 0 is the overlay proof: 14 lib tests, the gated integration test, Clippy `-D warnings` on those targets, and one live run. `live/oracle.txt` is the fresh fixture. `focused/attempt-1` through `attempt-4` are the failed runs.

The live path was install column ACL, `load_policy_catalog`, `guard_select`, then `DedicatedSession::execute_trusted_select` on the archive executor. Two roles saw different columns. Hidden columns, a new column, and a write were refused by the guard and, on a direct `SET ROLE`, by PostgreSQL with `42501`. Stale generation, a bad digest, a mismatched identity, and a missing role returned before user-table scans. `count(*)`, int2, int4, int8, text, bool, null, and a visible star ran. Operator and function OIDs in the catalog matched a second `pg_catalog` query. `count(any)` OID 2147 was not admitted.

## What stays open

SP06-W01 and SP06-W02 stay open. Do not mark the parent plan done. Do not claim a release.

Still open: administrator and login authentication, the audit WAL, row-level security, the public worker, the connection pool, and the rest of the R4 whitelist (arithmetic, patterns, `lower`, `upper`, `coalesce`, hashes, `date_trunc`, and `count` of a value). A non-superuser grantor was not part of this fixture. Frozen SQL was not changed.

## Rebuild notes

The focused script is `focused/launch.sh`. It refuses to run when six Rust builders are already up, builds `/tmp/oo-a190-overlay` from the immutable archive plus the three Rust files, and does not start PostgreSQL. The database script is `pg/up.sh`. The live test runs only when `OO_A190_LIVE=1`.

This session removed `oo-a190-rust194`, `oo-a190-pg`, volumes `oo-a190-cargo`, `oo-a190-target`, and `oo-a190-pgdata`, network `oo-a190-net`, and the private `/tmp` archive, overlay, and secrets. Exited `a064` containers were not removed. A new review needs its own container names, volumes, network, and secrets.

Workspace `HEAD` moved during the session with parser and connector commits. Those files were not inputs to this build. Compare the source commit to the archive, not to a later moving tree.
