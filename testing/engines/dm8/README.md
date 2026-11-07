# DM8 (Dameng) source engine (S5-E-03)

The `dm8` connector feature adds no Rust dependency and links no native
library. Reads go through a source-only worker that runs DM's own DPI client
(`libdmdpi.so` plus the bundled `dmPython` 2.5.38, built from source) in a
separate local image. The worker is built from the owner-authorised (E340)
native arm64 DM8 image and is never published; no Dameng binary enters a
default Open Ontology artifact. `cargo deny`/`cargo audit` cover the Rust
graph only, not the vendor client.

Driver decision, 2026-11-05: crates.io lists unofficial pure-Rust DM
protocol crates. `dm-pure` 1.2.4 (MIT, ~4k lines, one maintainer) passes
`cargo deny --config ci/deny.toml check advisories licenses` and
`cargo audit` on its own graph (36 packages), but has no TLS at all
(DH/DES login obfuscation only, plaintext data) and would change the shared
`Cargo.lock`. The vendor DPI client supports DM SSL, is the reference
implementation, and keeps the lock file unchanged, so the worker path was
chosen. This is not proof that no admissible Rust driver exists.

## Worker contract

`oo-load --dm8-worker WRAPPER` and `oo doctor` (environment `OO_DM8_WORKER`)
run an operator-trusted executable. `fixture.py worker` builds the image and
writes such a wrapper: `docker run --rm -i` with `oo.lane=d8`, the source's
network namespace, 256 MiB, one CPU, read-only root, no capabilities. Source
credentials travel once on stdin (`oo.dm8.driver/v1`), never argv or
environment. The worker accepts only bounded single `SELECT` statements,
opens the connection with `DSQL_MODE_READ_ONLY` and autocommit, returns
text/null cells (binary as hex) in framed JSON batches, and reports only DM's
numeric error code, never SQL or driver text. dmPython's `fetchmany()` leaks
memory per row (measured 97 MB per 270k rows), so the worker uses
`fetchone()`.

Only literal loopback inside the worker's network namespace is accepted;
DM SSL (client key directory) is not wired yet, so a remote DM8 source or a
`tls_ca` is refused (`dm8_tls_not_available_use_loopback_worker`,
`DM8_TLS_NOT_AVAILABLE`). SSH and log CDC are not integrated.

## Engine behaviour

- Probe: `V$INSTANCE` banner (`DM Database Server x64 V8 <build>`; DM8 does
  not expose a dotted release, so the version is major 8 with the build in
  `raw`), case sensitivity and charset. `COMPATIBLE_MODE`/`BLANK_PAD_MODE`
  need `SELECT ON V$DM_INI`; without it they are reported as unmeasured with
  advice, never a blocker. Missing table grants produce
  `GRANT SELECT ON "S"."T" TO "USER"` and block snapshot/C5.
- Catalog: `ALL_TAB_COLUMNS`, primary keys from `ALL_CONSTRAINTS`. String
  keys carry the source collation
  (`dm8:case_sensitive=1;charset=UTF-8;blank_pad_mode=...`).
- Chunks: source-ordered keyset bounds (`OFFSET n ROWS FETCH FIRST 1 ROWS
  ONLY`), every comparison in DM8; integer, decimal and character keys.
  Ranges are cut at 90% of `max_chunk_rows`, so a durable C5 range may grow
  10% before a read exceeds the budget. No-key tables use one bounded read.
  Plan revisions are per table, so a C5 run's plan matches its snapshot's.
- Budget: row, byte and deadline limits per statement; every catalog and
  data statement sleeps `elapsed * (100 - duty) / duty`.
- Watermark: `dm8-observed:<UTC SYSTIMESTAMP>`, an observation fence, not a
  log position. Consistency B (per-statement read committed).
- C5: order-independent SHA-256 over each range's rendered rows; lineage
  binds `ALL_OBJECTS` object id and creation time plus full column and key
  definitions.

## Type mapping

| DM8 type | Agent DB | Notes |
|---|---|---|
| BIGINT | bigint | |
| INT, INTEGER, PLS_INTEGER | integer | |
| SMALLINT, TINYINT, BYTE | smallint | |
| BIT | boolean | |
| DECIMAL/NUMBER/NUMERIC(p<=38, 0<=s<=p) | numeric(p,s) | exact; never rounded |
| NUMBER/DECIMAL without precision | text | exact decimal text |
| DOUBLE, FLOAT | double precision | |
| REAL | double precision | widened in the source (`CAST AS DOUBLE`) |
| DATE | date | |
| TIMESTAMP/DATETIME(<=6) | timestamp | |
| TIMESTAMP(7-9) | text | `YYYY-MM-DD HH24:MI:SS.FF9` |
| TIMESTAMP/DATETIME WITH TIME ZONE (<=6) | timestamptz | UTC instant, offset not kept (D17) |
| TIMESTAMP WITH LOCAL TIME ZONE (<=6) | timestamptz | UTC instant (D17) |
| TIME, TIME WITH TIME ZONE, INTERVAL ... | text | DM's own `CAST AS VARCHAR` rendering |
| CHAR, VARCHAR, VARCHAR2, NCHAR, NVARCHAR(2) | text | CHAR keeps its padding |
| TEXT, LONG, LONGVARCHAR, CLOB, NCLOB | text | |
| BINARY, VARBINARY, RAW, BLOB, IMAGE, LONGVARBINARY | bytea | |
| anything else (BFILE, classes, arrays, spatial) | refused | `dm8_type_not_admitted` |

## Reproduce

```sh
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4
cargo test -p oo-connectors --features dm8 --lib --locked dm8
python3 -m unittest discover -s testing/engines/dm8
cargo build --manifest-path tools/oo-load/Cargo.toml --locked
cargo build -p oo-cli --locked
python3 testing/engines/dm8/fixture.py create --run-id RUN --full   # server, seed, worker
python3 testing/engines/dm8/product_t0.py --run-id RUN --binary target/debug/oo
python3 testing/engines/dm8/target_acceptance.py --run-id RUN --c5
python3 testing/engines/dm8/fixture.py remove --run-id RUN
```

`--full` seeds 231 tables, 2M events and a 300-column table; without it the
fixture is six focused tables. `target_acceptance.py` runs the snapshot
load under a concurrent admin writer (insert, update, delete, primary-key
moves) plus one commit after the load returns, then exactly one C5
correction cycle on that load, which must publish, and compares every
source row (rendered by `disql`, zone conversion in Python) with Agent DB.
`--c5` adds idle, changed and idle cycles. Each run writes explicit verdict
rows through `tools/oo-test/verdicts.py`.
