# Catalog cut amendment

Session `eef831aa-dba4-4af2-a78b-541bc9e0a377`. Product base `20f065096fa9e20414eed8951d830333160e4af6`. This file is the cut rule for the repair of a372 `WITHHOLD`. It was written before any edit under `tools/pg-backup-restore/`. It does not accept E51, SP14-W04, AT-OPS-06, PITR, cluster roles, pgBackRest, a production SLO, or a parent goal.

a368 `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a368/SP14/pg-backup-quiescent-cut/CONTRACT-FREEZE.md` stays the rule for table `SHARE` locks, the marker and entry-count samples, and the refusal to treat WAL movement as PITR. This amendment adds the dump snapshot and the mutable catalog. It does not replace the 100000-row logical backup. The dump stays a full custom dump of `oo_a356_control` and of `oo_a356_agent`. No `--table`, no `--exclude-table`, no `--schema` filter, and no deletion of views or sequences from the dump. `replay_ready` is not a constant false.

## What a372 measured

While both table `SHARE` locks were held, each source database accepted `COMMENT` on the fixture view, `CREATE SCHEMA`, `CREATE TABLE`, `INSERT` of `7`, `CREATE SEQUENCE`, `nextval`, and `CREATE VIEW`. The fresh restore of the certified dumps contained those changes, and the report still had `cut_proven` true, `replay_ready` true, and `offset_published` `100000`. `SHARE` was taken only on `relkind` `r` and `p`. Before and after fingerprints of the sealed gold relations did not observe that catalog.

## Two snapshots

There is one exported snapshot per source database. There is no cross-database snapshot.

On `oo_a356_control` and on `oo_a356_agent`, one session each runs:

1. `BEGIN ISOLATION LEVEL REPEATABLE READ` before any statement that takes a snapshot.
2. `LOCK TABLE` of the pre-listed user tables (`relkind` `r` and `p`) `IN SHARE MODE`. These are the existing writer fences. They stay held through the gold `COPY` and both `pg_dump` calls, then the sessions commit.
3. A non-value-changing sequence lock attempt, specified below, still before the first snapshot.
4. `SELECT pg_export_snapshot()`.
5. A catalog inventory read in that same transaction: user tables, views, materialized views, and sequences; `sha256` of each view or materialized view definition; `sha256` of each comment; and for each sequence `last_value`, `is_called`, and `log_cnt`.

The snapshot identifier matches PostgreSQL 17.10 `ExportSnapshot` (`%08X-%08X-%d` in `src/backend/utils/time/snapmgr.c`). The two identifiers must differ. `pg_dump` for that database receives `--snapshot=<id>` and does not receive `--serializable-deferrable`. PostgreSQL ignores `--serializable-deferrable` when a snapshot id is set (`pg_dump.c` `setup_connection`). The other dump flags stay `--format=custom --no-password --lock-wait-timeout=15000 --encoding=UTF8`. `pg_restore` flags are unchanged.

Gold `COPY` and the sealed content digests for a database run in a second session: `BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY` then `SET TRANSACTION SNAPSHOT` of that database's id, while the exporting session is still open. A digest read outside that snapshot can show that a writer got through. It cannot by itself prove the cut. Equality of two samples is not a proof (ABA). The recorded cut is the pair of snapshot ids, the inventory SHA-256, the lock list, and the dump hashes.

The manifest records `snapshots` (database, id, isolation `REPEATABLE READ`), `catalog_sha256` of the canonical inventory text, `cut_started_unix` before the gold reads, and `cut_finished_unix` after both dumps and before the sessions commit. `catalog.txt` is that text. Its SHA-256 is `catalog_sha256`.

An object created or a comment committed after `pg_export_snapshot` is not part of the snapshot. `pg_dump --snapshot` must not put it in the dump. The a377 probe (`COMMENT` on the existing view, `CREATE SCHEMA oo_a377_probe`, `CREATE TABLE` plus `INSERT` of `7`, `CREATE SEQUENCE`, `nextval`, `CREATE VIEW`) runs only against these two local databases. It does not edit calendar, enum, or any other product migration.

## Sequence fence

Sequence values are not MVCC. `nextval_internal` calls `init_sequence`, which calls `lock_and_open_sequence`, which takes `RowExclusiveLock` (`sequence.c` `LockRelationOid(seq->relid, RowExclusiveLock)`). The tuple is then updated in place. `pg_dump` `dumpSequenceData` reads `SELECT last_value, is_called FROM <sequence>`, so a `nextval` that lands before that read is in the dump even under a snapshot.

`RowExclusiveLock` conflicts with `SHARE`, `SHARE ROW EXCLUSIVE`, `EXCLUSIVE`, and `ACCESS EXCLUSIVE` (PostgreSQL 17 explicit-locking table). The non-value-changing statement that asks for `SHARE` is `LOCK TABLE <sequence> IN SHARE MODE`. Official PostgreSQL 17.10 rejects it for sequences: `lockcmds.c` `RangeVarCallbackForLockTable` allows only `relkind` `r`, `p`, and `v`, and raises `cannot lock relation` (`42809`). `pg_dump.c` says the backend does not allow locking other relation kinds. `SELECT … FOR SHARE` is also rejected: `execMain.c` `CheckValidRowMarkRel` raises `cannot lock rows in sequence`. Advisory locks do not intersect `nextval`. `ALTER SEQUENCE`, `setval`, `nextval`, and `ALTER … RESTART` change sequence or catalog state and are not the fence. The product must not issue them to manufacture a proof.

The cut session attempts `LOCK TABLE <sequence> IN SHARE MODE` inside a PL/pgSQL block that records `SQLSTATE` and does not issue `nextval` or `ALTER SEQUENCE`. `sequence_fence.held` is true only when every user sequence visible in that database's snapshot accepted the lock (`sqlstate` `00000`) and the cut session still holds `ShareLock` on each of those sequences after both dumps. A database with no user sequence has nothing to fence. A sequence visible in the snapshot and absent from the granted lock list is not held.

The owned fixture creates `oo_key_fixture.cut_seq` and `shop.cut_seq` at load, `START WITH 41`, and does not call `nextval`. The live probe calls `nextval` on those sequences from another session while the cut is held. That probe is the measurement. If the lock is held, the probe must fail with `55P03` and the sequence value must stay the loaded value. If PostgreSQL rejects the lock, the probe is not a value comparison: `sequence_fence.held` is false because the server returned `42809` (or the `ShareLock` is absent). A successful `nextval` confirms the fence is absent. It is not evidence that a lock worked.

If `sequence_fence.held` is false, `cut_proven` is false, `cut_mode` is `unproven`, `history_insufficient` is true, `quiescent` is false, `replay_ready` is false, and `offset_published` is null. Restore exits 4 before `CREATE DATABASE` and before `pg_restore`. The manifest still stores the snapshot ids, the inventory hash, the sequence lock rows, and the table writer blocks so the refused cut can be reconstructed. Empty proof arrays are not a substitute for that record. This refusal is the measured fence result. It is not a hardcoded readiness flag, and it is not a waiver of the 100000-row dump.

`cut_proven` is true only when all of the following hold: the table `SHARE` fences and writer probes from the a368 rule, stable sealed digests, marker `cut-a356-1|100000`, entry count `90000`, two distinct snapshot ids actually passed to the two `pg_dump` invocations, catalog hash bound to `catalog.txt`, and `sequence_fence.held` true. `replay_ready` on a completed restore still requires the existing row, checkpoint, key, and post-cut checks plus `cut_proven`. `pitr_ready`, `all_state_certified`, `production_ops`, `production_slo_certified`, `offset_min_used`, and `parent_acceptance` stay false. `w04_gate` stays `open`.

## Live measurements that must be recorded

Private PostgreSQL 17.10, one instance, the pinned image, the existing 100000-row fixture plus the two local sequences. During the held cut: the a377 catalog probe above on both databases, and `nextval` on both preexisting sequences. `UPDATE shop.entry SET amount = 1.0001 WHERE entry_id = 3` keeps the marker and the count, returns `55P03` during the cut, commits after the sessions end, and the dump's `shop.entry` image keeps `1234567890.1234`. A certified restore, which happens only when `cut_proven` is true, must show that old amount and must not contain `oo_a377_probe` or the during-cut comment. When `cut_proven` is false, the product restore creates no database; `pg_restore -f` of the dump is the inspection that the post-cut objects are absent and the old amount is present. A corrupt dump byte is still refused with exit 4 before `CREATE DATABASE`, `replay_ready` false, and offset null.

Enum and calendar migration files stay with their owner. This repair does not edit them.
