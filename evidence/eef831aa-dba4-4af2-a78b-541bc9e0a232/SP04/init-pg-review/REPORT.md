# init-pg review

Verdict: APPROVE_LIMITED.

Session `eef831aa-dba4-4af2-a78b-541bc9e0a232`. Reviewed commit `6e1e6e576d1afd2bde10dedad649f09dee95bd34`, tree `7f387aeea34cfc374940bf454767f3a49b167be9`, parent `d71a17d68769306bb55a9b377c2aefb948da55c9`. The commit adds `contracts/pipeline/init-pg-cli-v1.md`, `crates/oo-pipeline/src/bin/init-pg.rs`, and `crates/oo-pipeline/tests/init_pg_cli.rs`. Those blobs match the a223 source copies. This review did not edit product source, manifests, or the lock. `cargo update` was not run. a207 remains the only writer of the connector manifest and the root lock.

This is the independent M1-D05A operator-command review. It does not accept M1-D05A, does not close SP04-W05, and does not release.

## What ran

Smoke, before the review harness, on a git archive of `6e1e6e57`: `linux/smoke/docker-run.exit` is 0, `rustc.exit` 0, `product-match.exit` 0, `overlay-absent.exit` 0, `test.exit` 0, `lock-unchanged.exit` 0. `clippy.exit` is the text `deferred_until_overlay`. Offline `cargo fetch --locked` exited 101 on the empty cargo volume. The following locked online fetch exited 0. `rustc` is 1.94.0, host `aarch64-unknown-linux-gnu`. The frozen `init_pg_cli` tests finished `4 passed; 0 failed; 1 ignored`. The ignored test is the author's live test. This review did not run it.

Proof copied only `overlay/init_pg_a232_review.rs` onto a fresh archive of the same commit. `product-match.exit` 0, `base-match.exit` 0, `overlay-match.exit` 0, `lock-unchanged.exit` 0, `cargo-update-invoked.txt` is `no`. Deciding container `oo-a232-proof`, id `6eba7630269be9374a88a23b422429c721b6594385009528e1a6a734c21c0895`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `--pull never`, linux/arm64, memory 6442450944, NanoCpus 1000000000, `CapDrop=["ALL"]`, not privileged, network `oo-a232-eef831aa-net`. `OOMKilled=false`. `docker-run.exit`, `test.exit`, `clippy.exit`, and `secret-scan.exit` are 0. The review test passed in 6.81s. `live/outcome.txt` is `init_pg_review_ok`.

PostgreSQL 17.10 image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, container `oo-a232-pg`, id `dffb7b5696fa71dfffcdb91ba36b097c523e20cec0a63212ab46d361104af04e`. SAN DNS `oo-a232-pg`. `pg/ports.json` is `{}`. Memory 1g, 1 CPU. One cluster held `oo_a232_src`, `oo_a232_dst`, `oo_a232_second`, `oo_a232_partial`, and `oo_a232_checksum`.

Recipe origins are `origin/recipes.txt`. `pg/pg-wrap.sh` and `tools/sha256_manifest.rs` are byte copies of the a200 and a199 files.

## Command results

Same private cluster, compiled `init-pg`, then compiled `oo-pipeline import-pg`. Raw files are under `linux/proof/live/`.

| Step | Exit | Stderr |
|---|---|---|
| `import-pg` before init | 1 | `oo-pipeline: target schema is not ready` |
| first `init-pg` | 0 | empty |
| replay `init-pg` | 0 | empty |
| `import-pg` | 0 | empty |
| `init-pg` after import | 0 | empty |
| wrong CA | 4 | `init-pg: target connect` |
| limited login on the installed database | 5 | `init-pg: target administrator` |
| limited login on the second empty database, before and after `CONNECT` | 5 | `init-pg: target administrator` |
| admin `init-pg` on the second empty database | 6 | `init-pg: target catalog refused` |
| partial `_oo` schema authorized to `oo_target_owner` | 6 | `init-pg: target catalog refused` |
| stored bootstrap checksum of 64 `ab` digits | 7 | `init-pg: migration checksum` |

First stdout: `install` `bootstrapped`, `full_import` `installed`, `historical_v2` false. Seals:

- bootstrap `37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208`
- fence `97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9`
- preflight `d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3`
- mapping `844de6f645f8f1ebf29d05118ae25d4669d8dc870eed1fa7697d4b98456eaf95`
- full import `a9cef2013019784857353ff4fe060350d71a4e9ac4ab18551b992a7a9b52ecb6`

The review oracle compared those compiled digests with `_oo.schema_migrations`. The five stored rows matched. `target-fence-receipts/2` count was 0. Replay left the migration text, including `applied_at`, unchanged. `pg_has_role('oo_pipeline_runtime', 'oo_target_owner', 'MEMBER')` was false, and the login `oo_a232_runtime` was not a member of `oo_target_owner`.

The harness, not `init-pg`, granted `CONNECT` on `oo_a232_dst` to `oo_a232_runtime` and granted `oo_pipeline_runtime` to that login. `import-pg` then exited 0. Protocol `oo.pipeline.import-pg-result/v1`, state `awaiting_publish`, `source_generation` 23, `table_count` 2, `target_generation` 1, run id `eff1c76f-9b86-43d4-a8ac-91c7697935dd`. Publication count stayed 0. Copied rows were `cedar-stem` and `ash-frame` on `yard.boats`, and `stem`, `frame`, and `keel` on `yard.parts`. Reader audit stayed 0 after the writer inserted one source row. A later `init-pg` was `replay` / `replay`, and the migration text stayed the sealed snapshot.

Wrong CA and the limited login left that snapshot unchanged. The second empty database stayed without `_oo`. Partial `public.kept` stayed `partial-a232`, and `schema_migrations` stayed absent. The bad checksum row and `public.kept` note `checksum-a232` stayed. Stdout and stderr did not contain that digest. `secret-scan.exit` is 0.

## Failed attempt

`linux/proof/attempt-1/` is the first proof. The review oracle compared migration rows in install order with `ORDER BY version`, so the test exited 101 after a successful first install. Clippy exited 101 because the harness field `secrets` was unread. The stored digests in that attempt already matched the init seals. The product source was not changed. The deciding run is the proof above.

## Limits

`init-pg` does not grant `oo_target_owner` to `oo_pipeline_runtime` and does not create a publication. `awaiting_publish` is the import result, not a publication. A partial `_oo` schema that `oo_target_owner` cannot use remains the author's open seam: the installer can return a database permission error and this command reports exit 1. This review did not add that case. The proved partial schema is authorized to `oo_target_owner`.

Smoke container id `681a08afabf9d8321975e21f184821f9b1ef1848a4633f42e91b086193b3e624`.
