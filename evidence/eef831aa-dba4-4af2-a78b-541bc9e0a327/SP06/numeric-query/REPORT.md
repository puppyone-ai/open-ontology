# Numeric query, a327

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a327`. Scope E46/M1-D14-N. Fixed ancestor `e6ebf93b5ac261c097a24d145611e0300e4f88b5`.

`linux/live-3` compiled the reviewed policy, the role pack, and `query-pg` from that archive plus the owned overlay, then selected six `shop.entry` rows over private TLS. The amount and wide cells are exact decimal JSON. The hidden column is a guard denial. The same query before `ANALYZE` is a policy denial. `install-column-acl` and `provision-mapping-reader` exited 0.

No M1-I01, M1-I02, parent acceptance, or release claim. This task did not re-run the a317 100k fixture. a317 gold, data, and decisions are unchanged.

## Commits

| Commit | What |
| --- | --- |
| `e6ebf93b5ac261c097a24d145611e0300e4f88b5` | Fixed archive. Immutable. Proof base. |
| `3517cd131719cfdd572ba2b85bb7013e6c90588e` | Frozen numeric wire, type, semantic, and cell contract. Before product code. |
| `c671a3caa1265dbf97a313b065cc13c188c1533a` | Quantity admission and exact OID 1700 decoder. |
| `2f2b79b1f8ba767b847a384d3bdd29af0a4d6948` | First evidence packet. Install exit 7 is preserved there. |
| `c4d9316d28ceefefc7bc21cd8ac61ce949232d07` | Allowlist addendum. Frozen before the two catalog edits. |
| `e9cba7df4b8d20ea8b3794f9efcc813434d7b1dd` | Bounded numeric in `policy_catalog.rs` and `policy.rs`. |
| `976cb4538f621eb5de157cef32976b029c6d17cf` | `row_filter: null` is the absent filter. Product SHA proved by live-3. |

Evidence for this UUID lives under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a327/SP06/numeric-query/`.

## Private database, live-3

Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Server `17.10 (Debian 17.10-1.pgdg12+1)`. Container `oo-a327-pg`, 6 GiB, 1 CPU, private network `oo-a327-eef831aa-net`, TLS, no published port. `pg/live-3/up.exit` is 0. `pg/up.exit` from the first start is still 0.

Builder image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Linux arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Container `oo-a327-live3`, 6 GiB, 1 CPU, `CARGO_BUILD_JOBS=1`, `RUSTUP_TOOLCHAIN=1.94.0`, locked offline. `cargo-update-invoked=no`. Host cargo count before launch was 0. No other a327 rust builder was running. Hostconfig: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=oo-a327-eef831aa-net`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`. `docker-run.exit` is 0. `live.exit` is 0. `outcome.txt` is `numeric_query_recorded`.

The mount was a git archive of `e6ebf93` (`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`) plus six owned files. The archive lock stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` before and after the build. The workspace `Cargo.lock` is a different file and was not the proof lock.

Private root `/Users/redux/puppy-one/oo-a327-eef831aa-dba4-4af2-a78b-541bc9e0a327`. Docker Desktop cannot bind-mount `/tmp`, so the root is a sibling of the workspace. `private_root_reason=docker_cannot_bind_mount_/tmp`.

Phases, all exit 0: toolchain, overlay, libclang, libclang-load, clippy-component, fetch, test-lib (31 passed, including `bounded_numeric_admits_precision_1_through_38` and `numeric_quantity_is_the_only_new_admission`), test-query-pg (`decimal_json_is_exact_and_existing_kinds_stay`), test-row-filter (`absent_row_filter_null_is_optional`), clippy `--lib --bins -D warnings`, build-pipeline, build-builder, build-gateway, test-live, lock-hash.

| Step | Exit |
| --- | --- |
| catalog-pg | 0 |
| captured-pg-draft | 0 |
| review-pg-draft | 0 |
| reviewed-effective-oos | 0 |
| role-pack | 0 |
| role-pack-bundle | 0 |
| import-pg before init | 1, stderr `oo-pipeline: target schema is not ready` |
| init-pg | 0 |
| import-pg | 0 |
| reviewed-policy | 0 |
| install-column-acl | 0 |
| provision-mapping-reader | 0 |
| analyze-pg | 0 |
| before-analyze | 5, stderr exactly `query-pg: policy\n`, stdout empty |
| hidden | 6, stderr exactly `query-pg: guard\n`, stdout empty |
| amounts | 0, stderr empty |

`reviewed-policy` wrote one policy for `shop.entry`. Semantics are `identifier`, `quantity`, `quantity` for `entry_id`, `amount`, and `wide`. `amount` is `numeric(18,4)`. `wide` is `numeric(38,4)`. The import copied the six source rows. `outcome.json` records `amounts_rows` 6, `ssl` true, `sql_in_query_output` false, `policy_tables` 1, generation 27, and the three acceptance fields unset.

`query-pg` stdout for the amount query is the file `linux/live-3/steps/amounts.stdout`:

```
{"record":"row","turn":0,"cells":[{"kind":"int","value":"1"},{"kind":"decimal","negative":false,"coefficient":"123456789012345678","scale":4},{"kind":"decimal","negative":false,"coefficient":"12345678901234567890123456789012345678","scale":4}]}
{"record":"row","turn":0,"cells":[{"kind":"int","value":"2"},{"kind":"decimal","negative":true,"coefficient":"123456789012345678","scale":4},{"kind":"decimal","negative":true,"coefficient":"12345678901234567890123456789012345678","scale":4}]}
{"record":"row","turn":0,"cells":[{"kind":"int","value":"3"},{"kind":"decimal","negative":false,"coefficient":"1","scale":4},{"kind":"decimal","negative":false,"coefficient":"1","scale":4}]}
{"record":"row","turn":0,"cells":[{"kind":"int","value":"4"},{"kind":"decimal","negative":true,"coefficient":"1","scale":4},{"kind":"decimal","negative":true,"coefficient":"1","scale":4}]}
{"record":"row","turn":0,"cells":[{"kind":"int","value":"5"},{"kind":"decimal","negative":false,"coefficient":"0","scale":4},{"kind":"decimal","negative":false,"coefficient":"0","scale":4}]}
{"record":"row","turn":0,"cells":[{"kind":"int","value":"6"},{"kind":"null"},{"kind":"null"}]}
{"record":"summary","turn":0,"status":"succeeded","truncated":false,"returned_rows":6,"result_bytes":162,"rollback_confirmed":true,"socket_dropped":false,"end_audit_ok":true,"original_sha256":"466f22e579afe5252e78ae6cffca1da0a9c4630ba8cb63440a0fc9b7a9c6957a","executed_sha256":"b7def2bb9721b0f643ba1e37bd6d4e5767b258d6e2ad4886bb0634a115d8adbc","stop":null,"sqlstate":null}
```

## Earlier boundary, still recorded

`linux/live` is the run before the allowlist. `install-column-acl` exited 7 with stderr `install-column-acl: policy type`. `live.exit` is 101. That directory was not rewritten.

`linux/live-2` is the run after the allowlist and before the request-null fix. Install and provision exited 0. `before-analyze` exited 3 with stderr `query-pg: request`. `live.exit` and `docker-run.exit` are 101. No `amounts.stdout`. That directory stays.

## Linux checks before the allowlist

Pre-edit smoke exit 0 (`linux/smoke/smoke.exit`, `docker-run.exit` 0). Post-edit product phases on Rust 1.94 exited 0 for `oo-gateway` lib tests, `a327_numeric`, `query-pg` unit tests, and clippy `--lib --bins -D warnings`. `linux/postedit/postedit.exit` is 2. The harness in that run required `wrong_physical_quantity_is_semantic` to fail. The test passes: an unbounded numeric column has no `data_type`, so admission returns semantic before the numeric rule. The contract records that pass as the product result. `linux/postedit/inside.sh` now treats a zero exit from that test as success. The recorded `postedit.exit` of 2 was not rewritten.

## Preserved failed attempts

| Directory | Result |
| --- | --- |
| `linux/postedit-failed-1` | Docker could not mount a new file onto the read-only archive. |
| `linux/postedit-failed-2` | `PgNumericCell` was not `Debug`. |
| `linux/postedit-failed-3` | Clippy was absent until `rustup component add`. |
| `linux/live-failed-1` | `--exact live_numeric_proof` matched nothing. Cargo exited 0 with 0 tests. |
| `linux/live-failed-2` | Fixture insert `sqlstate=22003`. `numeric(18,4)` cannot store Infinity. |
| `linux/live-failed-3` | `import-pg` exit 1: `full import refused numeric_nan`. |
| `linux/live` | Install type refusal, exit 7. |
| `linux/live-2` | Request refusal, exit 3, after the allowlist already installed. |

The live filter that produced cells is `--exact a327_live::live_numeric_proof` with `OO_A327_LIVE=1`.

## Ownership

Edited only the reserved paths, plus `policy_catalog.rs` and `policy.rs` after those two files were transferred for the bounded allowlist. Manifests, the lock, `lib.rs`, gateway HTTP, root consumers, and a317 data were not modified. `mapping_reader_access.rs` has no diff against `e6ebf93` and was not mounted as an overlay.
