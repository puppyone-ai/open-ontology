# W1-B / C5+ module delivery

Status: module prototype with owned fixtures; not eligible for `sync:auto` or
verified admission until coordinator integration and production calibration.
No customer systems or credentials were accessed. Only the owned container
`oo-wf-c5plus-mysql`, localhost:23361, MySQL 5.7.44 with log_bin=OFF was used.
No job-worker, Loader, Agent DB migration, or chunk connector changes.

## Module and integration contract

`c5plus.rs` exports digest construction, fixed-bound source-to-source comparison,
JSON persistence, window/load/duty controls, L1/L2/L4 helpers and row reconciliation.
`lib.rs` only registers the module. `Scanner` owns one `Conn` and requires mutable
access, so module operations cannot issue concurrent source reads.

wf-chunk supplies a trusted SQL predicate and `mysql_async::Params`, for example
`("id > ? AND id <= ?", Params::Positional(vec![0.into(), 100.into()]))`.
For varchar use bytes parameters; all boundary ordering remains in MySQL. No client
lexical range membership is used. Identifiers are quoted independently. Predicate
and complete SQL arguments are trusted internal inputs, not request SQL.
Call `digest_sql(schema, table, keys, columns, predicate, max_execution_ms)` then
`Scanner::digest(sql, params)`. Every key must be in the digest projection.
Call `check_schema` before every block using persisted metadata, and stop that
table on SCHEMA_DRIFT. This check is caller-driven, not automatic orchestration.
`recent_predicate` accepts typed lower/upper bounds computed as now-window-overlap
and now. Its result goes to `rows`; it does not advance any cursor.
`full_read_due` requests the existing full-reader; it does not implement another
full-read or scheduler. `Hint` has no skip-table result.

The caller must close the scan/receipt/checkpoint ordering: compare previous
**completed** scan against current, reread all new/changed blocks and reconcile removed memberships, apply
row mutations and wait for receipt, only then persist completion. `ScanProgress` rejects comparison of incomplete scans, checkpoints only the next
planned block after application, and reloads the next pending block. Incomplete
scans must never generate removed-block deletions. A changed topology uses new
chunk IDs and mandatory first full reads. `resize` returns split/merge candidates;
actual safe boundary selection and parent membership transfer are not implemented.

## SQL construction and normalization

For columns id,v the generated SQL is equivalent to:

```sql
SELECT /*+ MAX_EXECUTION_TIME(5000) */ COUNT(*),
 BIT_XOR(CAST(CONV(SUBSTRING(MD5(CONCAT_WS('#',
   COALESCE(HEX(CAST(`id` AS BINARY)), ''), ISNULL(`id`),
   COALESCE(HEX(CAST(`v` AS BINARY)), ''), ISNULL(`v`))),1,16),16,10) AS UNSIGNED)),
 BIT_XOR(CAST(CONV(SUBSTRING(MD5(CONCAT_WS('#',
   COALESCE(HEX(CAST(`id` AS BINARY)), ''), ISNULL(`id`),
   COALESCE(HEX(CAST(`v` AS BINARY)), ''), ISNULL(`v`))),17,16),16,10) AS UNSIGNED))
FROM `s`.`t` WHERE (`id` > ? AND `id` <= ?)
```

HEX of binary casts preserves source character-set bytes and blobs, avoiding
conversion replacement and making `#` impossible in payloads. Null flags and
fixed field count distinguish NULL, empty strings, and field boundaries. Numeric
and temporal casts use MySQL's own formatting; frozen-data fixtures include
DOUBLE, DECIMAL(20,6), DATETIME(6), NULL, empty text, and binary 00FF. Only compare
source to source under identical schema/session/server settings. Timestamp timezone,
server upgrades, unsupported types, and floating textual precision require further
validation. This is not cross-engine normalization. Keys are hashed with values,
so swapping values across keys does not trivially cancel the XOR. MD5/XOR remains
probabilistic; equal digests are not proof, and L4 remains mandatory.

## State file

Caller chooses a per-table file under an existing, owned directory:

```json
{"version":1,"table":"s.t","chunks":[{"chunk_id":"c0","bound":{"lo":0,"hi":100},"rows":2,"digest":["0123456789abcdef","fedcba9876543210"],"scanned_at":1790000000}],"next_chunk":"c1"}
```

`chunk_json` / `load_chunks` encode u64 digest halves as hex strings. `save` writes
an exclusive `.json.pending`, fsyncs, renames and fsyncs the parent. A leftover
pending file fails closed; recovery requires inspecting it. Single-writer only.

## 限制与升级条件

- JSON is not an atomic transaction with Loader receipts. Durable scan generations,
  receipt idempotency and two-generation scan promotion
  need coordinator integration before continuous use. Migrate to `_oo` append-only
  migrations when multiple workers, concurrent scheduling, or transactional receipt
  coupling is needed. Checkpoint resume is fixture-tested, but atomic
  receipt/checkpoint coupling is not claimed here.
- `typed_rows` reuses connector `decode_cell` and `StoreKind`, returning
  `DecodedCell` (logical and Arrow values), and fails closed on cell rejects.
  Target rows must use the same mapping. Keys retain MySQL integer/byte identity;
  collation-equivalent byte-distinct identities still need integration validation.
- Tombstones carry generation; applying generation fences and clearing/replacing
  a tombstone on key reuse belongs to the existing Loader, not this module.
- Read results are buffered per block; caller must bound blocks by rows and bytes.
  No streaming/memory rejection budget is implemented here.
- `Budget::adapt` is tested EWMA targeting 0.5s but caller must apply the proposed
  row count to future boundary planning. Fixed existing bounds do not move mid-scan.
- Pauses record duration, SELECTs have server hints and client timeouts; transaction
  start/commit/schema queries are not all client-time-bounded yet. A timed-out
  connection should be discarded by the caller. No automatic reconnect.
- General-log verification permits read-only transaction control and connection
  character-set initialization in addition to SELECT/SHOW. Literal SELECT/SHOW-only
  auditing is incompatible with the separate requirement for explicit read-only
  snapshot transactions; no data writes, LOCK TABLES or FLUSH are intended.
- General logging starts OFF, is enabled only by fixture admin for audit and then
  disabled. Admin fixture DDL/DML are not source-reader operations.
- Fixture coverage includes window inserts, old-row updates, deletion mutation
  and generation-tagged tombstones, same-key reinsertion mutations, a committed
  transaction missed without overlap but included with overlap, source collation
  case/PAD SPACE boundary equality and rejected duplicate keys, final-value-only
  updates, connection interruption plus checkpoint resume, load pause/recovery,
  add-column drift, CRC32-collision and swapped-value controls, and stable frozen
  digests. These are module/target-interface tests, not Loader or Gateway tests.
  Actual Loader tombstone fences and receipt atomicity remain NOT VERIFIED.
- No customer calibration or 24-hour real-source observation was performed or
  attempted. Those belong to the coordinator, with approved real-source access.

## Verification and performance

See final execution result below. The owned benchmark generator creates 10,000,000
anonymous rows. `c5plus_bench.py` runs 100 fixed 100k-row blocks with the same digest
construction, recording server profiling duration and client elapsed time, then
sleeping at least three times client elapsed. This administrative calibration
script uses profiling and is not the SELECT-only source reader.


### Executed results

- Formatter: `rustfmt --check --edition 2024 tools/mysql-preview/src/c5plus.rs tools/mysql-preview/tests/c5plus.rs tools/mysql-preview/tests/c5plus_live.rs` PASS.
- With `CARGO_TARGET_DIR=/tmp/oo-wf-c5plus-target CARGO_BUILD_JOBS=4`:
  `cargo clippy --locked --manifest-path tools/mysql-preview/Cargo.toml --lib`,
  `cargo check --locked --manifest-path tools/mysql-preview/Cargo.toml`,
  `cargo build --locked --manifest-path tools/mysql-preview/Cargo.toml`: PASS.
  Clippy has 12 warnings in existing preview code, none in c5plus.rs; dependencies also warn.
- `cargo test --locked --manifest-path tools/mysql-preview/Cargo.toml --test c5plus --test c5plus_live -- --nocapture`: 7 pure tests + 1 owned live scenario PASS, 0 failed/ignored. The scenario has assertions for both integer/varchar fixtures and all module scenarios listed above; not 11 independent tests.
- `cargo test --locked --manifest-path tools/mysql-preview/Cargo.toml --lib`: 171 passed, 0 failed, 5 existing ignored.
- `cargo test --locked --manifest-path tools/mysql-preview/Cargo.toml`: 182 passed, 0 failed, 28 existing ignored (including existing fixture-gated tests). No new ignored tests or skip flags.
- Initial baseline command started before edits, but compilation overlapped edits and failed on an interim missing transaction variable. It is NOT a clean baseline. The first new-test run likewise failed compilation, rather than a behavioral assertion; strict behavioral test-first evidence is unavailable. Final compilation and tests passed.
- `python3 -m py_compile tools/mysql-preview/src/c5plus_bench.py`: PASS.
- `git diff --check`: PASS.
- Native debug binary: `/tmp/oo-wf-c5plus-target/debug/mysql-preview`, SHA-256 `5a22314170a00339d5b74f02c8437507c7d030ac6f66947c7d3b48d395880588`. No Linux release binary produced; the public CLI does not yet invoke this module.

Benchmark: `python3 tools/mysql-preview/src/c5plus_bench.py`, PASS, exactly 100 blocks / 10,000,000 rows. Full per-block source/client timings and digest outputs: `c5plus-benchmark.json`.

| Metric | Measured |
|---|---:|
| Elapsed including sleeps/process overhead | 387.601 s |
| Total source SELECT time | 77.886 s |
| End-to-end throughput | 25799.7 rows/s |
| Active-source throughput | 128392.0 rows/s |
| Source duty | 20.094% |
| Source block min / median / max | 0.560456 / 0.687368 / 1.690138 s |

This is one narrow synthetic integer-key table, fixed 100k blocks, emulated amd64, with concurrent local fixture activity. It is not a customer SLA, varchar-scale or three-large-table calibration; adaptation was unit-tested separately. The fixed-size benchmark does not claim to meet the 0.5s target. Initial calibration should reduce rows from 100k using observed rate, then keep EWMA adjustment and byte budgets.

Audit aggregate: `c5plus-audit.txt`. No reader DML/DDL/LOCK/FLUSH appeared. Explicit read-only transaction control and charset initialization DO appear; literal SELECT/SHOW-only criterion is not met. This needs coordinator interpretation, not a hidden audit bypass.

Safety deviation discovered after the broader suite: the existing `source_sql_live::explicit_plaintext_full_transport_on_an_isolated_mysql_fixture` test automatically created and removed a **new**, uniquely named `oo-yjcap-full-source-sql-<pid>-<timestamp>` anonymous container. This violates this task's container-prefix rule, although no pre-existing container or customer system was accessed. Do not rerun that existing test until its fixture prefix is configurable; its source was not modified in this task. The task-owned `oo-wf-c5plus-mysql` fixture is cleaned up after evidence capture.

Acceptance: A module assertions PASS; actual Loader receipt/tombstone integration NOT VERIFIED. B no data writes/LOCK/FLUSH PASS, literal SELECT/SHOW-only FAIL due to necessary read-only transaction/session controls. C SQL/state/wf-chunk contract/limitations/10m evidence PASS. Real-source 24-hour observation NOT VERIFIED and deliberately not attempted.
