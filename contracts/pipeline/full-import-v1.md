# full-import/v1

Unpublished PostgreSQL full import. Protocol `full-import/v1`. This is the production entry that reads one source snapshot and writes one target generation. It is not a publication, not a permission grant, and not a control-database mirror.

## Entry

`oo_pipeline::full_import::import_unpublished` takes one idle target `PgConnection` and one `ImportRequest`. The connection must already be able to adopt a shard. The function returns `RunSummary` only when the run state is `awaiting_publish`. Installing the schema does not return that summary.

`ImportSession` is the same entry split so a test can hold one uncommitted batch. `stage_next_uncommitted` borrows the caller's connection for the open transaction. `commit_ambiguous` commits that transaction and returns the receipt identity. It does not claim the import is finished. `pull_and_apply` commits and then advances the in-memory coverage.

The source side is `SnapshotOwner`. Completion calls `finish_complete` and requires `RunComplete`. `SnapshotFinished` is not an input. A caller cannot build `RunComplete`.

## What one run does

The entry verifies `full-import/v1` readiness, adopts the shard, and binds the backend pid and `backend_start`. It opens the source snapshot, refuses a selected oid outside the approved COPY set, and registers namespace, table, and column physical names for that source generation. It then opens one run and creates one `LOGGED` shadow per selected table.

Rows move one batch at a time. The public encoder produces the binary COPY. The same transaction copies the rows, writes masked quality, and calls `commit_apply_receipt`. Nothing in that transaction is collected into a dataset. The reader queue depth stays one.

`awaiting_publish` is stored only when every selected table is exhausted, the reader returned `RunComplete`, and the source, config, type, input, schema, and generation bindings match the run and the applied coverage. Seal is a later transaction than the batch commits. Two databases are not one transaction.

Cancel, a source deadline, a lost source snapshot, or a typed refusal retires the open run. A later source generation reads every selected table again. Chunks from the retired snapshot are not reused. An unfinished or retired shadow has no `SELECT` grant for the Agent.

## Fence

`oo_target_owner` is `NOINHERIT`. `install_full_import` therefore grants `pg_read_all_stats` to that role `WITH INHERIT TRUE, SET FALSE` before changing role, and `0005` refuses to continue unless `pg_has_role(..., 'usage')` is true. `SET FALSE` means the owner cannot assume the stats role. `oo_pipeline_runtime` and an Agent login are not members. The membership exists so the security-definer fence can read the caller's `backend_start`. It does not replace the pid, owner, generation, or held shard-lock checks, and it is not a security proof. Every stage function checks the current backend, the bound session row, the owner, the generation, and the held shard advisory lock. The receipt writer gate is cleared before the function returns. Runtime `INSERT` on a shadow is granted so COPY can run, and triggers reject a write that is not an insert into an open run by the bound backend. Direct catalog DML raises `OO014`. An Agent login with only `CONNECT` cannot read the shadow or the quality table.

The receipt identity is `(source, physical table name, batch id)`. The same payload digest is `replay` and does not start COPY. A different payload raises `OO010` and rolls back the copy, the quality rows, and the receipt together. Fence generation is stored and is not part of that equality. After an ambiguous commit, a later connection reads the receipt. It does not copy the batch again when the digest matches.

## Migration

`0005_full_import_expand.sql` runs only through `install_full_import`, and only after the production installer rows are exact. `install` does not execute it. A bad `full-import/v1` checksum raises `OO001` and does not change the row. The SHA-256 of the file is `a9cef2013019784857353ff4fe060350d71a4e9ac4ab18551b992a7a9b52ecb6`.

Approved COPY oids are bool, int2, int4, int8, float4, float8, text, varchar, bpchar, json, bytea, uuid, date, time, timestamp, timestamptz, and numeric with scale at least zero. jsonb, interval, arrays, enums, and a negative numeric scale are refused. A missing value is not written as NULL. SQL NULL is copied as NULL. Unclassified raw values stay out of the quality table; the quality row stores a reason, a length, and the input digest.

## What this is not

No user classification. No publication. No automatic privileges. No control-database job, mirror, or stream acknowledgement. No CDC, merge, or tombstone. The second-database installer limitation is unchanged: a cluster that already has `oo_target_owner` is not a fresh bootstrap.
