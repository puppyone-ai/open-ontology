# init-pg operator command

Session `eef831aa-dba4-4af2-a78b-541bc9e0a223`. Fixed archive `c2db9c430b822a824a1062b6c3c7345924df7dd8`, tree `8fb187147b02d6cacf45cb5a683859f698b497cf`. Product commit `6e1e6e576d1afd2bde10dedad649f09dee95bd34` on the three owned paths. This is the M1-D05A operator command evidence. It does not accept M1-D05A, does not close a parent checkbox, and does not release.

a207 remains the only writer of the connector manifest and the root lock. This batch did not edit `Cargo.toml`, `Cargo.lock`, `lib.rs`, the installer, the migrations, or an existing CLI. `cargo update` was not run.

## What ran

Smoke, before the overlay, on the fixed archive: `linux/smoke/docker-run.exit` is 0, `lib.exit` 0, `cli.exit` 0, `fetch.exit` 0. `rustc` 1.94.0 linux/arm64. `clippy.exit` is the text `deferred_until_overlay`. The three init-pg paths were absent. Base file hashes are `linux/smoke/archive-sha256.txt`.

Proof mounted that archive plus only `crates/oo-pipeline/src/bin/init-pg.rs`, `crates/oo-pipeline/tests/init_pg_cli.rs`, and `contracts/pipeline/init-pg-cli-v1.md`. `linux/proof/base-match.exit` is 0. `lock-unchanged.exit` is 0. `cargo-update-invoked.txt` is `no`.

The deciding proof container was `oo-a223-proof`, id `f7ac5c3580a1823b0026a197e3c07f3219e4cda2b3bab56d33cd291e23099f71`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `--pull never`, linux/arm64, 6442450944 bytes, 1 CPU, `CapDrop=["ALL"]`, not privileged, network `oo-a223-eef831aa-net`. `OOMKilled=false`. `docker-run.exit`, `clippy.exit`, `test.exit`, and `live.exit` are 0. Offline `init_pg_cli` was 4 passed and 1 ignored. The ignored test then passed in 5.11s. `live/outcome.txt` is `init_pg_cli_ok`.

PostgreSQL 17.10 image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, container `oo-a223-pg`, id `df2c2552fecbe2a45742002f2df101506489a54e1340032c4f7deb6bdd4f51ba`. SAN DNS `oo-a223-pg`. `pg/ports.json` is `{}`. Memory 1g, 1 CPU. One cluster, source database `oo_a223_src` and target `oo_a223_dst`.

Recipe origins and the diffs from them are in `origin/recipes.txt`. `pg/pg-wrap.sh` is a byte copy of the a200 file. `tools/sha256_manifest.rs` is a byte copy of the a199 file.

## Command results

Same fixture, compiled `init-pg`, then compiled `oo-pipeline import-pg`:

1. `import-pg` before init exited 1. Stdout empty. Stderr `oo-pipeline: target schema is not ready`. `_oo` was absent.
2. First `init-pg` exited 0. `install` `bootstrapped`, `full_import` `installed`, `historical_v2` false. Seals and `full_import_digest` matched `compiled_digests` and `compiled_full_import_digest`. Publication count 0. `pg_has_role('oo_pipeline_runtime', 'oo_target_owner', 'MEMBER')` was false. `full_import_run` count 0.
3. Second `init-pg` exited 0. `replay` / `replay`. `schema_migrations` text, including `applied_at`, was unchanged.
4. The harness, not the CLI, granted `CONNECT` on `oo_a223_dst` to `oo_a223_runtime` and granted `oo_pipeline_runtime` to that login.
5. `import-pg` exited 0. Protocol `oo.pipeline.import-pg-result/v1`, state `awaiting_publish`, `source_generation` 11, `table_count` 2, run id `8d32d31e-5865-432c-8fe3-f2c8f7283a61`. Publication count stayed 0. `full_import_run` count became 1. Reader audit stayed 0 after the writer inserted one source row.
6. `init-pg` again exited 0 with `replay` / `replay`. Migration text stayed the sealed snapshot.
7. Wrong CA exited 4, stderr `init-pg: target connect`. Migrations unchanged.
8. Limited login on the installed database exited 5, stderr `init-pg: target administrator`. Migrations unchanged. Runtime was still not a member of `oo_target_owner`.
9. Limited login on a second empty database exited 5 before and after `CONNECT`. Admin init on that database exited 6, stderr `init-pg: target catalog refused`. `_oo` count on the second database was 0. The first database migration text was unchanged.
10. Partial `_oo` schema, authorized to `oo_target_owner`, with `public.kept` row `partial-a223`, exited 6. The row stayed. `schema_migrations` stayed absent.
11. Stored bootstrap checksum of 64 `ab` digits exited 7, stderr `init-pg: migration checksum`. The row and the stored checksum stayed. Stdout and stderr did not contain that digest.

`linux/proof/live/` holds each step's stdout, stderr, and exit. The marker `s3cret-a223-marker` is absent from those files.

## Failed attempts

`linux/proof/attempt-1/` is the first proof. Clippy exited 101 because `PgConnection::connect_with` needed `sqlx::Connection` in the test. Not OOM.

`linux/proof/attempt-2/` is the second proof. Offline tests and clippy passed. The limited login exited 1 `target database`: `to_regclass` on `_oo` raises `42501` before `install` can return `NotAdministrator`. The CLI now reads `pg_roles` and `pg_has_role` before `install`.

`linux/proof/attempt-3/` is the third proof. Privilege exited 5 and the second empty database exited 6. The partial schema, owned by the admin, made `0003` raise `42501` as `oo_target_owner`. The fixture now creates `_oo` with `AUTHORIZATION oo_target_owner`.

The fourth proof is the deciding run above. The 6GiB line-tables-only retry did not run.
