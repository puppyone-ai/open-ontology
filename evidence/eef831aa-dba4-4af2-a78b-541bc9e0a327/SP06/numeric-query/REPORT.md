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

Edited only the reserved paths, plus `policy_catalog.rs` and `policy.rs` after those two files were transferred for the bounded allowlist. Manifests, the lock, `lib.rs`, gateway HTTP, root consumers, and a317 data were not modified. The live-3 packet left `mapping_reader_access.rs` on the archive copy. The mapping-reader section below records the later owned edit.

## Mapping-reader bound, live-4

Addendum `9183e79082341a1a2bd7d3b1a2ae3e8764735d0e` froze the bound before this source edit. Product commit `21cc96241a841b47dbbcb116549030e38a643ba0` changes `mapping_reader_access.rs` and the existing `a327_live` harness in `query-pg.rs`. Parent of that commit is `e7fb67e27bcffcdeec10ad84463b4854bef6eed0`. `9183e790` is an ancestor. Other tasks committed between the addendum and the product commit.

`ensure_supported_type` selects `typname`, `typtype`, and `atttypmod` and accepts the column when `typtype` is `b` and `grantable_base_type` admits the pair. That is the same predicate install already uses. Precision `1..=38` stays the numeric bound. The check runs inside `resolve_tables`, before `grant_mapping`. A refusal rolls the provision transaction back.

`linux/smoke-map/smoke.exit` is 0. That run compiled the frozen tree on Rust 1.94 before the edit. `mapping_reader_access.rs` in that tree still used `SUPPORTED_TYPES.contains`. Lib tests: 31 passed. Lock before and after `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Two earlier launches, `linux/smoke-map-failed-1` and `linux/smoke-map-failed-2`, exited 125 because Docker Desktop would not bind the recreated sibling directory. The successful smoke and live-4 use `.a327-work` inside this workspace. `private_root_reason=recreated_sibling_unbindable_workspace_child_binds`.

`linux/live-4` then compiled the seven-file overlay and ran the same six-row proof. `docker-run.exit` is 0. `live.exit` is 0. `outcome.txt` is `numeric_query_recorded`. Hostconfig: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=oo-a327-eef831aa-net`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`. `pg/live-4/up.exit` is 0. Server `17.10 (Debian 17.10-1.pgdg12+1)`, `ssl` true. Host cargo at launch was 0. `builders-before.txt` names the foreign builder `oo-a337-preoverlay-3`. This task's own rust builder was `oo-a327-live4`.

Phases, all exit 0: toolchain, overlay, libclang, libclang-load, clippy-component, fetch, test-lib (31 passed), test-query-pg, test-row-filter, clippy `--lib --bins -D warnings` (`Finished` in `clippy.log`), build-pipeline, build-builder, build-gateway, test-live (`a327_live::live_numeric_proof ... ok`, 1 passed), lock-hash. `cargo-update-invoked=no`.

The source table kept the six decided rows and added nullable `loose numeric` and `over numeric(39,4)`. Those columns are not decisions. Before install, numeric typmods on `_oo` included `-1`, `1179656`, `2490376`, and `2555912`. `provision-loose` and `provision-over` each exited 7 with stderr exactly `provision-mapping-reader: policy type\n` (38 bytes). Login mapping privileges were absent before those calls and still absent after them. `install-column-acl` then exited 0 and `provision-mapping-reader` exited 0.

| Step | Exit |
| --- | --- |
| provision-loose | 7, stderr `provision-mapping-reader: policy type` |
| provision-over | 7, stderr `provision-mapping-reader: policy type` |
| install-column-acl | 0 |
| provision-mapping-reader | 0 |
| before-analyze | 5, stderr exactly `query-pg: policy\n` |
| hidden | 6, stderr exactly `query-pg: guard\n` |
| amounts | 0 |

`amounts.stdout` is the same six decimal rows as live-3. Summary `returned_rows` 6, `result_bytes` 162, `original_sha256` `466f22e579afe5252e78ae6cffca1da0a9c4630ba8cb63440a0fc9b7a9c6957a`, `executed_sha256` `b7def2bb9721b0f643ba1e37bd6d4e5767b258d6e2ad4886bb0634a115d8adbc`. `schema_hash` is `d64749d8fe1c9934a424dab4f296daadbf6f6d9918bab626e160babb3d66a8b0`. `config_digest` is `e51342d9f0fd173507b70469d999e7b5beeb2b045ff9be3f0a3c9f46aaac86b4`.

Live `SUPPORTED_TYPES` readers after this edit:

- `grantable_base_type` is the numeric precision bound. Install, catalog visibility (`supported_base`), and mapping-reader call it.
- `whitelist_tokens_ok` checks the allowlisted name syntax. It does not read typmod.
- `load_types` requires one `pg_catalog` base type for each allowlisted name. It does not apply precision.
- `supported_base` also requires the type OID to equal the loaded `pg_catalog` base type. Install and mapping-reader match the column's own `typname`, `typtype`, and `atttypmod`. That OID difference already exists for `bool`, `int2`, `int4`, `int8`, and `text`.
- `COMPARISON_SIGNATURES`, operators, literals, and aggregates still omit `numeric`. A bounded numeric column can be selected. This edit does not add a comparison, a literal, or an aggregate. `required_operator_count` stays 54.

No other live Rust file reads `SUPPORTED_TYPES`. This task did not re-run a317. No M1-I01, M1-I02, parent acceptance, or release claim.
