# Findings

Source is `1d88fe8ad9fa4a113c105e1df4d0903e517fc9a9`. Live numbers are from this a361 cluster, not from the a356 packet.

## Quiescence is two dumps and two counters

`ops.rs` `backup` samples `pg_current_wal_lsn()` on `oo_a356_control` only, reads the cut marker on the control database, and counts `shop.entry` on the agent database. It then COPY-exports the gold files, then runs `pg_dump` on the control database and again on the agent database, then repeats the LSN, marker, and entry-count reads.

Each dump is `pg_dump --format=custom --no-password --serializable-deferrable --lock-wait-timeout=15000 --encoding=UTF8` (`pg.rs`). There is no single snapshot that covers both databases. Restore is `pg_restore --no-password --exit-on-error --single-transaction`. The argument list has no `--clean`, no `DROP DATABASE`, and no `pg_dumpall`.

`history_insufficient` is true when the marker is not `cut-a356-1|100000`, when the marker changes across the window, or when the entry count is not 90000 on both reads. `quiescent` is the opposite flag. Gold COPY happens before the dumps. The seal hashes the dump files. It does not treat the gold bytes as the dump. A write that leaves the marker and the entry count unchanged can still seal `history_insufficient=false`. This review did not run that mutation.

On this cut the marker held, both entry counts were 90000, `history_insufficient` was false, and `quiescent` was true. LSN before and after were both `0/2B41638`.

## WAL movement is recorded and does not certify PITR

`wal_moved` is `lsn_before != lsn_after`. This cut's LSNs were equal, so the manifest and the happy report both say `wal_moved` false. `decide.rs` writes `"pitr_ready": false` in the readiness JSON. The happy report's `pitr_ready` is false. An LSN change on a future cut would set `wal_moved` true and would still leave `pitr_ready` false. This run did not observe an LSN change.

## Manifest hash binds the dump bytes

`manifest.sha256` is the SHA-256 of `manifest.json`. The independent oracle recomputed that digest and matched the sidecar. Each dump name in the manifest matched the SHA-256 of the pristine dump file copied out before the corrupt-byte probes. Later corrupt restores operated on a scratch copy and left the oracle's pristine dumps unchanged.

## Refusals stay unread and do not rewrite a populated database

`restore` reads the sealed manifest first. A missing manifest returns `manifest_absent` and a bad sidecar returns `manifest_hash_mismatch`, both through `refuse`, before `ensure_restore_db`. A dump hash mismatch is `verify_names` and also returns before `ensure_restore_db`. Those three live probes exited 4, set `replay_ready` false and `offset_published` null, and left both restore databases absent (`missing_dbs.txt`, `corrupt_dump_dbs.txt`, `corrupt_manifest_dbs.txt` are `none`).

`ensure_restore_db` creates a missing restore database. When the database already exists and has user objects, it returns `restore_database_not_fresh` and does not issue `DROP` or `--clean`. The second restore exited 4 with that reason. Afterwards `shop.entry` was 90000, `p00001` was `n00001`, and `p90000` was 0 on the restore.

## The published offset is the stored control position

`publish_offset` takes `stored` and `_other` and publishes `stored` when history is sufficient. `offset_min_used` is constant false. The live call is `publish_offset(manifest.history_insufficient, &core.offset_stored, Some("1"))`. The ignored argument is `"1"`. The stored watermark position is `100000`. `declared_next_pos` is `100001`. The happy report published `100000` with `offset_min_used` false. The minimum of `"1"` and `100000` is `1`, and that value was not published. The minimum of the stored position and `declared_next_pos` would also be `100000`, so those two numbers alone do not show the function. The ignored `"1"` does.

## Enum and calendar cannot share one database

`tools/calendar-store/migrations/oo_calendar_control_v1.sql` creates schema `oo_dict` and view `oo_dict.calendar`. `tools/enum-store/migrations/oo_enum_control_v1.sql` creates schema `oo_dict` and view `oo_dict.enums`. The operator fixture applies the calendar file to `oo_a356_control` and the enum file to `oo_a356_agent`. Separate databases hide the name collision, and the author's 11 tests do not apply both files to one database.

One probe database, `oo_a361_schema_probe`, received the calendar file (exit 0) and then the enum file (exit 3). stderr:

```
psql:/schemas/tools/enum-store/migrations/oo_enum_control_v1.sql:252: ERROR:  schema "oo_dict" already exists
```

The family `enum_calendar_same_database` remains in `remaining_state_families`. No other pair was loaded.

## Cluster roles, WAL archives, and pgBackRest stay open

Both `pg_restore -l` listings contain 0 `ROLE` lines. They do contain `ACL` lines for objects owned by `oo_a356_admin`. Same-instance restore works because `fixture.rs` already created those roles on this cluster. A restore onto a cluster that does not have the roles is not shown. `wal_archive` and `pgbackrest` are named in the open family list. This tool's output is two custom-format dumps plus a manifest.

## The tool's shop_gold_ok flag is its own generator

`shop_gold_ok` true means the process compared the restored rows with `tools/pg-backup-restore/src/gold.rs`. The review crate `oracle/` is separate, `std` only, with its own SHA-256 and its own row formulas from `testing/real-db/pg-e2e/CONTRACT.md`. It compared the `\copy` text of all 2000 parties, 8000 accounts, and 90000 entries. `failed=0`.

## RPO and RTO

Happy report `rpo_window_seconds` is 1 and `rto_seconds` is 1. Those equal `backup_finished_unix - last_write_unix` and the restore clock window. `docs/11-deployment-and-operations.md` §5 initial targets are 5 minutes / 1 hour for the control database and 15 minutes / 1 hour for the agent database, under pgBackRest. `production_slo_certified` is false.

## Failed attempts

1. `linux/build-attempt-1`: product `test.exit` 0 (11 passed) and product `clippy.exit` 0. Independent oracle clippy exited 101 on `collapsible_if` in `check.rs` and `needless_range_loop` in `sha256.rs`. The builder was removed. PostgreSQL was not started.
2. `linux/live-attempt-1`: after a successful backup, post-cut, three refusals, fresh restore, and existing-database refusal, `\copy` to `/var/lib/postgresql/a361-oracle/shop/party.copy` failed because that directory did not exist yet. `psql` with `ON_ERROR_STOP` exited 3, and `linux/run-all.exit` is 3. The prefix probes were kept. The directory is created before the copy in the corrected `linux/live-phases.sh`.
3. The first resume then matched the scan text inside the two harness copies. `linux/live-attempt-1/resume-tail.console.1` records that message. No PEM was in the tree. The scan now looks for a marker assembled at runtime. The second resume exited 0. `live/password-leak-files.txt` and `live/private-key-files.txt` are `none`.

## Gate

`w04_gate` remains `open`. This evidence does not accept the parent goal.
