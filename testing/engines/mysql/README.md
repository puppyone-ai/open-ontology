# Lane C: MySQL SourceEngine

Task card C-S0S1: S0-C-01, S1-C-01, S1-C-02.

## Run

```sh
cargo test -p oo-connectors --features mysql --lib --locked
python3 testing/engines/mysql/run.py --run-id c-mysql-new-run
```

The runner uses exact tags `5.7.44`, `8.0.44`, `8.4.7`, `9.5.0`, sequentially.
5.7 uses `linux/amd64`; the others use the host platform. Image IDs/digests
are recorded, not silently interpreted as a matrix certification. Use a new
run ID each time. Docker and OpenSSL are required.

Every container and named volume has `oo.lane=c` and `oo.run_id` labels.
Only resources created by that invocation are removed. The reader has SELECT
and replication privileges, not writes. A separate fixture account handles
mutation and observation. Passwords and TLS keys are runtime-generated into
private, git-ignored `evidence/<run-id>/` files; none are command arguments.
Readiness uses TCP so the image's temporary initialization server cannot be
mistaken for the final server.

The generator is local to `run.py`, outside lane H's `seed/` directory, and
can be replaced by the shared generator. It creates all five key shapes,
case/accent/PAD SPACE collisions, an empty table, a 2,000,000-row table, digest
separator-collision cases, and a table that grows beyond its original budget.

## Adapter contract

- `mysql::MysqlEngine` implements the fixed `SourceEngine` interface; no
  shared interface changes or dependencies are needed.
- Bounds are the interface's `[lo, hi)`, not the older preview's `(lo, hi]`.
  SQL comparisons and ordering remain on MySQL under source collation.
- Estimates use explicit `EXPLAIN FORMAT=TRADITIONAL`, including on 9.5.
  A bounded, cursor-relative boundary lookahead uses `LIMIT n, 1`; there is
  no growing whole-table offset or offset-based resume.
- Large plans sample keys on the server using a deterministic 1/1000 hash
  predicate. This reduces returned keys, not necessarily server scan work.
- Over-budget chunks return no batches. Call `split_chunk` with a fresh
  chunk-ID allocation to obtain a higher plan revision, then retry all
  children. Unsplittable/no-key reads fail explicitly.
- Receipts hash deterministic Arrow IPC grouping. Conversion rejects and
  serialized-byte overflows cannot become successful receipts.
- `rest_ms` measures source-idle time, including client conversion and
  consumer work; `sleep_ms` separately measures explicit duty-cycle waits.
  One engine owns at most one connection. The caller still owns bounded
  scheduling across multiple engine instances.

## Evidence boundaries

The live suite checks version dispatch, privileges, GTID and file-position
watermarks (including real tagged GTIDs on 8.4/9.5), source-ordered plans,
EXPLAIN index use, canonical row equality against an independent unbounded
source SELECT, IPC hashes, growth revisions, sampled plans, and load pauses.
T8 combines adapter timing with a separate SHOW PROCESSLIST observer.

The concurrent writer commits inserts, updates, deletes and PK moves.
The harness reads the real fixture binlog interval, verifies event CRC32s,
collects both key images of committed updates, and applies current point
reads at a verified quiescent end cut. It compares source/target canonical
hashes per original key range as well as the complete row multiset.
`mysql_binlog_fixture.rs` understands only this fixture's INT/VARCHAR layout.
It is deliberately **not** the product CDC reader.

This proves snapshot/watermark/reconciliation primitives, not continuous
production log consumption, Agent DB loading/publication, or the full
AT-ENG-01 T0-T2 gate. Those remain S2/coordinator integration work. No public
or customer systems are tested. The tests do not establish global snapshot
consistency across independently read chunks without correction.
