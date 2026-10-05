# Findings

## Pre-existing vendor lints

`sqlx-postgres` in the archive prints two unfulfilled `#[expect(dead_code)]` warnings, in `message/flush.rs` and `message/mod.rs`. They appeared in the smoke test compile and in the proof clippy compile. `cargo clippy -p oo-pipeline --bin init-pg --test init_pg_cli -- -D warnings` still exited 0. The vendor tree was not edited.

## Non-administrator reaches a catalog probe first

`target_installer::install` calls `read_catalog` before `NotAdministrator`. `to_regclass('_oo.schema_migrations')` as a login without `USAGE` on `_oo` raises `42501` (`permission denied for schema _oo`). Attempt 2 recorded that as CLI exit 1 `target database`.

`init-pg` now applies the same administrator rule from `pg_roles` and `pg_has_role` before it calls `install`: a session that is not a superuser and not a member of `oo_target_owner` exits 5. The deciding run's limited login exited 5 and left `schema_migrations` unchanged. The installer source was not modified.

## Partial schema ownership

An `_oo` schema owned by the installing superuser is not usable by `oo_target_owner`. `0003` then raises `42501` instead of `OO002`, and the CLI maps that to exit 1. The transaction rolls back. Attempt 3 stopped on that shape.

The deciding fixture creates the partial schema with `AUTHORIZATION oo_target_owner`. That run exited 6 `target catalog refused`, kept `public.kept`, and left `schema_migrations` absent. A partial schema the owner role cannot use is still exit 1. This command does not change schema ownership.

## What the deciding run showed

`init-pg` does not grant `oo_target_owner` to `oo_pipeline_runtime`. The installer does not add that membership. Publication count stayed 0. `import-pg` before init exited 1 `target schema is not ready`. After init, replay, and the harness grants, `import-pg` on the same fixture exited 0 `awaiting_publish`. A second empty database on the same cluster exited 6 and did not create `_oo`. Wrong CA exited 4. A bad stored checksum exited 7 without printing the digest.

Smoke and proof compared `target_installer.rs`, `import_cli.rs`, `oo-pipeline.rs`, both `Cargo.toml` files, and `Cargo.lock` to the same hashes. The lock file was unchanged. No dependency was added.

M1-D05A is not accepted by this evidence.
