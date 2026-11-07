# C-S3b: partial, 2026-11-04

S3-C-02 and S4-C-03 are **not done**. Rebased the four inherited commits onto
integration base `76b492b29`. No push, PR, service restart, customer access,
shared engine edit or lockfile change was performed.

## Reproducible exits

```sh
CARGO_TARGET_DIR=target cargo build --manifest-path tools/oo-load/Cargo.toml --locked
python3 testing/engines/mariadb/run.py --family mariadb --run-id c-s3b-mariadb-final-20261104
python3 testing/engines/mariadb/run.py --family tidb --run-id c-s3b-tidb-final-20261104
python3 testing/engines/mariadb/run.py --family oceanbase --run-id c-s3b-oceanbase-final-20261104
# Each family also ran with --loader-snapshot and run-id c-s3b-<family>-load-final-20261104.
```

Run IDs already exist; use fresh IDs when reproducing. All raw outputs are
ignored under `evidence/`. The standard mode tests the connector, not oo-load.

| Engine/version | Connector probe, six-table snapshot and C5 | Static oo-load/Agent DB |
|---|---|---|
| `mariadb:10.6.28` | passed; primary ROW C1 also passed | five tables zero differences; mandatory typed table **failed** |
| `mariadb:10.11.19` | passed; primary ROW C1 also passed | five tables zero differences; mandatory typed table **failed** |
| `mariadb:11.4.13` | passed; primary ROW C1 also passed | five tables zero differences; mandatory typed table **failed** |
| `mariadb:11.8.9` | passed; primary ROW C1 also passed | five tables zero differences; mandatory typed table **failed** |
| `pingcap/tidb:v7.5.0` | passed | five tables zero differences; mandatory typed table **failed** |
| `pingcap/tidb:v8.5.0` | passed | five tables zero differences; mandatory typed table **failed** |
| `oceanbase/oceanbase-ce:4.3.5.4-104000042025090916` | passed | five tables zero differences; mandatory typed table **failed** |

The source tests assert exact Arrow equality against independent whole-table
binary-protocol reads, including DECIMAL(30,8), u64::MAX, JSON, binary data,
negative TIME(6), NULLs, composite/text keys, no-key duplicates and empty tables.
C5 covers insert/update/delete/primary-key changes and no-change cycles.
MariaDB additionally tests primary-only C1 file-position resume, post/pre-image
equality, retained-log timestamps, no-change batches and DDL rebaseline refusal.
TiDB uses single-node unistore, not TiKV/PD. OceanBase CE 4.3.5.4 successfully
runs on this host under amd64 emulation: it is **not** a memory/emulation blocker.
The OceanBase native-plan adapter currently refuses secondary unique indexes.

## Why the card cannot exit in this write scope

1. Shared `crates/oo-pipeline/src/engine_load_pg.rs::column` omits
   `duration_micros`, although the existing native COPY encoder supports it.
   All seven real oo-load runs fail the typed table with `load_unsupported_type`.
   `shared-loader.patch` contains the exact coordinator proposal (unapplied;
   `git apply --check` passes). It still needs the coordinator's tests and live
   typed rerun; this report does not treat the proposed patch as validated code.
2. `tools/oo-load/src/main.rs` rejects non-PostgreSQL `--sync` and `--c5` with
   `sync_engine_not_integrated`. Six real CLI negative probes (both modes for
   all three families) are retained in `c-s3b-20261104/incremental-dispatch/`.
   This card's smallest family-arm grant was used for snapshot dispatch only.
   The missing durable family C1/C5 coordinators cannot be supplied by routing
   into the PostgreSQL-specific adapters: they use PG lineage, slots and WAL
   feedback. No silent snapshot fallback or unconditional baseline-alignment
   assertion was added. Coordinator integration is still required.
3. The static target fixture is intentionally not concurrent-writer T1,
   `oo doctor` T0, three-cycle Agent DB T2 or T8. Target tuple bytes are recorded,
   but reader connection peaks and measured duty cycles are not established.
   No full T0-T2/T8 acceptance or matrix promotion is claimed.

## Remaining mysql-preview paths

No preview CLI is silently redirected or declared family-compatible:

- `map.rs`, `values.rs`, `version.rs` already re-export connector semantics.
  `source.rs` already uses connector connect/inspect/catalog, ReadConnection
  and TableReader; those source primitives are not reimplemented here.
- `source_sql.rs`, `full.rs`, `customer.rs` retain legacy MySQL-specific
  admission/configuration (including MariaDB refusals and fixture plaintext
  checks). They are not a family engine registry. The new explicit family
  snapshot path is oo-load, not a widened legacy admission policy.
- `c1.rs`, `c1_chunked.rs` retain MySQL UUID/SID GTID streaming, window repair,
  spool and materialization protocols. MariaDB domain/server/sequence GTIDs
  cannot be substituted into these receipts; TiDB/OceanBase do not inherit C1.
- `c5plus.rs`, `c5_layered.rs` retain legacy target overlays, checkpoint and
  handoff/publication contracts. `target.rs`, fixture/admin writers and sealed
  import orchestration are consumer code, not read-only connector code.
  Their migration requires preserving those durable contracts and separate
  end-to-end regression, not changing module imports. They remain follow-ups.

## Validation and preserved failures

- `cargo check --workspace --locked`: passed before each change commit.
- `cargo fmt -p oo-connectors` and oo-load manifest formatting ran; unrelated
  formatting was left unchanged.
- Connector final suite: 324 passed, 31 ignored, 1 explicitly filtered base
  failure. Full initial run: 323 passed, 31 ignored, 1 failure.
- The failure is `parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`.
  Reproduced on an archive of base `76b492b29` inside ignored evidence:
  group_signal `rejected`, expected `sent`. Base command/log is in
  `c-s3b-20261104/base-failure.log`; no production test was weakened.
- `CARGO_TARGET_DIR=target cargo test --manifest-path tools/oo-load/Cargo.toml --locked`:
  7 passed, 6 ignored; includes `mysql_family_dispatch_is_explicit`.
- `python3 testing/engines/mariadb/test_runner.py`: 4 passed after correcting
  the pin-test regex to admit the hyphen in `oceanbase-ce`.
- Correction: commit `69e9eb720` prematurely described those four tests as
  passing because its shell continued after a failing assertion. The original
  failure is retained in `pin-test-regex-failure.log`. The pin digests and
  workspace build were valid; commit `c9f83f946` records the correction.
- Earlier failures remain intact: temporary Docker refusal and image TLS
  bad-record-MAC pulls; the oracle's text/binary protocol mismatch; stale base
  build artifacts sharing the target directory (resolved by a forced source
  rebuild); and OceanBase native EXPLAIN/empty primary scan mismatches.

Every source/target was freshly named and lane-C-labelled. The runner now
requires 20 GiB before pull and again before container creation. No more than
two task database containers ran at once. All task containers were removed;
`c-s3b-20261104/cleanup-audit.json` independently verifies none remain.

## Evidence digests

The following hashes identify raw results and per-run source manifests, not
committed data. Earlier raw attempts are retained and never overwritten.

| Evidence path (under evidence/) | SHA-256 |
|---|---|
| `c-s3b-mariadb-final-20261104/results.json` | `b557bee0cbe8b1c49711eee314ac9c3dc623226c015b9531d76d8e976c144152` |
| `c-s3b-mariadb-final-20261104/mariadb-10.11.19/source-manifest.json` | `35fda5ad3f1df327547e012fac582d2fc148f8ade68195e3007ada5f851b6e4c` |
| `c-s3b-mariadb-final-20261104/mariadb-10.6.28/source-manifest.json` | `35fda5ad3f1df327547e012fac582d2fc148f8ade68195e3007ada5f851b6e4c` |
| `c-s3b-mariadb-final-20261104/mariadb-11.4.13/source-manifest.json` | `35fda5ad3f1df327547e012fac582d2fc148f8ade68195e3007ada5f851b6e4c` |
| `c-s3b-mariadb-final-20261104/mariadb-11.8.9/source-manifest.json` | `35fda5ad3f1df327547e012fac582d2fc148f8ade68195e3007ada5f851b6e4c` |
| `c-s3b-mariadb-load-final-20261104/results.json` | `cfc7f33bc49a77adbc2ac93bf61a173c099b7c9ae393dca166243ff06ed5bdc9` |
| `c-s3b-mariadb-load-final-20261104/mariadb-10.11.19/source-manifest.json` | `35fda5ad3f1df327547e012fac582d2fc148f8ade68195e3007ada5f851b6e4c` |
| `c-s3b-mariadb-load-final-20261104/mariadb-10.6.28/source-manifest.json` | `35fda5ad3f1df327547e012fac582d2fc148f8ade68195e3007ada5f851b6e4c` |
| `c-s3b-mariadb-load-final-20261104/mariadb-11.4.13/source-manifest.json` | `35fda5ad3f1df327547e012fac582d2fc148f8ade68195e3007ada5f851b6e4c` |
| `c-s3b-mariadb-load-final-20261104/mariadb-11.8.9/source-manifest.json` | `35fda5ad3f1df327547e012fac582d2fc148f8ade68195e3007ada5f851b6e4c` |
| `c-s3b-mariadb-load-final-20261104/mariadb-10.11.19/loader/result.json` | `e3296960ab7bbf7eab54cffe4a5155e674b541849d27509c07b237c51590284e` |
| `c-s3b-mariadb-load-final-20261104/mariadb-10.6.28/loader/result.json` | `e3296960ab7bbf7eab54cffe4a5155e674b541849d27509c07b237c51590284e` |
| `c-s3b-mariadb-load-final-20261104/mariadb-11.4.13/loader/result.json` | `e3296960ab7bbf7eab54cffe4a5155e674b541849d27509c07b237c51590284e` |
| `c-s3b-mariadb-load-final-20261104/mariadb-11.8.9/loader/result.json` | `e3296960ab7bbf7eab54cffe4a5155e674b541849d27509c07b237c51590284e` |
| `c-s3b-tidb-final-20261104/results.json` | `d38e765512362d0beae900d4ddccbcc95f28b95a30d6bfe77f6852671c7042ca` |
| `c-s3b-tidb-final-20261104/pingcap-tidb-v7.5.0/source-manifest.json` | `35fda5ad3f1df327547e012fac582d2fc148f8ade68195e3007ada5f851b6e4c` |
| `c-s3b-tidb-final-20261104/pingcap-tidb-v8.5.0/source-manifest.json` | `35fda5ad3f1df327547e012fac582d2fc148f8ade68195e3007ada5f851b6e4c` |
| `c-s3b-tidb-load-final-20261104/results.json` | `102dcda5ff3036e682450f0bb2c715d6ff83a164fc939f917b6a77e8b5c8d67f` |
| `c-s3b-tidb-load-final-20261104/pingcap-tidb-v7.5.0/source-manifest.json` | `35fda5ad3f1df327547e012fac582d2fc148f8ade68195e3007ada5f851b6e4c` |
| `c-s3b-tidb-load-final-20261104/pingcap-tidb-v8.5.0/source-manifest.json` | `35fda5ad3f1df327547e012fac582d2fc148f8ade68195e3007ada5f851b6e4c` |
| `c-s3b-tidb-load-final-20261104/pingcap-tidb-v7.5.0/loader/result.json` | `e3296960ab7bbf7eab54cffe4a5155e674b541849d27509c07b237c51590284e` |
| `c-s3b-tidb-load-final-20261104/pingcap-tidb-v8.5.0/loader/result.json` | `e3296960ab7bbf7eab54cffe4a5155e674b541849d27509c07b237c51590284e` |
| `c-s3b-oceanbase-final-20261104/results.json` | `ff650b8c5cf0c4c9bb44223544285b8a424afc10083b4de690dd5eb1e7cd61cb` |
| `c-s3b-oceanbase-final-20261104/oceanbase-oceanbase-ce-4.3.5.4-104000042025090916/source-manifest.json` | `35fda5ad3f1df327547e012fac582d2fc148f8ade68195e3007ada5f851b6e4c` |
| `c-s3b-oceanbase-load-final-20261104/results.json` | `dd8dfc007997f2302644121a26d3fe6ef489b5c7910e79c7c8c23bff31f0202e` |
| `c-s3b-oceanbase-load-final-20261104/oceanbase-oceanbase-ce-4.3.5.4-104000042025090916/source-manifest.json` | `35fda5ad3f1df327547e012fac582d2fc148f8ade68195e3007ada5f851b6e4c` |
| `c-s3b-oceanbase-load-final-20261104/oceanbase-oceanbase-ce-4.3.5.4-104000042025090916/loader/result.json` | `e3296960ab7bbf7eab54cffe4a5155e674b541849d27509c07b237c51590284e` |
| `c-s3b-20261104/cleanup-audit.json` | `2cc1604c20e7df747d6a2a1705ab961c749de2b7efff9afface6327993336df2` |
| `c-s3b-20261104/fixture-tests.log` | `2811b223d182d8dc999dc06ba869ec658eedb30789aa603d86f323ba3b31e388` |
