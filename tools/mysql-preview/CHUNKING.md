# MySQL source-collation chunk contract

## Capture entry and evidence

`mysql-preview capture-full --config PATH --out DIR` accepts optional top-level
`"chunking": true`. Without it, supported small tables use the existing bounded
single-read path and output shape. A raw row/byte budget overflow restarts keyed
tables with chunks. Composite keys and legacy charset conversion use chunks.
Explicit chunk mode allows an operator total deadline up to 24 hours; ordinary
single-read mode keeps its 300-second cap. Each range/boundary still has the
configured statement limit (at most 30 seconds).
A table without a usable key can only use a bounded, single full snapshot;
overflow returns `excluded` with an explicit budget/unique-index upgrade reason.

Chunk captures emit one `capturer.json`, `schema-preimage.json`,
`type-preimage.json`, `driver.json`, `access.json`, `connection.json`, and
`reader-write-audit.json`, as before. `table-0.arrow` is a file-backed Arrow stream
with one record batch per chunk, not an in-memory table concatenation. For chunked captures, `access.json` additionally records
`chunk_key_order: source_collation`; small-table access evidence is unchanged.
Additional files are `<id>.arrow`, `<id>.audit.json`, `<id>.metrics.json`, `chunks.json`,
and `chunk-binding.json`. `capturer.json` is emitted only after the whole table
and its audit are valid. Failure leaves `failure.json`, never a publishable
partial capturer. Capturer `chunk_budget` records per-chunk limits; its v1
`max_rows` records completed table rows for the admission budget. Existing
operator admission budgets are still enforced, not bypassed.

**The chunked table grade and cross-table grade are B, not S0.** Each range gets
its own REPEATABLE READ `START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY`.
Different chunks can observe different times. Resume during writes is not a
single consistent whole-table cut. There are no OFFSETs, synthetic GTIDs or
fabricated log positions. C5+ must preserve chunk identity/key membership;
never determine source range ownership with target-side string ordering.

Each successful chunk uses its own source connection. A
`mysql.chunk_audits/v1` source-audit envelope binds each child's schema/table and
actual connection ID. Children must all satisfy the installed evidence policy:
`mysql.general_log` requires complete Connect/start/business read/commit evidence,
logging enabled before/after, and no forbidden statement; `grant_enforced`
requires observed read-only grants and the read-only transaction. Admission
cannot enable grant_enforced policy from a witness. The preparer accepts explicit
operator `--source-audit-policy grant_enforced`; native-producer forwards only
its operator-configured `source_audit_policy`, never one inferred from capture. Completed child audits are
persisted before their manifest entries and reused on resume. Failed attempts
are not successful evidence. Failure cleanup may KILL only an owned connection;
KILL remains forbidden in successful source-audit records.

The v1 preparer/sealer accepts the combined multi-record-batch Arrow stream.
This is capture/admission compatibility, **not a removal of downstream Loader
per-import limits** (100,000 rows / 32 MiB). A capture beyond those limits needs
a per-part Loader/staging integration before target import/publication. Do not
confuse source-to-Arrow scale verification with target publication verification.

## Stable connector API

`oo_connectors::mysql::chunk` exports:

- `KeyColumn { name, kind: KeyType, collation }`, in source unique-index order.
  `KeyType::{Int, Text, Bytes}`; collation is descriptive, not a SQL override.
- `KeyValue::{Int(i128), Text(Vec<u8>), Bytes(Vec<u8>)}`. Text values are the
  bytes returned by MySQL, not Rust Strings. JSON uses arrays of byte integers,
  losslessly, including invalid UTF-8. There are no client string ordering traits
  or sorting operations. Only exact equality checks validate manifest adjacency.
- `ChunkBound { lo: Option<Vec<KeyValue>>, hi: Option<Vec<KeyValue>> }` represents
  `(lo, hi]`; missing lo is the first range.
- `range_where(&[KeyColumn], &ChunkBound) -> Result<(String, Vec<KeyValue>), MysqlError>`
  returns an empty fragment or leading ` WHERE ...`, backtick identifiers and
  positional `?` parameters. Composite predicates are equality-prefix
  disjunctions, not row-value comparisons. Invalid identifiers, duplicate key
  columns, tuple widths and value variants fail closed.
- `key_columns(&mut Conn, schema, table)` selects PRIMARY first, otherwise the
  first wholly non-null unique index in MySQL index-name order. Column order
  comes from `SEQ_IN_INDEX`. Prefix indexes, nullable keys and unsupported key
  types do not qualify; no suitable index returns an empty vector. Supported
  key types are MySQL integers, char/varchar and binary/varbinary.
- `next_boundary_sql(schema, table, keys, lo, rows)` exposes SQL/parameters for
  EXPLAIN. `next_boundary(&mut Conn, ...)` executes it and returns an optional
  vector; None means exhausted. LIMIT is 1..=100,000. Single keys use MAX over
  an ORDER BY/LIMIT subquery. Composite keys select the last tuple of that
  subquery, with descending order performed in MySQL. All comparisons use the
  columns' own collation and indexes, never BINARY ordering imposed on text.
- `read_chunk(Conn, ChunkRead) -> Result<(Conn, Batch), MysqlError>` consumes the
  connection. Success returns it and the existing typed Arrow Batch (raw rows,
  bytes, conversion rejects). Failure returns no batch and closes the connection.
  ChunkRead supplies schema/table, ordered keys, mapped columns, run binding,
  finite hi, max_rows/max_bytes and deadline. Its caps are 100,000 rows and
  16 MiB per payload/Arrow batch, reusing the shared encoder bound. The full
  input can request 32 MiB but clamps an individual chunk to 16 MiB; larger
  single LOB values need a LOB/encoder upgrade, not an oversized batch. It checks InnoDB and non-null mapped
  keys. A server-side bounded row-size preflight prevents oversized LOB values
  from being transferred in full. Streamed row count, payload bytes and Arrow
  array memory are checked independently. Overflow never returns a partial batch.
- `AdaptiveRows::new(initial, max_rows, max_bytes)`, `rows()`, `observe(rows,
  bytes, elapsed)` target 0.5 seconds with 0.25-weight EWMA read throughput and
  observed bytes/row; `shrink()` halves the current limit down to one. Capture
  automatically shrinks/re-solves overflow ranges, checks serialized IPC bytes,
  and retries protocol-disconnected ranges unchanged, at most three reconnects.
  A single row that still cannot fit records a masked budget reject and excludes
  the candidate. Statement/deadline failures are not mislabeled as large rows.

Integer parameters cover signed i64 and unsigned u64; other i128s are refused.
The existing integer ChunkReader API and tests are unchanged. Identifiers retain
the existing connector ASCII alphanumeric/underscore, 64-byte limit. Payload and
Arrow budgets bound batches, **not total RSS** or protocol/allocator overhead.
Captured scale RSS is reported separately. Per-chunk elapsed metrics describe
the completed attempt; retry counters and total wall time expose fault overhead.

## Manifest, identity and resume

`ChunkEntry { table, chunk_id, lo, hi, rows, bytes, arrow_sha256 }` is an ordered
JSON array in `chunks.json`; IDs start at zero. `chunk-binding.json` is its
mandatory identity companion: source_id, schema, table, run_generation, ordered
key descriptors including collation, config_sha256, and schema_sha256. The
schema hash covers ordered catalog definitions (names, ordinals, source types,
nullability, lengths, numeric metadata, charsets and key flags). Config bytes
must be identical, including whitespace and source endpoint/generation/approval.
Changed identity, schema, collation or policy config refuses resume.

Rerun capture-full with the **same config and incomplete output directory**.
An existing final capturer is not resumed/overwritten. Capture checks identity,
loads the manifest and verifies SHA-256 of every completed Arrow before reuse,
then reconstructs the compatible aggregate stream from those verified files
and resumes after the last hi. No OFFSET is used. Existing completed Arrow
files are not rewritten. Corrupt/missing files and malformed chains fail closed.

`load_manifest(directory, table)` validates IDs, byte-equal adjacency, nonempty
ranges, table identity and every completed file hash. `save_manifest` validates
the chain and newly appended file hash; prior entries must already have been
loaded/verified or produced in this writer. It fsyncs a new
`chunks.json.pending`, atomically renames it, then fsyncs the directory. Capture
first fsyncs the chunk Arrow and child audit. A crash-left pending manifest is
uncommitted and discarded only after binding and the committed manifest pass
validation; unreferenced chunk files are not reused. Manifest cap: 32 MiB.

## Types and offline column conclusions

`mysql::map::column_mapping_conclusion(&information_schema_row, session_utc)`
returns JSON with accepted/target_type/target_oid/target_typmod/read_expression,
or accepted=false with a closed code/reason. Input uses information_schema
COLUMNS names: COLUMN_NAME, ORDINAL_POSITION, DATA_TYPE, COLUMN_TYPE, IS_NULLABLE,
CHARACTER_MAXIMUM_LENGTH, NUMERIC_PRECISION, NUMERIC_SCALE, CHARACTER_SET_NAME,
COLUMN_KEY. Missing optional numeric metadata is null. No database is contacted.
The `mysql_column_map` example reads one such JSON row per stdin line; pass
`--session-utc` only when the capture session is guaranteed UTC.

`map_capture_column` and `capture_expression` extend existing mapping without
changing the legacy `map_column`/`map_table` contract. gbk/ucs2 are explicitly
converted to utf8mb4. A binary round-trip guard plus Rust UTF-8 validation detect
unrepresentable/invalid results (including the ucs2 surrogate fixture). Failed
non-key cells use existing Unavailable/CellReject/MaskedReject semantics. Capture
writes masked `rejects.json` with source/run/column/key-digest identity, never
raw values, and blocks candidate publication. Budget rejects have unknown
key/value length explicitly null; a boundary digest is not a canonical key hash.
These sidecars preserve sync.rejects semantics for downstream ingestion; they do
not directly write a target database's sync.rejects table.

Decimal uses exact Decimal128/Decimal256 mapping, never f64; precision/scale
limits are explicit mapping decisions. datetime(6) is timestamp without timezone,
with microseconds preserved. timestamp(6) is UTC timestamptz; capture explicitly
sets the session time_zone to +00:00 on every connection. A zero date is not a
valid target date: existing nullable zero-date behavior is null + reject note;
chunk conversion failures are masked rejects; either blocks capture publication.
text/blob honor per-row and per-chunk byte limits; single-row overflow is an
explicit reject/exclusion, not truncation.

## Restrictions and upgrade conditions

Single writer, trusted local per-table directory and local atomic rename/fsync
are required. There is no shared/distributed checkpoint coordinator. Upgrade to
transactional checkpoint storage before multiple workers, distributed capture,
source rebinding or manifests beyond the size cap. A schema or
collation change requires a new capture, not editing a checkpoint. Schema and
account grants are assumed stable during an active run; the fingerprint is
checked at start/resume, not by a DDL lock or a privilege-change monitor. Retry is
bounded; source outages pause the candidate for operator retry. A fully
consistent moving-table cut requires CDC or a separately supported source snapshot
mode, not relabeling B as S0. Downstream multipart target import/publication is a
separate integration gate described above.

See `testing/mysql-full-c5/w1-a/` for actual scale timings, RSS, interruption and
ordered row digest evidence, and `tests/full_chunk_live.rs` for owned-fixture
collation, composite/index order, EXPLAIN, in-flight KILL, budget, charset, temporal,
identity and corruption tests. These are anonymous fixture evidence, not customer
certification; no customer systems are contacted by these tools/tests.
