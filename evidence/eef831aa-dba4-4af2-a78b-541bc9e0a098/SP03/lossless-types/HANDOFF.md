# a098 handoff

Review the fixed source commit below. This file is evidence and is not in that commit. Full tables, hashes, commands, and audit notes are in `REPORT.md` beside this file.

## Commit

| Field | Value |
| --- | --- |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a098` |
| Commit | `3ca5357930a2c44c42d3061db5fabe577881269a` |
| Tree | `ec11cd3ee7948c33e6d43cd2e984e01b948f94e6` |
| Parent | `dd5ecf39604b8467a0d1f91d56d2e90473fa2c48` |
| Parent tree | `5dbc3d423707689dad53ee4ece2edfdcc8a23ab8` |
| Subject | connectors: decode PostgreSQL values into Arrow 60 batches |
| Source admission | `34fae5ddd73ff250fde6f51aa681fb0f7fbd0362` / tree `c35b95664d53dcef1497bbd224ecbeec65586cea` |
| Catalog | `oo.source.pgsql/v1` unchanged |
| Conversion | `oo.source.pgsql.types/v1` |
| Lock before | sha256 `40664b41001d01db59d272e0d9fab3977842becc88998bd52eefaa0c1b7a7147`, blob `887cef930ba2fdadf3541aded755dc594e5bd1d7` |
| Lock committed | sha256 `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f`, blob `15ae1071c7236a956547e3f1d37d627b2cbebfce` |
| Handoff grant | `execution/decisions/a098-root-writer-handoff.md` sha256 `9d42781bff82a6c27fc95f5c0816fe96e39da027c0053b15234fa9e20a25f898` |

The committed lock is the file the green Linux 1.94 `--locked` fetch used. The older `b3a8ed3` lock was not restored. Root `Cargo.toml`, `crates/oo-cli`, `crates/oo-sdk`, `ci/deny.toml`, vendor, pipeline, jobs, and source admission were not in the commit. `oo-cli` and `oo-sdk` package blocks in the lock are byte-identical to the parent. Nineteen packages were added and none were removed. Shared edges that changed are `ahash` 0.8.12 (`const-random`), `chrono` 0.4.45 (`iana-time-zone`, `windows-link`), `num-traits` 0.2.19 (`libm`), and explicit `num-bigint` 0.4.8 on `fraction`, `num`, and `num-rational`. `oo-connectors` gained `arrow-array`, `arrow-buffer`, and `arrow-schema` 60.0.0.

Foreign unstaged worktree edits were left untouched: `contracts/pipeline/target-fence-receipts/README.md`, `crates/oo-pipeline/migrations/README.md`, and `testing/harness/containment-supervisor/provenance/README.md`.

## Contract

Production path is SQLx binary decode, canonical type-policy values, then `TypedBatch::record_batch` using arrow-array, arrow-buffer, and arrow-schema 60.0.0 with default features off. `pg_arrow_crate` is supported on `type_capabilities()`. `connector_capabilities()` remains entirely unsupported, including `type_conversion`.

A result oid is accepted when it is the catalog oid, or the verified base `decode_oid` after `plan_domain` walks a lossless base chain. Any other oid fails. Arrays, nulls, exact decimals, metadata, row preservation, and bounds are covered by the library tests and by the live test that passed. A field is nullable when the catalog column is nullable or a masked reject becomes an Arrow null. SQL NULL in a non-nullable column still fails the read.

The deciding oracle is `crates/oo-connectors/tests/pg_wire_oracle.rs` sha256 `dbc5f61ed21fb4c87160acfe0692daa0de20495f163dfec179c770375ce6223d`, blob `0c02e41d26d2ff2414793ef936bae1bb862b7772`. Raw input `pg/out/oracle-raw.json` sha256 `79e106301a0c76cc88b12c824678eb117094634af2f1e21d0de348bdd810a2f4`. Historical Python `pg/oracle.py` sha256 `21605932302cf8466fb2c01f02caafc6199835f74febef4b14dcfc2d260e1c13` and `pg/out/oracle.json` sha256 `366f86d0f3bcae676cb3c94dcac67111179b638875b6bf35f074212ea909e6ad` stay identified as the past attempt.

## Green run

Container `oo-a098-msrv-b254bfa9-65d9-4209-9ac6-4aac2b2289b3` exited 0, OOM false, `2026-10-02T13:22:51.467052838Z` to `2026-10-02T13:23:10.971814583Z`. Image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. rustc 1.94.0 aarch64-unknown-linux-gnu. PostgreSQL image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, server `17.10 (Debian 17.10-1.pgdg12+1)`.

Phase exits: clippy-component 0, fetch 0, test-lib 0 (45 passed), test-doc 0, test 101, clippy 0, arrow-export 0, arrow-lock 0, arrow-fetch 0, arrow-test 0, arrow-clippy 0, test-live 0 (`live_lossless_types` ok, 0.17s), feature-graph 0, final 0.

Archive: `linux-smoke/verify/attempts/7-green/`. Docker stdout sha256 `97b2122c7b217403d6c4c072d5bdf689bd991233dded5b8b189562677a611f46`.

## Retained failure attempts

| Attempt | Final | Retained cause |
| --- | --- | --- |
| 1-arrow-compile | 101 | `BinaryArray::from` trait bound. First Arrow compile failure. |
| 2-live-oid | 101 | live fact `type_oid_mismatch`. Second, domain OID failure. |
| 3-record-batch-compile | 101 | `E0599` `data_type`. Lock later committed comes from this resolution. |
| 4-clippy | 101 | clippy `-D warnings`, including `manual_repeat_n`. |
| 5-live-arrow | 101 | live fact `arrow_batch`. |
| 6-reject-debug | 101 | assertion expected the debug text to omit `sample`; the table is named `sample`. |

Package `test` exit 101 on the green run, and on attempts 1, 2, 4, 5, and 6, is `OO_PG_FIXTURE is required` in `live_pg.rs`. That gap stays unverified.

## Dependency gate

All 19 new crates are older than 168 hours, not yanked, and checksum-matched. Details and checksums are in `REPORT.md` and `attempts/7-green/provenance.json` sha256 `2e9400d22da23cae127d6bd115c46e9844a9cafd188178b4538a6f72c335b77b`.

`tiny-keccak` 2.0.2 is CC0-1.0. `ci/deny.toml` does not allow CC0-1.0. The crate is the wasm `ahash` `compile-time-rng` edge recorded in the lock. The linux `oo-connectors` tree does not compile it. `cargo-deny` was not run. This gate is unverified and the license is outside the written allow list. Policy files were not changed. RUSTSEC-2022-0049 does not affect `iana-time-zone` 0.1.65. Other advisory lookups in that provenance file are HTTP 404.

## SQL audit

File log sha256 `f6140dac3436eab81cdcfa88ddd502efc8dde83382ff450420e148e9d56e62e6`, 4099 bytes, 31 lines. Docker log sha256 `abcbaa3957c9bce347942c2ba45696878577b663c36c0b5844e7ec51972b0d1d`, 1768 bytes, 41 lines. Password hits 0. Canary hits 0. `reader_verbs` empty. Four `oo-pg-source` lines are connection resets and contain no statement text. Zero-write verbs are not proven. Raw logs are under `attempts/7-green/sql/` and `pg/audit/`.

## Cleanup

Removed this run's Rust container, PostgreSQL container, network, four volumes, and `/tmp` snapshot, secrets, audit copies, and watch files named `b254bfa9`. No prune. Baseline run `e45a72cf-3b73-4708-ad21-5328559ed0b8` container, volumes, network, and tmp directory remain.

## Not signed

SP03-W04, full AT-SRC-03/05, bounded reader, Loader, CDC, MySQL unsigned, PositionCodec, publish, and a passed `cargo-deny` scan are not signed. Stop here for a separate review of commit `3ca5357930a2c44c42d3061db5fabe577881269a`.
