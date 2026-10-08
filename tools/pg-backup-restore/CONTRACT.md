# pg-backup-restore/v1

Status: cut rule amended by session `eef831aa-dba4-4af2-a78b-541bc9e0a377` after `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a377/SP14/pg-backup-catalog-cut/CONTRACT-FREEZE.md`. The a368 freeze remains the rule for table `SHARE` locks, the marker, and the entry count. This file is not a development unit and does not accept a parent goal. Fixture names stay the a356 names.

Session `eef831aa-dba4-4af2-a78b-541bc9e0a356`. Fixed product archive `7a6f7eac8b8041eae215c087d1a80930437871ce`. Owned paths are `tools/pg-backup-restore/**`, `testing/real-db/pg-backup-restore/**`, and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a356/SP14/pg-backup-restore/**`.

No existing tool under `tools/` performs PostgreSQL backup or restore. `local-pg-mirror` is a read-only snapshot copy and is not this protocol. This crate does not wrap it and does not edit it.

## Caller

`caller` is `trusted_local_admin`. The program reads one local operator file and one password file. It is not public authentication, not a Gateway, and not a console login. A different caller is refused.

Lineage version is `pg-backup-restore/v1`. Manifest version is `oo-pg-backup-manifest/v1`. Report version is `oo-pg-backup-readiness/v1`. Source cut is `cut-a356-1`. Target fence is `fence-a356-restore`. Owner is `a356`. Session is this UUID.

## What is in the slice

One private PostgreSQL 17.10 instance. Two source databases and two fresh restore databases on that same instance.

| Database | Contents |
| --- | --- |
| `oo_a356_control` | Actual `cdc-offset-store-v1`, `session-store-v1`, and `calendar-store-v1` SQL, plus the key-registry fixture |
| `oo_a356_agent` | Public shop fixture, 100000 typed rows, plus actual `enum-store-v1` SQL |
| `oo_a356_restore_control` | Fresh database. Created only when absent. Never a name outside the fence |
| `oo_a356_restore_agent` | Fresh database. Same rule |

`calendar-store-v1` and `enum-store-v1` both execute `CREATE SCHEMA oo_dict`. They cannot both be applied, unmodified, to one database. Enum is loaded into the agent database for this fixture. That placement is not a claim that enum belongs in the agent product database. `enum_calendar_same_database` stays a remaining family.

The key registry is a fixture. The product has no `CREATE TABLE` for issued entity keys or negative decisions in this archive. The fixture tables are `oo_key_fixture.issued_key` and `oo_key_fixture.negative_decision`. Keys `k-0001` and `k-0002` are issued once. The pair is a `negative` decision. Restore must not add a key, must not reuse a key, and must not drop the negative pair.

The shop row contract is the public fixture from `testing/real-db/pg-e2e/CONTRACT.md`: `shop.party` 2000, `shop.account` 8000, `shop.entry` 90000. Role names in that file are `oo_a317_*`. This slice uses `oo_a356_reader` and `oo_a356_writer`. The row formulas are unchanged. `COMMENT ON TABLE shop.party` is `甲注评名录`.

Checkpoint rows that must survive as stored bytes:

- CDC watermark `pubfix` / `batch-cursor-us/v1` / `lineage-a356` / epoch `1`, applied position `100000`, applied transaction `txn-a356-1`, declared next `txn-a356-2` at `100001`. One matching mirrored receipt. The program does not publish `min(control position, any other position)`.
- Calendar snapshot closure date `2026-03-31`, label `2026-03`, and the published `last_closed_period_end_date` of that same date.
- Session head `sess-a356-1`, revision `3`.
- Enum published code `confirmed`, status `approved`.

`install_manifest.sql_sha256` is the SHA-256 of the exact migration file bytes that were applied.

## Protocol

Password bytes live only in the mode-`0600` password file and in a mode-`0600` `PGPASSFILE` that the program creates and deletes. The password is not a process argument, not `PGPASSWORD`, and not an inherited environment variable. Child processes start from a cleared environment. Captured output is scanned and the password is stripped before any report is written.

`pg_dump`, `pg_restore`, and `psql` are absolute paths inside the pinned `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` image. Each `--version` line must start with the program name and contain `(PostgreSQL) 17.10`. A host `PATH` lookup is not used. A path containing `homebrew` or `Cellar` is refused.

Direct exec of the Rust 1.94 binary inside that image failed before any SQL: the binary needs glibc 2.39 and the image linker does not provide it. Dump and restore flags were not changed. The harness invokes the same binary with a private copy of `ld-linux-aarch64.so.1`, `libc.so.6`, and `libgcc_s.so.1` taken from the pinned Rust image. Those three files are not a replacement of the image libc. Child processes still start from a cleared environment, so `pg_dump`, `pg_restore`, and `psql` stay the image binaries.

`pg_dump` flags: `--format=custom --no-password --snapshot=<id> --lock-wait-timeout=15000 --encoding=UTF8 --file=<backup>/<db>.dump` plus host, port, username, and dbname. There is no `--serializable-deferrable`, no `--table`, and no schema filter. Each source database passes its own exported snapshot id.

`pg_restore` flags: `--no-password --exit-on-error --single-transaction` plus host, port, username, and the fresh dbname. No `--clean`. No `DROP DATABASE`. No `pg_dumpall`. Role password hashes are not exported.

Before the gold `COPY` and both dumps, one session on each source database starts `REPEATABLE READ`, runs `SET ROLE oo_a377_operator`, takes `SHARE` on every user table, restates each user sequence with `ALTER SEQUENCE ... OWNED BY` (the current owner, or `NONE`), and calls `pg_export_snapshot()`. The table locks and the sequence `ShareRowExclusiveLock` stay held through the gold `COPY` and both dumps. Table `SHARE` blocks writers, including the table owner, and does not conflict with `pg_dump`. `ShareRowExclusiveLock` blocks `nextval` and `setval`, including a backend that still has cached sequence values, and does not conflict with `pg_dump`. While the locks are held, a writer statement on each database must be rejected with `lock_not_available`. Gold `COPY` and the sealed content digests run in a second session that imports that database's snapshot. The two snapshot ids must differ. The manifest binds `snapshots`, `catalog_sha256` of `catalog.txt`, `cut_started_unix`, `cut_finished_unix`, the sequence fence, the writer blocks, and the relation digests. `history_insufficient` is false only when that record is proven, including `sequence_fence.held`. `quiescent` is the opposite bit. It is not a caller field. An operator file that contains `quiescent` is refused.

The owned fixture creates `oo_key_fixture.cut_seq` and `shop.cut_seq` at `START WITH 41` and does not call `nextval`. It registers `oo_a377_owner` and `oo_a377_operator`. The owner is `NOSUPERUSER` and owns the user tables, views, and sequences. The operator is `NOSUPERUSER INHERIT` and inherits the owner. The cut uses `SET ROLE oo_a377_operator`. Sequence values are not MVCC. `OWNED BY` is the only `ALTER SEQUENCE` option that does not rewrite the sequence tuple. The cut reads `last_value`, `is_called`, and `log_cnt` before and after that statement and withholds the sequence if they differ. It does not call `nextval`, `setval`, `RESTART`, `CACHE`, or `INCREMENT`. `sequence_fence.held` is true only when every user sequence visible in the snapshot accepted that statement (`sqlstate` `00000`) and the cut session still holds `ShareRowExclusiveLock` on each of those sequences after both dumps. `sequence_fence.lock_mode` is `ShareRowExclusiveLock` and `sequence_fence.method` is `alter_sequence_owned_by`. A refused statement records the SQLSTATE, sets `sequence_fence.held` false, and withholds the cut. The manifest still stores the snapshot ids, the catalog hash, the sequence rows, the writer blocks, and the digests. Empty proof arrays are not a substitute.

The cut marker and `count(*)` of `shop.entry` are read under the locks, before and after the dumps. They can withhold the seal when they move or are not `cut-a356-1|100000` and `90000`. They cannot establish a cut. A value update that leaves them unchanged is invisible to those samples. WAL LSN movement sets `wal_moved` only. `pitr_ready` stays false. This is not WAL archiving and not pgBackRest.

If the locks are not acquired, or a probe is not rejected, backup returns before writing a manifest or a dump and releases the sessions. If the dumps exist and the sequence fence, snapshots, digests, locks, marker, or count do not prove the cut, the manifest is `cut_mode` `unproven`, `history_insufficient` true, and `quiescent` false. Restore of that manifest is exit 4 before `CREATE DATABASE` and before `pg_restore`. A dump, gold, or catalog hash mismatch is the same exit, also before `CREATE DATABASE`. The refusal report has `replay_ready` false, `offset_published` null, and `history_insufficient` true. Unknown history is not reported as sufficient. `replay_ready` is not a constant false: it is the existing restore checks plus a proven cut.

The manifest is one JSON object written once. Its sidecar is the SHA-256 of those exact bytes. Dump files and checkpoint `COPY` files are named in the manifest with their SHA-256. A second backup into an existing manifest is refused. Missing manifest, sidecar mismatch, dump mismatch, or gold-file mismatch writes `replay_ready=false` and does not call `pg_restore`.

Restore creates a fenced database only when `pg_database` has no such name, using `TEMPLATE template0 ENCODING 'UTF8'`. An existing database that already has a user table, view, or materialized view is refused. Names outside the two restore databases are refused.

After restore, shop `COPY` text is compared with the Rust generator for all 100000 rows. Checkpoint `COPY` text is compared with the files taken at the cut. Probes compare counts, `numeric(18,4)` text, NULL, primary keys, the comment, foreign-key orphans, the CDC position, the calendar closure, the session revision, the enum row, and the two issued keys. A row inserted after the manifest is sealed (`shop.party` `p90000`, key `k-9000`) must remain on the source and must be absent from the restore.

`replay_ready` is true only when every check above passed, `history_insufficient` is false, and `cut_proven` is true. `cut_proven` is true only for the writer-blocked record above. These fields are constants even then:

- `all_state_certified`: false
- `w04_gate`: `open`
- `production_slo_certified`: false
- `parent_acceptance`: false
- `production_ops`: false
- `offset_min_used`: false

When history is insufficient the published offset is withheld. It is not the minimum of the control position and any other position.

`rpo_window_seconds` is the integer seconds from the loader's last included write to the manifest seal. `rto_seconds` is the integer seconds from restore start through the readiness report. Both are measurements. They are not a certificate that the 5-minute or 1-hour production targets were met.

## Remaining state families

The manifest lists these as not included: `git_mirror`, `object_store`, `secret_store`, `audit_anchors`, `valkey_buffer`, `jobs_control`, `gateway_tokens`, `file_hosting`, `ontology_publication`, `agent_pipeline_schema`, `entity_materialized_tables`, `cluster_roles`, `wal_archive`, `pgbackrest`, `enum_calendar_same_database`.

AT-OPS-06 and SP14-W04 stay open. This slice does not certify full-state recovery.

## Resources

One Rust builder, 6 GiB, 1 CPU, including host compiler processes and image inventory. One private PostgreSQL 17.10, 1 GiB, 1 CPU. Images are local. The program does not pull, prune, or publish a port. No privileged mode, host network, Docker socket, host home, shared cargo cache, or auth environment. Cleanup removes only objects whose `oo.owner` is `a356` and whose `oo.session` is this UUID.

| Use | Value |
| --- | --- |
| Network | `oo-a356-eef831aa-net` |
| PostgreSQL name and alias | `oo-a356-pg` |
| Volumes | `oo-a356-eef831aa-cargo`, `oo-a356-eef831aa-target`, `oo-a356-eef831aa-pgdata` |
| Private tmp | `/tmp/oo-a356-eef831aa-dba4-4af2-a78b-541bc9e0a356` |
| Admin login | `oo_a356_admin` |

## Recipe origins

Shell and server flags start from the reviewed a274 chain. Copied `pg/pg-wrap.sh` keeps the a274 server flags. Launch changes are the UUID, the 6 GiB and 1 CPU limits, this network, and this fixture.

| Origin | Path | SHA-256 |
| --- | --- | --- |
| a274 pg-wrap | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a274/SP11/pg-onboarding-review/pg/pg-wrap.sh` | `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb` |
| a274 up | same review, `pg/up.sh` | `ec1dd1087fddfebfe22c89ee21677ed6c81eba75f7ba4e6a55b2d867daa02c76` |
| a274 focused launch | same review, `linux/focused/launch.sh` | `e9fd0027356f5fc90ebe56c0070f1078ba9f0f8d6468fbbecd4db98b4ff16910` |
| a274 sha256 tool | same review, `tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |

The SHA-256 implementation in this crate is that reviewed compression function, as a library. The file hash differs because the CLI wrapper is not copied.

## Non-claims

`parent_acceptance` is false. `development_unit` is false. A passing restore is not AT-OPS-06, not a production backup drill, and not an all-state certificate.
