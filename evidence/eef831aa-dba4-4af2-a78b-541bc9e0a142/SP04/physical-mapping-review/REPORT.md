# Physical mapping review

Verdict: **APPROVE LIMITED** for the `physical-mapping/v1` primitive at `b5415544c9eb8d608eafd411884627dda9ee7c84`.

The primitive stores one finished `pgident/v1` logical name, for one source, config digest, and run generation, as a 63-byte PostgreSQL identifier. Register recomputes SHA-256 and returns `inserted` or `replay`. Lookup reads the stored row. This approval does not cover the Loader, fenced `COPY`, or reader R4.

## Source

Reviewed commit `b5415544c9eb8d608eafd411884627dda9ee7c84`, tree `bbb320f7523099699a606f8fa189a5a5a37a8ec9`, parent `6766126b8e7c0916d6740ba75601698d2ebd0fb2`. Workspace HEAD at archive time was `fa9f701c3889391bb1c86c00960db2ae9a00b2b4`. This review did not move HEAD.

The snapshot is `git archive` of that commit. Archive SHA-256 `9631a9060fc57c277b98ec12c4b269a1412393b4b5295011c70920443544fef4`. Verification: `files=728`, `links=0`, `mismatched=0`, `missing=0`, `expected=728`. The archive includes `contracts/pipeline/target-fence-receipts` because `cargo test -p oo-pipeline` compiles those fixtures. It is not a product edit.

`0000` through `0003` are byte-identical to the parent. Content SHA-256 inside the container:

| file | sha256 |
|---|---|
| `0000_bootstrap_roles.sql` | `37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208` |
| `0001_target_fence_expand.sql` | `97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9` |
| `0002_target_segment_null_expand.sql` | `59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512` |
| `0003_target_segment_preflight_expand.sql` | `d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3` |
| `0004_physical_mapping.sql` | `844de6f645f8f1ebf29d05118ae25d4669d8dc870eed1fa7697d4b98456eaf95` |

Git blob of `0004` is `02e27bbca30d6546e73a2144b3db38dae55e430d`. Author evidence `6cdb559b` has no complete hash manifest. This review does not claim that inventory verified.

## Images and runs

Rust `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. Postgres `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `linux/arm64`. Both were already local. Pull was never. Toolchain `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Server `17.10 (Debian 17.10-1.pgdg12+1)`.

Green run `0c476cb4-eb51-4d3a-b8f9-0d417ecd6509`. Capacity before launch: `builder_containers=0 host_cargo=0 own_postgres=0`. Rust builder cap-drop `ALL`, no added caps, `no-new-privileges`, private bridge, no published ports, `HOME=/tmp`. Postgres cap-drop `ALL`, added `CHOWN,DAC_OVERRIDE,FOWNER,SETGID,SETUID`.

| attempt | where | test | clippy | live | probe | restart | docker |
|---|---|---|---|---|---|---|---|
| 1 missing fence-receipt contracts | `linux/fixed/attempt-1` | 101 | 101 | 9 | 9 | skipped | 101 |
| 2 probe parsed `BEGIN` as a data row | `linux/fixed/attempt-2` | 0 | 0 | 0 | 1 | skipped | 0 |
| 3 green | `linux/fixed` and `linux/probe` | 0 | 0 | 0 | 0 | 0 | 0 |

Attempt 1 is a harness miss. The package tests `include_str` `contracts/pipeline/target-fence-receipts`, and the first archive omitted that directory. Attempt 2 stored the correct row and then failed two assertions because `psql -At` still prints `BEGIN` and `COMMIT`. The stored-row check in that same transcript already matched the Python digest. Restart did not run. The parser now drops those command tags. `attempt-2/probe/probe.py` is the failing copy.

`cases.log` appends. It holds the attempt-2 block and the green block. The green block is the second `live=ok`. `attempt-2/cases.log` has only the first block.

## Commands

```
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test physical_mapping_live physical_mapping_live -- --ignored --exact
```

The live test ran only after fetch, the package test, and Clippy had all exited 0. Green exits: fetch 0, test 0, clippy 0, live 0, worst 0, docker-run 0, probe 0, restart 0, clippy-component 0. Package output includes library `7 passed`. The non-ignored `physical_mapping_live` counted `1 passed` and `1 ignored`. The ignored invocation then passed that one test: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.15s`.

`sqlx-postgres` printed two unfulfilled `#[expect(dead_code)]` warnings during the package test and during Clippy. Clippy still exited 0. Those files were not edited.

## Checked on PostgreSQL

The unchanged live test on database `oo_pm` installed `0000`, `0001`, and `0004`. Its green block records SHA-256 oracles matching empty, `abc`, 200 bytes of `a`, and the golden payload `d528fe7ee77dde914147ad1edc2b2d6f2f42b25106ccfe6f39d5f8996f1fc398`. Physical name `pn_d528fe7ee77dde914147ad1edc2b2d6f2f42b25106ccfe6f39d5f8996f1f`. Locale `UTF8|b|C.UTF-8`. The long pair that shares a 63-byte logical prefix received `pn_f5cb921527f810b82edcb1508ad40c4338be0e3b819b87f3e329c8d03743` and `pn_dac559f034005c71ce1d7919f73a140f58adfa8e9d38294e0be8d5c4dea4`. Unicode logical length 482 stayed one stored identity. Normalization, reserved, and quoted forms stayed distinct. Closed payloads returned `23514`, including a 4096-byte name. Absent generation `OO024`. Column name `pc_48f7de829df96644ef7e4fe0ae0f8e9b5a56fadd98e261cc6b2bf524f7bd`. Conflict `OO022` kept the original row. Rollback left the identity absent (`OO024`). Concurrent registration returned `inserted+replay` and one row. Agent and runtime denial `42501`. Direct write `OO014`. Unfenced `commit_apply_receipt` returned `OO011`, with `apply_receipts` count 0 and public user tables 0. A second apply with the same checksum skipped, and a different checksum returned `OO001`.

The independent probe on database `oo_pm_review` uses source `src_review`, config `ef` repeated to 64 hex characters, generation 4, and two logical names of 82 bytes that share their first 63 bytes and differ in the last two hex characters. Python `hashlib` and `_oo.pm_sha256` agreed:

- left digest `3ed5f69f238377b6dced6027048080b23cfe75801b4a976aef004cb21972282b`, physical `pn_3ed5f69f238377b6dced6027048080b23cfe75801b4a976aef004cb21972`
- right digest `a34b0d15bc039b6621031f2b4ed9b1624092b41329be13fd54324a12ead34431`, physical `pn_a34b0d15bc039b6621031f2b4ed9b1624092b41329be13fd54324a12ead3`

Register as `oo_a142_runtime` returned `inserted`. The following statement in the same transaction read both `_oo.physical_mapping_writer` and `_oo.writer` as empty. Replay returned `replay` and left `registered_at` unchanged. Lookup returned the stored physical name. The stored row has `octet_length(physical_name) = 63` and `octet_length(logical_namespace) = 82`. SQL digest of the golden payload and of the raw golden bytes matched `d528fe7ee77dde914147ad1edc2b2d6f2f42b25106ccfe6f39d5f8996f1fc398`.

Catalog constraints present: `physical_mappings_pkey`, `physical_mappings_identity_digest_unique`, `physical_mappings_physical_name_unique`, and `octet_length(physical_name) = 63`. Lookup is security definer, owner `oo_target_owner`, `search_path=pg_catalog, _oo`, and `strpos` of `pm_sha256` in its source is 0. Register is security definer with the same owner and search path, and `pm_sha256` occurs at offset 2586. `pm_sha256` is security invoker.

`oo_a142_agent` select, lookup, and register returned `42501`. `oo_a142_runtime` select, `pm_sha256`, `set_config`, and `SET` plus `INSERT` returned `42501`. Runtime execute of register and of lookup is true. Owner `INSERT` without the writer GUC, and owner `UPDATE`, `DELETE`, and `TRUNCATE` with the writer GUC, returned `OO014`. Unfenced `commit_apply_receipt` returned `OO011`. Receipt count stayed 0. Public user tables stayed 0. `SELECT 1 AS "pn_..."` parsed the quoted physical name.

The probe installer applied the `0000` remainder after `CREATE ROLE` (roles already existed), then `0001`, `0003`, and `0004` as `oo_target_owner`, and recorded `schema_migrations` for `target-fence-receipts/3` and `physical-mapping/v1`. A later transaction that only locks and reads the checksum left `applied_at` and the table oid unchanged. A different checksum raised `OO001` and left the row unchanged. `UPDATE` of the marker returned `OO014`. This is the narrow installer contract. It is not a full import.

After `docker restart` of that same postmaster, a new `psql` session called lookup before any new hash. `restart-compare.txt` says `match=1`. Golden lookup and the stored row are `pn_d528fe7ee77dde914147ad1edc2b2d6f2f42b25106ccfe6f39d5f8996f1f` and digest `d528fe7ee77dde914147ad1edc2b2d6f2f42b25106ccfe6f39d5f8996f1fc398`, server `17.10 (Debian 17.10-1.pgdg12+1)`. The fresh probe row lookup returned `pn_3ed5f69f238377b6dced6027048080b23cfe75801b4a976aef004cb21972`, and the stored select returned that name, digest `3ed5f69f238377b6dced6027048080b23cfe75801b4a976aef004cb21972282b`, and octet length 63.

## Boundary

`_oo.reject_physical_mapping_write` rejects every non-`INSERT`, and rejects `INSERT` unless `_oo.physical_mapping_writer` is `register`. It does not recompute the digest. `_oo.register_physical_mapping` does, at the `OO022` check, and inserts `v_digest`. The table checks bind `physical_name` to the first 60 hex characters of `identity_digest`. They do not require `identity_digest` to equal `pm_sha256(pm_payload(...))`.

As `oo_target_owner`, with the writer GUC set, an `INSERT` of logical name `review_lie`, generation 9, and a 32-byte digest of `0x11` was accepted inside an open transaction (`state=OK`). `ROLLBACK` followed. The count of `review_lie` afterwards was 0. Runtime cannot take this path: its `INSERT` is `42501` before the trigger. The limited approval covers register, lookup, and the runtime and agent grants. It does not treat a table-owner insert as a second registration API.

## Cleanup

Green cleanup removed the Rust container, the Postgres container, the network, and the cargo, target, and Postgres volumes. A later `docker volume ls` and `docker network ls` showed no `a142` volume or network, and no `oo.owner=a142` container. No prune. Slot B resources were not removed.
