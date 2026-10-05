# Findings

## Frozen command

`init-pg` reads one `oo.pipeline.init-pg/v1` document from stdin and takes no arguments. The password and CA are paths. The connection is verify-full with the caller CA. The process environment is cleared in the review oracle, with decoy `PGPASSWORD`, `DATABASE_URL`, and `PGSSLROOTCERT` values. The successful install used the password file and the server CA.

The command calls `install`, `verify_readiness`, `install_full_import`, and `verify_full_import_readiness`. Stdout seals matched `compiled_digests` and `compiled_full_import_digest`. The same five checksums were stored in `_oo.schema_migrations`. Replay did not change `applied_at`.

## Runtime membership

After bootstrap, `oo_pipeline_runtime` was not a member of `oo_target_owner`. The review login was not a member either. The harness granted `oo_pipeline_runtime` to `oo_a232_runtime` and granted `CONNECT` before `import-pg`. After that grant, the login was still not a member of `oo_target_owner`. Publication count stayed 0 through import and the later replay.

## Refusals

A different CA certificate, accepted as PEM and not the server CA, exited 4. Migrations were unchanged.

A login that is not a superuser and not a member of `oo_target_owner` exited 5 on the installed database and on a second empty database, both before and after `CONNECT`. Migrations on the first database were unchanged.

Admin init of that second empty database exited 6. `_oo` was absent there. The first database migration text was unchanged. The product roles already existed on the cluster, so this is the contract's second-database refusal. `0000_bootstrap_roles.sql` was not edited.

Partial `_oo`, created with `AUTHORIZATION oo_target_owner`, plus `public.kept` row `partial-a232`, exited 6. The row stayed. `schema_migrations` stayed absent.

A stored `target-bootstrap/1` checksum of 64 `ab` digits exited 7. The row, the note `checksum-a232`, and the stored digest stayed. The digest was not in stdout or stderr.

## Pre-existing vendor lints

`sqlx-postgres` prints two unfulfilled `#[expect(dead_code)]` warnings, in `message/flush.rs` and `message/mod.rs`. `cargo clippy -p oo-pipeline --bin init-pg --test init_pg_cli --test init_pg_a232_review -- -D warnings` exited 0. The vendor tree was not edited.

## Open seam left in place

An `_oo` schema owned by the installing superuser is not the case this review ran. The author recorded that `0003` then raises `42501` and the CLI maps that to exit 1. The contract's proved partial-catalog refusal is exit 6 when the owner role can use the schema. A change for the unusable-owner shape needs its own frozen child. This review did not add that test and did not edit the installer.

## Archive identity

a223 proved the three files on archive `c2db9c430b822a824a1062b6c3c7345924df7dd8`. The reviewed commit's parent is `d71a17d68769306bb55a9b377c2aefb948da55c9`. `Cargo.lock`, `crates/oo-pipeline/Cargo.toml`, and the pipeline sources other than the three new files are the same bytes in that archive and in this commit. This review built the commit, not the worktree.

M1-D05A is not accepted by this evidence.
