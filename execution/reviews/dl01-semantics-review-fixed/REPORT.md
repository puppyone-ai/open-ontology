# DL01-W1-36 fixed semantics review

Reviewer Grok grok-4.7/xhigh, session `025e07bb-e26b-4c0e-8b43-a80ac0f48dda`. The frozen child is DL01-W1-05 in `execution/batches/dl01-wave1-20261009/DL01-W1-05-semantics.md` together with its convergence card. This packet reviews the sealed-import extension on that same child. Source `b1ffb079d7beec4297dbd3deec97219584c6775d`. Seal `b0e64601a24d4a824fe30ed32cba038deb811ee7`. Producer catalog commit `c6f628a3a8c34c731704e2008d31bf00ec6a7f9a`. Author packet `evidence/1e7798c0-f799-4d4b-ab47-5e15599f60d9/DL01/semantics`. Dependency archive `577160c22de63adb9b38e0147f3bc2da893c4d9d`.

Verdict: approved, bounded to the sealed-import compiler extension of DL01-W1-05 at `b1ffb079`. `compile_sealed_import_catalog`, `compile_reviewed_sealed_import`, and `compile_role_pack` consume the producer catalog bytes and return an effective OOS document and a role pack. The pack carries source engine `mysql`, `normalized_representation` `postgresql_target_storage`, the producer digests, generations 1 and 3, and identity hash `33bb4e8b9385da219db0fd784ab06589bda0bbe9c08518fc38fd68fbefe4677f`. The old PostgreSQL catalog compiler still rejects this document. Stale bindings, a wrong schema digest, unsealed and retired state, and a stale generation do not pack. An input-digest change packs only under the new identity hash.

Commit `cb5b71e37` approved source `5bd6e414bf0b3ca008dc413fb39f2c6d0aa8bae5` only. That approval does not cover these three APIs. This packet replaces the review files at this path.

Parent DBX-W05, the 0/8 acceptance count, gateway `get_freshness`, live database reads, DBP8 binary-sort proof, the support matrix, and release stay open. The producer file is a sealed catalog fixture. `live_database` is `not_connected`. This run is not a MySQL source acceptance.

## Identity

`577160c22de63adb9b38e0147f3bc2da893c4d9d` is an ancestor of `b1ffb079`. `5bd6e414` is an ancestor of `b1ffb079`. `b1ffb079` is the parent of seal `b0e64601`. The seal diff is `evidence/1e7798c0-f799-4d4b-ab47-5e15599f60d9/DL01/semantics/sealed-import/receipt.json` only, 66 lines. Source subject: `DL01-W1-05: compile a sealed import catalog into the role pack.` Seal subject: `DL01-W1-05: record the sealed-import compiler receipt.`

`git diff 577160c b1ffb079 -- Cargo.toml Cargo.lock` is empty. Root `Cargo.toml` sha256 `b772f8fb4873885fcfac747eb24163559031bb9192ba99e8a8472571c75822bc`. Root `Cargo.lock` sha256 `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`.

`contracts/schema/oos-v1.json` content sha256 at archive `577160c` is `48432a164cba1e42c28790ad499850a8f3b76abc0b3fb863d9538b6dbe279462`, 54432 bytes, git blob `90b56670f23dea50adfa722e55b8b55296f6f9d4`. The same path at `5bd6e414`, `b1ffb079`, and the seal is sha256 `ee134c232f0d1f1e5ac3cdd435462ee15ed0f66a801dad7d7cdd32daf244f331`, 60068 bytes, git blob `f8bbc8d0f24bb0d0382e778ed399c6946cc6c69e`. `git diff 5bd6e414 b1ffb079 -- contracts/schema/oos-v1.json` is empty. The compile overlay used the `b1ffb079` blob.

The compile input was `git archive 577160c` plus `crates/oo-core`, `crates/oo-builder`, `tools/source-semantics`, and `contracts/schema/oos-v1.json` from `b1ffb079`. Ninety-six overlay blobs matched `git hash-object` to `git rev-parse`. `tools/source-conformance` was absent and was not overlaid. The mutable worktree was not mounted. Review probes were added only on the private tree.

Producer bytes are `git show c6f628a3a8c34c731704e2008d31bf00ec6a7f9a:evidence/b5ac4bc8-1872-4fe4-a56a-04a70f7103a2/DL01/mysql/import-proof/phases/pg-import-catalog.json`. Size 5757. sha256 `d9af109ea2f8cd63497771d960a8c396b959501b575b07aa9668fb04a61db844`. Protocol `oo.pipeline.sealed-import-catalog/v1`. Engine `mysql`, source id `mysqlpreview`, state `awaiting_publish`, capture `sealed_import`, generations 1 and 3. Every column `source_type` in that document is null. The compiler records `native_type` `absent` and keeps `target_type` as PostgreSQL storage.

Child lock `tools/source-semantics/Cargo.lock` has 117 registry packages and 0 versions outside the base lock. Pins present: `crypto-common 0.1.6`, `generic-array 0.14.9`, `js-sys 0.3.105`, `libc 0.2.189`, `wasm-bindgen 0.2.128`.

## Author recipe gap

The author sealed-import `HANDOFF.md` says to leave `contracts/schema/oos-v1.json` on the archive. That sentence is wrong for a schema-contract compile. The archive blob and the `b1ffb079` blob differ, as hashed above.

The author's decisive invocation `attempt4-invocation.txt` does not follow that sentence. It bind-mounts `/tmp/oo-dl01w1-05/overlay-schema` onto `/src/contracts/schema` without `readonly`. Current bytes at that host path are the `b1ffb079` schema, sha256 `ee134c23…`, 60068 bytes. The archive copy at `/tmp/oo-dl01w1-05/archive/contracts/schema/oos-v1.json` is still the 577 blob, sha256 `48432a16…`, 54432 bytes. `inside-attempt4.sh` runs the schema phase with `OO_WRITE_SCHEMAS=1` against that writable mount. The review did not copy that mode.

This review mounted the whole source tree read-only, overlaid the `b1ffb079` schema, and left `OO_WRITE_SCHEMAS` unset. Schema sha256 before and after the schema test were both `ee134c232f0d1f1e5ac3cdd435462ee15ed0f66a801dad7d7cdd32daf244f331`. The schema test exit was 0.

Author sealed-import attempt failures stay in the author packet and were not rewritten. Attempt 1 `semantics.exit` 101: `tests/sealed_import.rs:105` assertion `verified.contains("select id, amount from erp.parcel")` failed. Attempt 2 `semantics.exit` 0 and `clippy-core.exit` 101 (`collapsible_if` in `crates/oo-core/src/sealed_import.rs`). Attempt 3 `semantics.exit` 0, `clippy-core.exit` 0, and `clippy-builder.exit` 101 (`collapsible_if` in `crates/oo-builder/tests/sealed_import.rs`). Attempt 4 `test.exit` 0. `HOME` on that author run was `/tmp`, which is the 512m tmpfs. This review set `HOME=/target/home`.

## Compile gate

One Linux arm64 Rust 1.94 builder at a time for this prefix. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, already local, `--pull never`. Flags: `--cpus 1`, `--memory 6g`, `--memory-swap 6g`, `--pids-limit 512`, `--cap-drop ALL`, `CARGO_BUILD_JOBS=1`, `--security-opt no-new-privileges`. Labels `oo.project=open-ontology`, `oo.owner=dl01-w1-36`, `oo.session=025e07bb-e26b-4c0e-8b43-a80ac0f48dda`, `oo.kind=rust-builder`. Cargo used `--locked --offline`. `cargo update` was not run. No host `rustc` or `cargo` compiled this tree. No Postgres, Node, or browser ran.

Admission under `/tmp/oo-dl01-wave1-resources.lock`. First start saw 1 other running `oo.kind=rust-builder` and 0 of this prefix, available memory 26244235264 bytes. Second start, after the first container had exited, saw 2 other builders (`oo-dl01w1-38-rust`, `oo-dl01w1-39-rust`) and 0 of this prefix, available memory 26359152640 bytes. Both counts were under 10. Inspect: `Memory=6442450944`, `NanoCpus=1000000000`, `PidsLimit=512`, `OOMKilled=false`.

`rustc --version` printed `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `uname -m` printed `aarch64`. Those lines are not the compile.

`oo-dl01w1-36-b1` (`a4a58ce0051f40d262101648cee7cd8421684852b3530d744ff2c28a9a6e72e1`) started `2026-10-09T19:03:12Z` and finished `2026-10-09T19:06:29Z`. Docker inspect `ExitCode` is 1. Every product phase in that container exited 0, including the sealed-import tests and the independent probes. `clippy-tool` exited 101 because the review probe `tests/w36_sealed.rs` had a nested `if` (`collapsible_if`). That file is not product code. The phase logs from this container are in `rerun-1/phases/`.

`oo-dl01w1-36-b1b` (`1e60777ed51da290f475e1f81f94a791a8452223958f7f7024ac0a19cc7ae984`) started `2026-10-09T19:07:56Z` and finished `2026-10-09T19:08:17Z` after the probe `if` was collapsed. It reused the same private target. Docker inspect `ExitCode` is 0. `docker wait` stdout was `0`. This container is the decisive gate. Its phase logs are in `phases/`.

| Phase | Exit | Result on `oo-dl01w1-36-b1b` |
| --- | --- | --- |
| lock subset | 0 | 117 registry rows, outside 0 |
| schema contract, `OO_WRITE_SCHEMAS` unset | 0 | 1 passed; schema file bytes unchanged |
| `cargo test -p oo-core --lib source_semantics_contract` | 0 | 2 passed |
| `cargo test -p oo-core --lib sealed_import` | 0 | 3 passed |
| `cargo test -p oo-builder --test reviewed_effective` | 0 | 6 passed |
| `cargo test -p oo-builder --test role_pack` | 0 | 12 passed |
| `cargo test -p oo-builder --test sealed_import` | 0 | 5 passed |
| `cargo test --manifest-path tools/source-semantics/Cargo.toml` | 0 | author journey 7, author sealed 3, w36 8, w36 sealed 4 |
| `rustup component add clippy --toolchain 1.94.0` | 0 | component already present from the first container |
| `cargo clippy -p oo-core --lib --tests -- -D warnings` | 0 | finished in 0.37s |
| `cargo clippy -p oo-builder --lib --test sealed_import --test reviewed_effective --test role_pack -- -D warnings` | 0 | finished in 0.27s |
| `cargo clippy --manifest-path tools/source-semantics/Cargo.toml --all-targets -- -D warnings` | 0 | finished in 1.47s |
| container `test.exit` | 0 | follows those phases |

Debug binary `/target/debug/source-semantics` sha256 `8399e4d6e5124417f8f015eb504d54776f7ebd9931603747d1ebc528310c9e06`, size 106118216 bytes. The private target was removed after the hash was copied. The binary is not retained. The hash differs from the `5bd6` review binary because this overlay includes the sealed-import compiler.

`runs/docker-wait.exit` for the first launch is the Docker client status `0`. The container exit is the inspect `ExitCode` 1 recorded above. `runs/docker-wait-b1b.exit` is the `docker wait` stdout `0`, which matches inspect.

## Independent probes

Old PostgreSQL regressions used the previous review probe `harness/w36_independent.rs`: payer, invoice, region, slip, ticket, `shadow_log`, grades T/S1/S0/B, money column `fee_cents` unit USD. All 8 tests passed again on this overlay, including precision-loss key and cursor, the 980 thousandths rule, hidden names, native SQL, and the public CLI exits 0, 3, 2, 2, 1, and 1 (`probes/cli-packed.exit` through `probes/cli-field-missing.exit`).

Sealed probes are `harness/w36_sealed.rs`. Catalog bytes are the producer file above. Binding values are this review's: project `w36seal`, principal `nia`, role `oo_r_w36`, reviewer `nia.reviewer`, grade `S1`, foreign key `w36_units_fk`, example `w36-fee-kept`. They are compiler inputs.

| Probe | Result |
| --- | --- |
| `compile_captured_pg_catalog` on the producer bytes | `field.unknown`, message `catalog evidence is not oo.source.pgsql/v1` |
| catalog protocol `oo.source.mysql/v1` | `protocol.mismatch` at `/catalog/protocol`, no pack |
| `compile_sealed_import_catalog` | engine `mysql`, source `mysqlpreview`, four digests as in the producer file, generations 1 and 3, `postgresql_target_storage`, identity `33bb4e8b…` |
| `compile_reviewed_sealed_import` | effective protocol `oo.builder.effective-oos/v1`, `amount` semantic type `money` unit `USD`, provenance carries the same engine, digests, generations, and identity |
| `compile_role_pack` on that effective document, then again on `role_pack_inputs` | page and `examples/verified.sql` match; cache key contains `catalog=33bb4e8b…` |
| rendered page | `## 来源与语义`, `USD`, `consistency_grade: S1`, `postgresql_target_storage`, `tinyint1_as_int2`, `enum_as_text`, `parcel_key`, `w36_units_fk`, `declared_only`, `family: mysql`, `capture: sealed_import` |
| relationships versus semantics | `w36_units_fk` with rows 12 and ratio 980 is under `## 来源与语义` and absent from `## relationships` |
| README | `所有 SQL 为 PostgreSQL 方言，无论数据来自何种源库` |
| verified SQL | keeps `select amount from erp.parcel where amount is not null`; drops `NVL`; rejected id `w36-source-nvl` |
| hidden and shadow text | `blob_col`, `payload`, `json_not_jsonb`, `mysql_time_not_interval`, `empty_box`, `NVL`, `pc_`, `pn_`, `oo.source.pgsql/v1`, and the catalog shadow tokens are absent from the page, README, and verified SQL |
| `erp.parcel.id` | `native_type` `absent` |
| input digest first byte `8` to `a` with the old catalog version | `binding.catalog_version`, no pack |
| same altered catalog with the new identity | packed, identity `24563cf2bd58ac70592c7cdb2b5c9883abb6852691f1c14479c8b5b284bebe8b`, cache key contains the new hash and does not contain `33bb4e8b…` |
| settings or visibility `catalog_version` `cat-w36-stale` | `binding.catalog_version` |
| schema digest flipped | `digest.mismatch` at `/schema_digest` |
| state `retired` and state `unsealed` | `seal.unsealed` at `/state` |
| `config_digest`, `source_id` `w36preview`, `type_digest`, `input_digest`, `target_generation` 4, each with the old catalog version | identity changes; consumer returns `binding.catalog_version` |
| engine `oracle` with target family `mysql` | direct compile records engine `oracle` and `postgresql_target_storage`; consumer returns `provenance.conflict`; no pack |
| `source_generation` 0 | `seal.stale` at `/source_generation` |
| money unit removed | `oos.invalid` at `/columns/5/unit`, message `effective OOS validation failed` |
| unit `unknown` and `UNKNOWN` | `decision.invalid` |
| declared FK with no inclusion, ratio 979, and zero rows | blocked, pack null, builder call only `compile_sealed_import_catalog`, codes `declared_only_fk_untested`, `declared_only_fk_below_threshold`, `declared_only_fk_empty_sample` |
| key column oid 700 with a recomputed schema digest | `precision_loss_key`, pack null, catalog proof `refused` |
| cursor on `rate` with `precision_loss: true` | `precision_loss_cursor`, pack null, catalog proof `accepted` because the sealed key check does not own that cursor |
| CLI producer command | exit 0, stdout contains the identity hash and `postgresql_target_storage` |
| CLI precision-loss key | exit 3 |
| CLI state `retired` | exit 1, stderr `seal.unsealed` |

`gateway_get_freshness` on these results is `not_called`. `live_binary_sort_proof` is `not_run`. `source_connection` is `not_used`.

The author test `oracle_engine_uses_the_same_import_path` passed inside the same journey. It packs when the target family matches the catalog engine. That cache key is the new identity. It is the same compiler path with a recorded engine string, still `live_database` `not_connected`.
