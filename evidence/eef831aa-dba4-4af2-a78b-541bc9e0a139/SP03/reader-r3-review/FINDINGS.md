# FINDINGS — a139 fixed reader R3 review

Reviewed git archive `58afa97ded85a7dd89c72532bfc8973c45255d7c`, tree `543fd103532a8366ce92a0b4eaea4e089eb76d2d`. Parent `8bda0cd48976b23d268d125b34bc8130d94b8253` is contained in that commit. Workspace HEAD stayed `02b9847159a68ced1443615cbb47bcb5872724b0` until the evidence commit. No product, vendor, manifest, lock, spec, or ledger file was edited.

## Verdicts

Keys slice: **APPROVE**.

Cancel and unpublished-run slice: **REQUEST_CHANGES**.

R4, the whole full reader, and the SP03 parent stay unsigned. `bounded_reader` and `type_support.converted` stay false. Resume cursors, CDC, and the registry API stay unsigned.

## Keys

Author `pg_reader_keys_live` once, exit 0. Oracle `linux/author/keys-outcome.txt`: collation `af-NA-x-icu` orders `a,A,ä,b,B,z`; `C` orders `A,B,a,b,z,ä`. Sparse chunk 0 matches the repeatable-read cut. A writer commit during that cut stays invisible. The fresh generation reads the new cut. No-key generation 3. Reader audit on that run: 440 statements, 1 writer write, zero DML, DDL, COMMIT, or OFFSET.

Independent probe `review_public_cut_and_owner_drop` on schema `r3v`, attempts `linux/probe-2` and `linux/probe-3`. The key phase completed in both. `linux/probe-3/review-outcome.txt` records:

- `af-NA-x-icu` order differs from both `COLLATE "C"` and Rust `sort`.
- `plan_keys(0)` and `plan_keys(50001)` return fact `target_rows` and do not poison. `plan_keys(2)` then succeeds on the same owner.
- Sparse keys `-8,-3,1,4`, stride 2, two half-open chunks. Stored starts are fewer than the row count. Chunk count is at most 64. `resumable` is false.
- After chunk 0, writer `oo-fixture-writer` inserts `0` and `9`. The old cut stays `[-8,-3,1,4]`. A new admin query sees `0` in the first range and `9` in the tail. Fresh generation 2 reads `[-8,-3,0,1,4,9]`.
- Text, composite `(name,n)` including negatives, and unique `m_code` chunks match admin SQL `ORDER BY` with half-open bounds. The comparison is the server order.
- `fb` declares `Unique` index `m_code` on `code`. It does not declare the uuid primary key. Partial index `a_partial` (`payload > 0`) is earlier in index-name order and is skipped.
- `soft` (nullable unique, no primary key), `bare` (no key), and `onlyu` (uuid primary key only) are one unkeyed chunk, bounds empty, `resumable` false. `read_keys` on `soft` returns `key_required` and the following unkeyed read still runs.
- `read_probe` of the uuid primary key with `ProbeValue::Int8(1)` returns `probe_key`. `abort` returns generation 3. It does not return `SnapshotFinished`.

`supported_key_oid` in `snapshot.rs` accepts bool, int2, int4, int8, text, varchar, and bpchar. Uuid oid 2950 is a lossless Arrow value and is not a declared key. A uuid primary key does not invent a resume cursor. The live `onlyu` table is that case.

Planning fetches call `session.arm_rows(1)` before the catalog and boundary queries (`key_plan.rs`, the six call sites from `read_primary` through the page walk). `fetch_all` reserves that armed bound and settles to the returned row count. Each planning statement returns one metadata row. The server may scan `stride` tuples to produce the boundary. The client stores at most 64 boundary keys. Logged planning SQL in the probe-3 window: 10 statements containing `count(*) over`, each also containing `LIMIT`, none containing `OFFSET`. Control statements (`START`, `SAVEPOINT`, `RELEASE`, `ROLLBACK`) are charged one rate-ledger row before SQL. `StreamAccounting.executes` counts portal Execute calls and does not count those control statements.

Offline audit of the probe-3 postgres log window, `linux/probe-3/sql-audit-offline.txt`: 430 reader statements, verbs only `SELECT`, `START`, `ROLLBACK`, `SAVEPOINT`, and `RELEASE`. Zero write tokens, zero `OFFSET`, zero `COLLATE`, zero `pg_terminate_backend`. One `SELECT pg_catalog.pg_cancel_backend(...)`. That cancel statement is a `SELECT`. One writer `INSERT` on `oo-fixture-writer`. The in-test `audit()` did not run because the test panicked in the drop phase first. Author keys and cancel audits did run inside those tests.

## Cancel and run failure

Author `pg_reader_cancel_live` once, exit 0. Oracle `linux/author/cancel-outcome.txt`:

- `RunCancel` before start, generation 1.
- Demand-blocked cancel, generation 2.
- Cancel while receiving, fact `cancelled`, generation 3.
- Dropped `next_batch` left the backend active. `cancel()` then released it, generation 4.
- Cancel after a completed read, generation 5.
- Deadline fact `deadline`, elapsed 19783 ms, statement timeout 60 s, generation 6.
- `finish` after the flag returns fact `cancelled`, not `SnapshotFinished`.
- Admin `pg_terminate_backend` then next pull fact `database`, SQLSTATE `57P01`. The old run does not finish.
- Writer inserts id 4. Old partial `[1]`. Fresh generation 9 reads `[1, 2, 3, 4]`.
- Reader audit: 790 statements, 3 `pg_cancel_backend` selects, 2 writer writes, zero DML, DDL, COMMIT, or OFFSET.

Independent drop, `linux/probe-3/console.log`. Admin holds `ACCESS EXCLUSIVE` on `r3v.sparse`. The owner opens a stream and `next_batch` is polled until the data backend is `active`, then that future is dropped. The data backend stays `active`. Outcome line: `next_batch_drop_state=active data_pid=277 reader_backends=2`. The owner is then dropped. `finish` is not called. For the next 5000 ms both original `(pid, backend_start)` rows remain, and both stay in a transaction:

```
pid=277 state=active wait=/ xact=true query=SELECT id,note FROM r3v.sparse
pid=278 state=idle in transaction wait=/ xact=true query=SELECT pg_catalog.pg_is_in_recovery(), pg_catalo
```

`CLEANUP_OBSERVATION_BOUND_MS` is 5000. `reader-lifecycle-v1.md` requires the row to be gone within that bound after `Drop` returns, and not to remain `active` or `idle in transaction`.

The same server log shows, about 80 ms after the blocked `SAVEPOINT`, `SELECT pg_catalog.pg_cancel_backend(...)`, `ERROR: canceling statement due to user request` for `SELECT id,note FROM r3v.sparse`, then `ROLLBACK TO SAVEPOINT`, `RELEASE SAVEPOINT`, and `ROLLBACK` on both sessions (`21:01:46.022`–`21:01:46.023 UTC`). At the 5000 ms sample the same backends are still present and still in those transactions. `linux/probe-2/console.log` is the same failure without the wait-event detail.

Source of the drop path: `SnapshotOwner::Drop` in `crates/oo-connectors/src/snapshot.rs` sets the cancel flag and, when a Tokio handle exists, `spawn`s `slot.stop()` plus `release_session` for both sessions, then returns. `StreamSlot::stop` sets the gate and awaits the drive. `await_cancellable` in `stream.rs` asks the guard to run `CANCEL_BACKEND` on the 50 ms tick. `release_session` calls `PgSession::shutdown`, which runs `ROLLBACK` through `execute_charged` (bounded by the 60 s statement timeout) and then `Connection::close`. The sockets were still open, and the transactions still open, when the 5000 ms bound elapsed.

`RunCancel::request` and `SnapshotOwner::cancel` are a different path. The author test calls `cancel()` after dropping `next_batch` and that path released the backend. Dropping the future alone does not. Dropping the owner is required to release, and on this fixture it did not finish inside the bound.

`cleanup_unconfirmed` is the typed fact when the drain passes 5000 ms or the guard cancel fails, including SQLSTATE `25006`. `mark_skip_rollback` poisons the session and skips `ROLLBACK`. This review did not inject that fault. The author lifecycle note says the same: the path is in the source and was not observed live.

## Evidence defects

a138 `hashes.sha256` is 103 lines, 0 mismatches, 0 missing. Its final recorded exits are lib 63, keys 0, cancel 0, Clippy 0, docker 0. The failed consoles it overwrote are still gone. This review does not treat them as recovered. a138 did not sign R3. Its dropped-`next_batch` result was "backend still active, then `cancel()`", which is not drop-of-owner completion.

a139 kept failed attempts immutable:

- `linux/probe/` compile error `E0382` (`pointer` used after move). `review.exit=101`.
- `linux/probe-2/` the drop failure above. `review.exit=101`.
- `linux/probe-3/` the same drop failure, with `wait_event` and the uuid-only table. `review.exit=101`.

## Coverage that stays where it is

`snapshot.rs` and `stream.rs` differ from the tree a137 reviewed. This review's passing key and cancel reads pulled Arrow batches through the current `await_cancellable` wrapper. The column-revoke matrix was not re-run. That matrix stays an a137 receipt on the earlier tree. It is not re-signed here.

Smoke on the fixed archive, before any live probe: `cargo test -p oo-connectors --lib` 63 passed, `cargo clippy -p oo-connectors --all-targets -- -D warnings` exit 0. Vendored `sqlx-postgres` prints two unfulfilled `#[expect(dead_code)]` warnings. Those warnings do not fail the `oo-connectors` gate.
