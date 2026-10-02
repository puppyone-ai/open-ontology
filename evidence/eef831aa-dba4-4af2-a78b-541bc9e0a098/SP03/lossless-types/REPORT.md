# a098 PostgreSQL lossless types

Session `eef831aa-dba4-4af2-a78b-541bc9e0a098`. Local source commit only. No push, pull request, or release.

## Fixed source

| Field | Value |
| --- | --- |
| Commit | `3ca5357930a2c44c42d3061db5fabe577881269a` |
| Tree | `ec11cd3ee7948c33e6d43cd2e984e01b948f94e6` |
| Parent | `dd5ecf39604b8467a0d1f91d56d2e90473fa2c48` |
| Parent tree | `5dbc3d423707689dad53ee4ece2edfdcc8a23ab8` |
| Parent subject | jobs: record lease operations smoke and handoff |
| Subject | connectors: decode PostgreSQL values into Arrow 60 batches |
| Files | 17 files, 4596 insertions, 9 deletions |

The index was empty before staging. Staged paths were exactly the 17 paths below. `git diff dd5ecf39604b8467a0d1f91d56d2e90473fa2c48 HEAD -- Cargo.toml crates/oo-cli crates/oo-sdk ci deny.toml` is empty. Root `Cargo.toml` was not edited. `ci/deny.toml` was not edited. SDK, CLI, vendor, CI, pipeline, jobs, and source-admission files were not staged.

After the commit the worktree still has foreign unstaged edits that this commit does not contain:

- `contracts/pipeline/target-fence-receipts/README.md`
- `crates/oo-pipeline/migrations/README.md`
- `testing/harness/containment-supervisor/provenance/README.md`

## Baselines and manifests

| Record | Value |
| --- | --- |
| Source admission interface | `34fae5ddd73ff250fde6f51aa681fb0f7fbd0362` |
| Source admission tree | `c35b95664d53dcef1497bbd224ecbeec65586cea` |
| Root-writer handoff | `execution/decisions/a098-root-writer-handoff.md` sha256 `9d42781bff82a6c27fc95f5c0816fe96e39da027c0053b15234fa9e20a25f898` |
| Handoff-time HEAD named in that file | `b13650e403a64498955daf93bc25003f7c857aaa` |
| Lock named by that handoff | sha256 `40664b41001d01db59d272e0d9fab3977842becc88998bd52eefaa0c1b7a7147`, blob `887cef930ba2fdadf3541aded755dc594e5bd1d7` |
| Lock this commit adds | sha256 `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f`, blob `15ae1071c7236a956547e3f1d37d627b2cbebfce` |
| Older lock explicitly not restored | `b3a8ed3` |
| Catalog protocol | `oo.source.pgsql/v1` |
| Conversion protocol | `oo.source.pgsql.types/v1` |
| Numeric typmod version | `pg-numeric-typmod/v1` |
| Type policy version | `type-policy/v1` |
| Connector manifest | `crates/oo-connectors/Cargo.toml` pins `arrow-array`, `arrow-buffer`, and `arrow-schema` at `=60.0.0`, `default-features = false`. Dev-dependencies pin `arrow-array` and `arrow-schema` the same way. |
| Workspace rust-version | 1.94.0, unchanged |
| Crate lint | `forbid(unsafe_code)` remains. The new type modules and tests contain no `unsafe`. |

The committed `Cargo.lock` is the file the green `--locked` fetch copied out of the snapshot. Its sha256 matches `linux-smoke/verify/attempts/7-green/phases/Cargo.lock` and `linux-smoke/verify/attempts/3-record-batch-compile` lock. `cargo fetch --locked` exited 0 and did not rewrite it.

The snapshot for that run was `git archive` of parent `dd5ecf39604b8467a0d1f91d56d2e90473fa2c48` plus the a098 connector sources, contract, and the attempt-3 lock. `linux-smoke/verify/snapshot-head.txt` is that parent SHA.

## Type contract

`contracts/source/pgsql/type-conversion-v1.md` matches the implementation in this commit.

`ColumnPlan::accepts_result_oid` accepts the catalog oid. When `decode_oid` differs from the catalog oid, it also accepts that `decode_oid`. `plan_domain` walks `typbasetype` for at most 8 steps, rejects a missing base and a cycle, and requires the base to be lossless. The catalog oid stays the domain oid. Any other result oid fails the read. Array header element oids use the same rule. An unknown or wrong oid is not accepted.

`connector_capabilities()` stays all unsupported: `scan`, `bounded_read`, `cdc`, `type_conversion`, `registry_api`, `source_ddl`, `source_write`. `type_support.converted` is ignored. `type_capabilities()` is a separate list. Supported there: `pg_builtin_lossless`, `pg_signed_numeric_typmod`, `pg_domain_base`, `pg_enum_label`, `pg_array_lower_bound`, `pg_arrow_series60`, `pg_arrow_crate`. Unsupported there: `mysql_unsigned`, `position_codec`, `cdc`, `bounded_reader`, `loader_reject_persistence`, `user_defined_output`.

`TypedBatch::record_batch` builds an Arrow 60 `RecordBatch` with the production dependencies. A field is nullable when the catalog column is nullable or any cell is an Arrow null, including a masked reject in a catalog `NOT NULL` column. SQL NULL in a non-nullable column still fails the read before a batch exists. The canonical hash is the type-policy frame. Arrow buffers are not hashed.

The live test `live_lossless_types` passed against this builder. It checked two rows, Decimal128(10,-2) coefficient, Decimal256 little-endian bytes, exact utf8 decimals, float −0 versus +0, float4 NaN, float8 infinity, array nulls and lower bounds, a flat 2-dimensional list, domain `flag_domain` decoded as bool, json text versus compact jsonb, date/time/timestamp values, interval parts, masked rejects for numeric NaN and date infinity, and omission of money and composite columns. Catalog numeric scale for `numeric(10,-2)` stays 2046. Conversion scale is −2.

## Source hashes

Git blob ids are `git ls-tree` of the commit. SHA-256 is `git show HEAD:path`.

| Path | Blob | SHA-256 |
| --- | --- | --- |
| `Cargo.lock` | `15ae1071c7236a956547e3f1d37d627b2cbebfce` | `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f` |
| `contracts/source/pgsql/type-conversion-v1.md` | `a86eb4595a3d71743eb2a7fd81e9e48dcfcf20ae` | `c89aafc6859e079febef5e17693095cfc376cdd46578a0ab574abfcbf05fe25d` |
| `crates/oo-connectors/Cargo.toml` | `e8e3254da87a29ce90c485e8ad6566dbd4654f43` | `347f21f56657900c99939d505b72d1e60e8c5a2b7c60177e0d6ff8e009f3f2cf` |
| `crates/oo-connectors/src/lib.rs` | `73e5750656284b1018a82e8f73540f3ab95e668a` | `cb26b05d1acb2b03e60f048660a3b4af0329210c9465ea3ed29a8b1bef00c913` |
| `crates/oo-connectors/src/session.rs` | `8c833d83c79badfc01b56d598339048282758f5d` | `c68ba5b1965ad8e4ff618cf24c9a2b3b9993a9955a4137e6c8d0eb83860c3dd3` |
| `crates/oo-connectors/src/sql.rs` | `7dd548092de929bdfb2add9f2cb35eeeded55712` | `65f7f3703c5667237aa5ad26a5f351303d544f47dcac32fa5cdc8a974153bfa8` |
| `crates/oo-connectors/src/types/arrow_batch.rs` | `df9ca78cd5f6d588c7997e90bbb86dbd14c213a7` | `a281e0479536eedeb4b3f94b45b6ce845aeb5adf2be1afe2ba6ddd1f96e8b09f` |
| `crates/oo-connectors/src/types/batch.rs` | `5fb73a64875a0570125c23f4545fd10b639ee236` | `7883bdaa853dc2db82bee2ea1d334bf537bb7bcc1db45331989671e57ff5d2fe` |
| `crates/oo-connectors/src/types/binary.rs` | `f0c506fb48eadd913991d73eb9e6e9ade294e241` | `35fb94e82eceba79442c6816866706b60b6323bc0ac6aef70b878a360045b6ae` |
| `crates/oo-connectors/src/types/facts.rs` | `b7776d7e7a26a79425ecb2b48f273b825f3b25cd` | `291ffd565850fd2a2067c8848c379662e809a657733d3c3f2f7b4e0f2f816dbb` |
| `crates/oo-connectors/src/types/mapping.rs` | `f41173b978ab9bbd87a178d29704c6dd2710eb93` | `379eefc89805aeceb8759c675016e99948e9c93a9fc1cb0fd47cfaa1e20f5d79` |
| `crates/oo-connectors/src/types/mod.rs` | `5519776c52dff8560fc667f8baca7d1871aabffc` | `6b47c39cc93d34d9bef810b061e22e7f6caf51dd4377cab8fdddc26b2a648711` |
| `crates/oo-connectors/src/types/read.rs` | `131a101c03306e163e5c82bbc12e85de087e9b94` | `fc092922b73aae14624b773b69c6da79ebfc2f307c2bb06ddd62493850a46899` |
| `crates/oo-connectors/src/types/tests.rs` | `0b80698990fc027457d53b58dac7b7129a568cd8` | `fa9c12f4a59ffa50a304cfc62cffab06917d7f277456877b9c7f2cf3cb83b316` |
| `crates/oo-connectors/src/types/typmod.rs` | `e7ace650c7ec277146b319fc801e20d4fef1f9e9` | `947f345bb9ebe5fd18d916be045e5eae607e60aef77d712080c730c0f220f7a5` |
| `crates/oo-connectors/tests/lossless_live.rs` | `ccd367158840650c9c5818372cc2e002861e9db9` | `314099a516be81ec1303ab38d38ec3cf0f6e3d0c3300ad9150d82a06c8e239f8` |
| `crates/oo-connectors/tests/pg_wire_oracle.rs` | `0c02e41d26d2ff2414793ef936bae1bb862b7772` | `dbc5f61ed21fb4c87160acfe0692daa0de20495f163dfec179c770375ce6223d` |

## Oracle

The deciding oracle is Rust source `crates/oo-connectors/tests/pg_wire_oracle.rs`, included by `lossless_live.rs`. It reads `pg/out/oracle-raw.json`.

| File | SHA-256 | Role |
| --- | --- | --- |
| `pg/oracle.py` | `21605932302cf8466fb2c01f02caafc6199835f74febef4b14dcfc2d260e1c13` | Historical Python attempt. Preserved. |
| `pg/out/oracle.json` | `366f86d0f3bcae676cb3c94dcac67111179b638875b6bf35f074212ea909e6ad` | Historical Python output. |
| `pg/out/oracle-raw.json` | `79e106301a0c76cc88b12c824678eb117094634af2f1e21d0de348bdd810a2f4` | Raw send-hex input retained for the Rust oracle. |
| `pg/out/server-version.txt` | `f3ee60d66d4e1fcf95402c8ef40068343ddd03824e3036f329fb47ebc4329517` | `17.10 (Debian 17.10-1.pgdg12+1)` |

## Integrated run

Run id `b254bfa9-65d9-4209-9ac6-4aac2b2289b3`.

Rust container `oo-a098-msrv-b254bfa9-65d9-4209-9ac6-4aac2b2289b3`, image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64. State at receipt: exited 0, OOM false, started `2026-10-02T13:22:51.467052838Z`, finished `2026-10-02T13:23:10.971814583Z`. Toolchain inside the container: rustc 1.94.0 (`4a4ef493e 2026-03-02`), host `aarch64-unknown-linux-gnu`, Debian 13. Host rustc was not the acceptance compiler.

PostgreSQL container `oo-a098-pg-b254bfa9-65d9-4209-9ac6-4aac2b2289b3`, image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, started `2026-10-02T12:26:56.558549883Z`. The server log and `server-version.txt` both say PostgreSQL 17.10 (Debian 17.10-1.pgdg12+1) on aarch64. The image config version label was empty.

Commands from `linux-smoke/verify/inside.sh`, all with `RUSTUP_TOOLCHAIN=1.94.0` and the snapshot manifest:

1. `rustup component add clippy --toolchain 1.94.0`
2. `cargo fetch --manifest-path /src/Cargo.toml --locked`
3. `cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --lib -- --test-threads=8`
4. `cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --doc`
5. `cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors -- --test-threads=8`
6. `cargo clippy --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings`
7. `OO_ARROW_FIXTURE_OUT=/phases/arrow-fixture.json cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --lib types::tests::export_arrow_fixture -- --exact`
8. Isolated probe: `cargo generate-lockfile`, `cargo fetch --locked`, `cargo test --locked --offline`, `cargo clippy --locked --offline --all-targets -- -D warnings` on `/tmp/arrow-probe`
9. `OO_PG_TYPES_FIXTURE=/fixture/pointer.json cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --test lossless_live -- --ignored --test-threads=1`
10. `cargo tree -p oo-connectors --locked --offline --edges normal` and `--edges features`

Green phase integer exits, archived at `linux-smoke/verify/attempts/7-green/phases/`:

| Phase | Exit |
| --- | --- |
| clippy-component | 0 |
| fetch | 0 |
| test-lib | 0 |
| test-doc | 0 |
| test | 101 |
| clippy | 0 |
| arrow-export | 0 |
| arrow-lock | 0 |
| arrow-fetch | 0 |
| arrow-test | 0 |
| arrow-clippy | 0 |
| test-live | 0 |
| feature-graph | 0 |
| final | 0 |

Counts from `attempts/7-green/docker.log` sha256 `97b2122c7b217403d6c4c072d5bdf689bd991233dded5b8b189562677a611f46` and `docker.err` sha256 `ddaf52d05d7dc1aee0da3b840c7eb345a90a11f0cf789c9de8536e326d82d7ad`:

- Library tests: 45 passed, 0 failed.
- Doc tests: 0 passed, 0 failed, exit 0.
- Package test: the same 45 library tests passed, then `live_source_catalog` panicked at `crates/oo-connectors/tests/live_pg.rs:58` with `OO_PG_FIXTURE is required: NotPresent`. Exit 101. The script records that exit and continues. This is the preserved missing-fixture gap. It is unverified, and it is not a pass.
- Clippy `-D warnings`: exit 0.
- `export_arrow_fixture`: 1 passed. `arrow-fixture.json` sha256 `f581e5a2b58a2aa4769eee4050e00a760c501063a2f419a262c39208008424c9`.
- Arrow probe: 1 integration test passed. Probe library and doc targets reported 0 tests.
- `test live_lossless_types ... ok`. 1 passed, 0 failed, 0 ignored, finished in 0.17s. Log: `attempts/7-green/phases/test-live.log`.
- Feature graph exit 0. File sha256 `0f6949c054aa0b5d57e164e9dc104e73aa9ef263746a5bedac5af9ae0cc3fcc9`, 115225 bytes. It is `cargo tree -p oo-connectors` only.

A full workspace feature graph from before the lock update was not captured. The before graph of package versions is the parent lock. The edge diff is `attempts/7-green/lock-pin-diff.txt` sha256 `3d83ab5aee76a62d4a574a8f7cdfc64abbea34821bf6ad7a0ab4455105c2fe8a`.

## Retained failures

These directories were kept. The green archive is attempt 7 and does not replace them.

| Attempt | Final | What the retained log shows |
| --- | --- | --- |
| `1-arrow-compile` | 101 | Arrow probe `arrow-test` 101. `BinaryArray::from` does not implement `From<Vec<Option<Vec<u8>>>>`. Package `test` 101. Library tests 0. |
| `2-live-oid` | 101 | `test-live` 101. `read_typed` fact `type_oid_mismatch`. Package `test` 101 is the missing `OO_PG_FIXTURE` panic. Arrow probe tests 0. |
| `3-record-batch-compile` | 101 | `test-lib` 101. `E0599` no method `data_type` on `&PrimitiveArray<Decimal128Type>`. The resolved lock sha256 from this attempt is the lock that later passed and was committed. |
| `4-clippy` | 101 | `clippy` 101. Retained log cites `clippy::manual_repeat_n`. `test-lib` 0. Package `test` 101. |
| `5-live-arrow` | 101 | `test-live` 101. `record_batch` fact `arrow_batch` after the wire assertions. Package `test` 101. Library, clippy, and arrow probe 0. |
| `6-reject-debug` | 101 | `test-live` 101. Assertion `!format!("{nan:?}").contains("sample")` failed. The table name is `sample`. Package `test` 101. RecordBatch construction had already succeeded on this attempt. |

Baseline phase exits under `linux-smoke/baseline/phases/`, from the earlier focused run, were not re-executed: clippy-component 0, clippy 0, fetch 0, test-doc 0, test-lib 0, test 101, final 101.

## Dependency delta

Parent lock sha256 `40664b41001d01db59d272e0d9fab3977842becc88998bd52eefaa0c1b7a7147`. Committed lock sha256 `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f`. Zero packages removed. Nineteen packages added. Existing package blocks that changed:

- `oo-connectors` 0.1.0 gained `arrow-array`, `arrow-buffer`, `arrow-schema`.
- `ahash` 0.8.12 gained optional dependency `const-random`. Version and checksum of ahash are unchanged. The linux `oo-connectors` tree shows ahash feature `runtime-rng`. It does not list `const-random` or `tiny-keccak`. The lock still records that edge because arrow-array enables ahash `compile-time-rng` for wasm32.
- `chrono` 0.4.45 gained `iana-time-zone` and `windows-link`. The after feature tree shows `arrow-schema` enabling chrono feature `clock`, which enables `iana-time-zone`. `windows-link` was already a locked package.
- `num-traits` 0.2.19 gained `libm`.
- `fraction` 0.17.0, `num` 0.4.3, and `num-rational` 0.4.2 now name `num-bigint` 0.4.8 explicitly because 0.5.1 was added.
- `oo-cli` 0.1.0, `oo-sdk` 0.1.0, `oo-core` 0.1.0, `oo-pipeline` 0.1.0, and `oo-jobs` 0.1.0 package blocks are identical to the parent lock.

Provenance for the 19 additions is `attempts/7-green/provenance.json` sha256 `2e9400d22da23cae127d6bd115c46e9844a9cafd188178b4538a6f72c335b77b`. Source was the crates.io API plus GitHub advisory-db contents. Cutoff for 168 hours before session date 2026-10-02 is 2026-09-25. Every row has `age_ok_168h` true, `yanked` false, and `checksum_match` true. Lock checksums:

| Crate | Version | Created | MSRV | License | Checksum |
| --- | --- | --- | --- | --- | --- |
| android_system_properties | 0.1.6 | 2026-08-05 | none published | MIT OR Apache-2.0 | `ae221649c9976a6f6c56ae1facf410f3ddb33cc661c4b7b61020a912d4237fbc` |
| arrow-array | 60.0.0 | 2026-09-15 | 1.88 | Apache-2.0 AND MIT | `51dff6e4b9f158864a0aeb6a131ad53858ecbae7a2fc8989307b9b3c6b2f122e` |
| arrow-buffer | 60.0.0 | 2026-09-15 | 1.88 | Apache-2.0 | `c7235b863533e9db3ab86905b4521251de11226275b436ff3cb2d0010558d1c5` |
| arrow-data | 60.0.0 | 2026-09-15 | 1.88 | Apache-2.0 | `5aa06d5e22786c4966ffc4d2f171d0cb65d95f6bdea108a14e8c335e9faa437f` |
| arrow-schema | 60.0.0 | 2026-09-15 | 1.88 | Apache-2.0 | `f85a9a58cd03560526b0066487574aacb9008e413504fa0ee23d61141e96c338` |
| const-random | 0.1.18 | 2024-03-03 | none published | MIT OR Apache-2.0 | `87e00182fe74b066627d63b85fd550ac2998d4b0bd86bfed477a0ae4c7c71359` |
| const-random-macro | 0.1.16 | 2023-10-23 | none published | MIT OR Apache-2.0 | `f9d839f2a20b0aee515dc581a6172f2321f96cab76c1a38a4c584a194955390e` |
| crunchy | 0.2.4 | 2025-06-25 | none published | MIT | `460fbee9c2c2f33933d720630a6a0bac33ba7053db5344fac858d4b8952d77d5` |
| half | 2.7.1 | 2025-10-14 | 1.81 | MIT OR Apache-2.0 | `6ea2d84b969582b4b1864a92dc5d27cd2b77b622a8d79306834f1be5ba20d84b` |
| iana-time-zone | 0.1.65 | 2026-01-28 | 1.62.0 | MIT OR Apache-2.0 | `e31bc9ad994ba00e440a8aa5c9ef0ec67d5cb5e5cb0cc7f8b744a35b389cc470` |
| iana-time-zone-haiku | 0.1.2 | 2023-05-12 | none published | MIT OR Apache-2.0 | `f31827a206f56af32e590ba56d5d2d085f558508192593743f16b2306495269f` |
| libm | 0.2.16 | 2026-01-24 | 1.63 | MIT | `b6d2cec3eae94f9f509c767b45932f1ada8350c4bdb85af2fcab4a3c14807981` |
| num-bigint | 0.5.1 | 2026-07-05 | 1.60 | MIT OR Apache-2.0 | `93e7820bc0a80a0238e650327316f929ba18d5be054b647490a3a6a339f3e7c0` |
| tiny-keccak | 2.0.2 | 2020-04-01 | none published | CC0-1.0 | `2c9d3793400a45f954c52e73d068316d76b6f4e36977e3fcebb13a2721e80237` |
| windows-core | 0.62.2 | 2025-10-06 | 1.82 | MIT OR Apache-2.0 | `b8e83a14d34d0623b51dce9581199302a221863196a1dde71a7663a4c2be9deb` |
| windows-implement | 0.60.2 | 2025-10-06 | 1.74 | MIT OR Apache-2.0 | `053e2e040ab57b9dc951b72c264860db7eb3b0200ba345b4e4c3b14f67855ddf` |
| windows-interface | 0.59.3 | 2025-10-06 | 1.74 | MIT OR Apache-2.0 | `3f316c4a2570ba26bbec722032c4099d8c8bc095efccdc15688708623367e358` |
| windows-result | 0.4.1 | 2025-10-06 | 1.82 | MIT OR Apache-2.0 | `7781fa89eaf60850ac3d2da7af8e5242a5ea78d1a11c49bf2910bb5a73853eb5` |
| windows-strings | 0.5.1 | 2025-10-06 | 1.82 | MIT OR Apache-2.0 | `7837d08f69c77cf6b07689544538e017c1bfcf57e34b4c0ff58e6c2cd3b37091` |

`iana-time-zone` 0.1.65 has RUSTSEC-2022-0049 in the advisory file. It is informational/unsound, affected `>0.1.42, <0.1.45`, patched `>=0.1.45`. Version 0.1.65 is outside the affected range. The other 18 lookups returned HTTP 404 from that GitHub contents request. A 404 from that request is not a completed advisory scan.

`ci/deny.toml` allows Apache-2.0, MIT, ISC, BSD-3-Clause, Unicode-3.0, Zlib, Unlicense, and MIT-0. Yanked is deny. `multiple-versions` is warn. `tiny-keccak` 2.0.2 is CC0-1.0, which is outside that allow list. It is present only as the wasm optional edge described above. `num-bigint` 0.4.8 remains beside 0.5.1, which the written policy would warn on. `cargo-deny` was not run. It was not installed. The license fact is known from crates.io metadata. The scanner gate is unverified. Policy was not relaxed and `deny.toml` was not edited.

No new first-party C helper was added.

## SQL audit

Collected before cleanup. Raw logs are retained because both password hit counts are 0. Statement bodies are not copied into this report.

| Log | Bytes | Lines | SHA-256 |
| --- | --- | --- | --- |
| `attempts/7-green/sql/postgresql.log` | 4099 | 31 | `f6140dac3436eab81cdcfa88ddd502efc8dde83382ff450420e148e9d56e62e6` |
| `attempts/7-green/sql/pg-docker.log` | 1768 | 41 | `abcbaa3957c9bce347942c2ba45696878577b663c36c0b5844e7ec51972b0d1d` |

Count file `audit-file.json`: reader password hits 0, admin password hits 0, comment canary hits 0, token canary hits 0, reader app lines 4, `reader_verbs` empty, `reader_other_verbs` empty, forbidden ident lines 0, `keep_raw` true.

Docker-log count file `audit-docker.json`: the same password, canary, verb, and forbidden counts are 0, reader app lines 0, 41 lines, `keep_raw` true.

The four reader lines in the file log are `could not receive data from client: Connection reset by peer` for `app=oo-pg-source` `user=oo_type_reader` `db=typedb` at 12:35:43.200, 13:10:52.213, 13:22:06.421, and 13:23:10.753 UTC on 2026-10-02. There is no `STATEMENT` line for that app. A SIGHUP at 12:27:01.942 UTC logged that `max_connections` and `shared_buffers` cannot be changed without restart. One further reload line is only in the raw log. One `ERROR` at 12:47:20.115 UTC is `app=psql` `user=postgres` `db=typedb`, syntax error at character 71, with a redacted `STATEMENT` line of length 76 in the raw log.

Reader SELECT and ROLLBACK verbs are absent from both logs. Zero-write behavior is not proven by this audit. Canary hits of 0 mean those strings were not present in the logged text. They do not show that the canaries were selected.

Copies also live under `pg/audit/`.

## Remaining limitations

These are explicit unverified or out-of-scope items. They are not waivers.

- Package test exit 101. `live_pg.rs` requires `OO_PG_FIXTURE`, which this run did not set. The missing catalog live test was not executed.
- `cargo-deny` was not run. `tiny-keccak` 2.0.2 is CC0-1.0 and sits outside the allow list. Dual `num-bigint` would be a warning under the current policy. Advisory coverage is the single RUSTSEC file above plus HTTP 404s.
- A before/after `cargo tree` of the whole workspace was not captured. CLI and SDK package blocks and manifests are unchanged. Chrono feature unification from arrow-schema is in the lock.
- SQL statement logging did not record reader verbs. Zero writes are not proven from the log.
- Bounded reader, Loader, CDC, MySQL unsigned, PositionCodec, user-defined output, and publish are out of this slice. `connector_capabilities()` still reports them unsupported.
- SP03-W04 and full AT-SRC-03/05 are not signed by this child.
- The isolated arrow probe is supporting evidence. The production proof is the `oo-connectors` library tests, `export_arrow_fixture`, and `live_lossless_types` on the workspace lock.

## Owned cleanup

Done after the green phases, both raw PostgreSQL logs, audit counts, provenance, and hash inventory were copied into `attempts/7-green/`.

Removed:

- container `oo-a098-msrv-b254bfa9-65d9-4209-9ac6-4aac2b2289b3`
- container `oo-a098-pg-b254bfa9-65d9-4209-9ac6-4aac2b2289b3` after stop
- network `oo-a098-pg-b254bfa9-65d9-4209-9ac6-4aac2b2289b3-net`
- volumes `oo-a098-cargo-b254bfa9-65d9-4209-9ac6-4aac2b2289b3`, `oo-a098-target-b254bfa9-65d9-4209-9ac6-4aac2b2289b3`, `oo-a098-pg-b254bfa9-65d9-4209-9ac6-4aac2b2289b3-data`, `oo-a098-pg-b254bfa9-65d9-4209-9ac6-4aac2b2289b3-log`
- `/tmp/oo-a098-b254bfa9-65d9-4209-9ac6-4aac2b2289b3`
- `/tmp/a098-pg-log.bin`, `/tmp/a098-pg-docker.log`, `/tmp/a098-sql-audit.json`, `/tmp/a098-sql-audit-docker.json`, `/tmp/a098-provenance.json`
- `/tmp/oo-a098-msrv-watch.sh`, `/tmp/oo-a098-msrv-watch.log`, `/tmp/oo-a098-verify-run.txt` because they named this run id

Left in place, because they belong to the earlier baseline run `e45a72cf-3b73-4708-ad21-5328559ed0b8`:

- container `oo-a098-msrv-e45a72cf-3b73-4708-ad21-5328559ed0b8`, exited 101
- volumes `oo-a098-cargo-e45a72cf-3b73-4708-ad21-5328559ed0b8` and `oo-a098-target-e45a72cf-3b73-4708-ad21-5328559ed0b8`
- network `oo-a098-net-e45a72cf-3b73-4708-ad21-5328559ed0b8`
- `/tmp/oo-a098-e45a72cf-3b73-4708-ad21-5328559ed0b8`, `/tmp/oo-a098-baseline-root.txt`, `/tmp/oo-a098-baseline-uuid.txt`

No prune. No foreign container, volume, or network was removed.

This report is evidence. It is not part of commit `3ca5357930a2c44c42d3061db5fabe577881269a`.
