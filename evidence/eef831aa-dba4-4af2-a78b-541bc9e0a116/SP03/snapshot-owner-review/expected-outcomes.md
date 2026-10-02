# a116 preregistered snapshot-owner outcomes

Registered before the production `SnapshotOwner` runs. The fixture bind writes `live/expected/cut.txt` from the live PostgreSQL 17.10 catalog and these row values, and it exits if the server disagrees. The Rust probe loads that file and does not invent a second set of row expectations.

Author `live/phases/outcome.txt` labels are not inputs.

## Cut

Database `revdb`. Principal `oo_rev_reader`. Recovery false. Postmaster text is whatever `pg_postmaster_start_time()::text` returns at bind time, with server `TimeZone=UTC`.

`rev.item` columns, attnum order: `id int4 not null`, `label text not null`, `flag bool not null`, `code varchar(8) not null`, `odd"note text null`. Primary key `id`.

`rev."line item"` columns: `id int4 not null`, `item_id int4 not null`, `qty int2 not null`, `memo text null`. Primary key `id`.

Same-cut rows, before the fixture writer:

- item 7: `int:7 text:cedar bool:true text:A116 null`
- line item 21: `int:21 int:7 int:4 text:sprig`
- line item 22: `int:22 int:7 int:5 null`
- line item 99: absent, empty batch
- line item 23: absent

Fixture writer, after the owner has bound the catalog and before the owner's reads, commits exactly:

1. `UPDATE rev.item SET label = 'willow', "odd""note" = 'kept' WHERE id = 7`
2. `UPDATE rev."line item" SET qty = 9 WHERE id = 21`
3. `INSERT INTO rev."line item" (id, item_id, qty, memo) VALUES (23, 7, 1, 'fresh')`

Fresh generation after that commit:

- item 7: `int:7 text:willow bool:true text:A116 text:kept`
- line item 21: `int:21 int:7 int:9 text:sprig`
- line item 22: `int:22 int:7 int:5 null`
- line item 23: `int:23 int:7 int:1 text:fresh`

`rev.bump()` is not security definer. `rev.canary.n` starts at 0. The three writer statements move it by 3. An owner-only window moves `n` and `n_tup_ins/upd/del` on `item`, `line item`, and `canary` by 0.

`rev.twin` holds `id=3, label=one`. Child `rev.twin_more` inherits it and holds `id=3, label=two`. A plain `SELECT` from `rev.twin` where `id=3` returns 2 rows. The owner probe of that key returns `probe_key` and not `SnapshotFinished`.

## Refusals before reserve

These do not consume the generation and do not leave an `oo-pg-source` backend:

- generation 0 → `generation_invalid`
- past deadline → `deadline`
- flipped config identity → `config_identity`
- flipped type-policy identity → `type_policy_identity`
- admitted table set shorter than the selection → `selection`

The same generation then opens.

## Lifecycle

The open owner is `idle in transaction` on the admin view of `(pid, backend_start::text)`, application `oo-pg-source`, user `oo_rev_reader`, database `revdb`, `xact_start` set.

`finish` returns `SnapshotFinished` only after that row is gone within 5000 ms and is not left `idle in transaction`. `abort` returns `SnapshotAborted` on the same observation. `Drop` returns neither type. Release is the activity row disappearing, not a dropped future.

Reuse of a consumed generation returns `generation_reused` and not `SnapshotFinished`. A new generation reads the fresh cut.

Deadline expiry returns `deadline` and not `SnapshotFinished`, and the same backend row is gone within 5000 ms.

`pg_terminate_backend` runs only when pid, `backend_start::text`, application name, user, and database all match. The owner then returns an error, not `SnapshotFinished`, and does not reconnect under that generation.

## Admission, catalog, ACL

Wrong expected principal → `principal`. Wrong database → `database`. Wrong postmaster text or `expected_in_recovery=true` → `read_node`. Role `oo_rev_guc` has `ALTER ROLE ... SET role` and returns `startup_options`. Wrong CA and the alias `oo-a116-wrong` return `tls_handshake`. Each connected failure releases its backend and does not return `SnapshotFinished`.

A tampered admitted type oid returns `catalog_identity`.

`PgSession::catalog` from a moment when `SELECT` is granted is not current admission. After `REVOKE SELECT` on `rev.item`, the next `SnapshotOwner::open` returns `select_not_granted`.

In-flight `REVOKE SELECT` on `rev.item`, and in-flight `REVOKE SELECT (memo)` on `rev."line item"`, each make the probe return `select_not_granted` or `insufficient_privilege` with SQLSTATE `42501`. `finish` is not `SnapshotFinished`.

In-flight `ADD COLUMN` is compared with an independent repeatable-read `pg_attribute` count whose transaction began before the `ALTER`. If that count stays on the old shape, the owner probe returns the old column list and may `finish`. If that count sees the new column, the probe returns `schema_drift` and `finish` is not `SnapshotFinished`. The next generation opened with the pre-alter admitted shape returns `catalog_identity`.

`GRANT INSERT` on `rev.item` makes open return `write_capable`.

Empty key and a text value for an int key return `probe_key`.

`connector_capabilities` stay unsupported, including `bounded_read` and `type_conversion`. Catalog `type_conversion` is `unsupported`. Column `type_support.converted` is false. Batch protocol is `oo.source.pgsql.types/v1`. Owner protocol is `oo.source.pgsql.snapshot/v1`.

## Audit

`log_statement=all` is reloaded before marker `A116_AUDIT_WINDOW_START`, and the log is not truncated. The marker is nonempty and precedes every `oo-pg-source` statement. Inside the window, statements for user `oo_rev_reader` and application `oo-pg-source` contain no `INSERT`, `UPDATE`, `DELETE`, `TRUNCATE`, `MERGE`, `COPY`, `ALTER`, `CREATE`, `DROP`, `GRANT`, `REVOKE`, `COMMIT`, or `VACUUM`. `SELECT`, `WITH`, `START`, `ROLLBACK`, and `SET` are the read and session classes. Writer, admin, oracle, and GUC statements are counted apart. A password or `PRIVATE KEY` in the log fails the run.
