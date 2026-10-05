# Handoff

Fixed review of the query-pg command is next. This directory is the author record for UUID `eef831aa-dba4-4af2-a78b-541bc9e0a198`. It is not that review, and it is not an independent review of the policy catalog.

## Source

Commit `4f3f329aa0dabb3f848e6040fdc5379df921c2f9`, subject `gateway: add the local query-pg command`.

Parent baseline for the proof archive: `78a60fdeabcada0d024fc6f9a81e0e6b184260e8`.

Owned paths in that commit:

- `crates/oo-gateway/src/bin/query-pg.rs`
- `crates/oo-gateway/tests/query_pg_cli.rs`
- `contracts/gateway/query-pg-cli-v1.md`
- `crates/oo-gateway/Cargo.toml`
- `Cargo.lock` (`oo-gateway` dependencies only: `serde`, `serde_json`)

Blobs of those files are in `source/blobs/`. The catalog, executor, guard, and parser worker were not edited. a194 and a195 stay immutable. a196 and a197 were other writers and were not touched.

## Contract to review against

`contracts/gateway/query-pg-cli-v1.md`.

Check the exit table, the rule that admission sync happens before user SQL, the rule that a guard refusal creates no audit file, and the rule that stdout integers are strings. The catalog remains author-approved. Do not treat a passing command test as catalog sign-off.

## Proof already run

Linux arm64 Rust 1.94 image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL 17.10 image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`.

Green phase exits are in `focused/phases/`. Raw logs are `focused/check.log`, `focused/test-offline.log`, `focused/test-live.log`, `focused/clippy.log`, and `focused/lib-test.log`. The live oracle is `live/oracle.txt`. Earlier failures are in `focused/attempts/`.

The private database, containers, and `/tmp/oo-a198-*` files are removed after this evidence commit. Rebuilding them is a new resource reservation. Do not reuse another task's builder, volume, or PostgreSQL.

## Limits to keep visible

- Password file only. No argv, env, request literal, or diagnostic copy.
- No HTTP entry and no claims authentication.
- No `GRANT` and no source database inside the command.
- Local audit receipt with `flush` and `sync_all`. Not a production WAL and not SP06-W06.
- Success exit only when every turn's executor status is `succeeded`.

Do not close a parent checklist from this handoff.
