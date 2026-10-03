# Test matrix — PostgreSQL 17.10, Rust 1.94

Independent of the a121 oracle. The a121 fail `add_column_active_current_schema` stays in `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a121/` with oracle exit 101. Do not edit that outcome to attnum 3 or to a pass.

The new fixture is a fresh database. `init.sql` in the a121 evidence grants only `UPDATE` on `lot.tick`. The canary trigger is `SECURITY INVOKER` and reads `hits`, so `UPDATE` alone returns `42501`. The new fixture must execute and record:

```sql
GRANT SELECT, UPDATE ON TABLE lot.tick TO lot_writer;
```

After every `ADD COLUMN`, the admin session records the live catalog, including dropped columns:

```sql
SELECT a.attnum, a.attname, a.atttypid::int8, a.atttypmod, a.attnotnull, a.attisdropped
FROM pg_catalog.pg_attribute a
JOIN pg_catalog.pg_class c ON c.oid = a.attrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = 'drift' AND c.relname = 'item' AND a.attnum > 0
ORDER BY a.attnum;
```

Assertions use that attnum for `extra`. A fresh table's next attnum may be 3. a121's attnum 6 is the earlier database. Both are valid only as recorded output.

Image: PostgreSQL `17.10` (Debian 17.10-1.pgdg12+1 is the a121 image family). Rust image linux/arm64, `RUSTUP_TOOLCHAIN=1.94.0`. One builder, one database. `--pull never` only when the image is already present.

Public API under test: `SnapshotOwner::open`, `binding`, `read_probe`, `finish`, `abort`. No substitute reader. No caller SQL.

## Cases

| Id | Setup | Action | Expected |
| --- | --- | --- | --- |
| S0 | Frozen tree, before edits | Lib tests and Clippy as in the card | Record exits. Historical a121 red stays red. |
| C0 | Two related tables, writer commits after the owner snapshot and after the side session's first read | Owner probe, side probe, expected old set | Sets equal, including absent keys. Fresh generation equals the new-cut oracle. `SnapshotFinished` on both generations. Canary delta from `oo-pg-source` is 0. |
| C1 | Owner open on `drift.item` (`id`, `label`) | Commit `ADD COLUMN extra text` before `read_probe` | `read_probe` is `catalog` / `schema_drift`. No batch. `finish` is the same fact. Not `SnapshotFinished`. Recorded attnum of `extra` is the one compared. |
| C2 | Owner open, `read_probe` returns the old token | Commit `ADD COLUMN` after the probe returns, before `finish` | Probe token is the old cut. `ALTER` commits rather than waiting out `lock_timeout`. `finish` is `schema_drift`. Not `SnapshotFinished`. |
| C3 | Owner open, no `read_probe` | Commit `ADD COLUMN`, then `finish` | `schema_drift`. Not `SnapshotFinished`. |
| C4 | Owner open, no probe yet | `DROP TABLE` and `CREATE TABLE` of the same name, then probe or finish | `schema_drift` because the oid changed. Record both oids. A matching column list does not pass. |
| C5 | Separate owners | Commit one of: column drop, column rename, type change, typmod change, `SET NOT NULL`, primary-key column change. Then probe. | Each owner: `schema_drift` before a batch is returned. |
| C6 | Three `PgConnector::connect` sessions held | `SnapshotOwner::open` | `budget` / `max_connections`. Generation not consumed. No new `oo-pg-source` backend. Drop one holder. Open succeeds and holds two backends. A second owner fails the same fact. |
| C7 | The C6 owner, or a normal owner | `finish`, `abort`, and `Drop` on separate runs | Both pids absent from `pg_stat_activity` within 5000 ms. Neither row remains `idle in transaction`. A later `try_reserve(2)` succeeds. |
| C8 | Admin `pg_terminate_backend` on the guard pid | Next `read_probe` or `finish` | Connection fact, not `SnapshotFinished`. Data pid and guard pid gone within 5000 ms. Slots released. |
| C9 | Wrong CA | `open` | Existing TLS fact. No owner. No leftover `oo-pg-source` backend. Slots released. |
| C10 | `expected_database` or `expected_principal` does not match | `open` | Existing `database` or `principal` fact. No owner. No leftover backend. |
| C11 | `expected_postmaster_start` does not match | `open` | `read_node` after the sockets exist. Both backends gone within 5000 ms. Generation consumed. |
| C12 | Deadline already passed | `open` | `deadline`. No socket. Generation not consumed. |
| C13 | Owner open, client deadline expires before `finish` | `finish` | `deadline` when nothing was poisoned. Not `SnapshotFinished`. Both backends gone within 5000 ms. |
| C14 | Log from a run that includes C0 and C1 | Count `LOG: execute` by `application_name` | Nonempty. `oo-pg-source` has zero DML, DDL, and `COMMIT`. A probed run includes `SAVEPOINT oo_probe_lock` and `ROLLBACK TO SAVEPOINT oo_probe_lock`. Secret-file hits 0. |
| C15 | Fresh generation after C1, admitted shape includes the recorded `extra` attnum | `open`, probe, `finish` | Old token plus the new null column. `SnapshotFinished`. This is a new generation, not an append. |

## Audit verbs

Actor `oo-pg-source` may show `SELECT`, `START`, `ROLLBACK`, `SAVEPOINT`, and `ROLLBACK TO SAVEPOINT`. It must show zero `INSERT`, `UPDATE`, `DELETE`, `COMMIT`, `ALTER`, `CREATE`, `DROP`, `GRANT`, `REVOKE`, `TRUNCATE`, `COPY`, and `CALL`. Writer and admin verbs stay in their own application names.

## Fixed-SHA rereview

After the implementation commit, a reviewer other than the coder reruns C0–C15 against that commit's `git archive`. The reviewer does not inherit this design session's builder. Original a121 exit 101 is cited, not rerun as a pass. A `SnapshotFinished` on C1, C2, or C3 is `REQUEST_CHANGES`. This design does not schedule that review and does not mark R1.
