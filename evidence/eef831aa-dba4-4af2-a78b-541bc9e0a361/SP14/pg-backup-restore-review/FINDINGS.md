# Findings

Source is `1d88fe8ad9fa4a113c105e1df4d0903e517fc9a9`. Live numbers are from this a361 cluster, not from the a356 packet.

## Verdict on the frozen backup

`WITHHOLD`. The original E51 backup does not establish one quiescent cut. The 101-check oracle was not rerun and is not a substitute for that cut.

## Two sequential dumps are not one cut

`ops.rs` `backup` does this, in order:

1. Read `pg_current_wal_lsn()` on `oo_a356_control`, the cut marker `source_cut || '|' || entry_count` from `oo_key_fixture.cut_marker`, and `count(*)` of `shop.entry` on `oo_a356_agent`.
2. `COPY` the gold files, one statement at a time, including shop rows from the agent database and checkpoint rows from the control database.
3. `pg_dump` `oo_a356_control`.
4. `pg_dump` `oo_a356_agent`.
5. Read the control LSN, the cut marker, and the entry count again.

Each dump is `pg_dump --format=custom --no-password --serializable-deferrable --lock-wait-timeout=15000 --encoding=UTF8`. Each call has its own snapshot. Nothing holds a lock that blocks writers on both databases from the first gold `COPY` through the second dump. Restore flags are unchanged: `pg_restore --no-password --exit-on-error --single-transaction`, no `--clean`, no `DROP DATABASE`, no `pg_dumpall`.

`history_insufficient` is true only when the marker is not `cut-a356-1|100000`, when the marker changes, or when the entry count is not 90000 on both reads (`ops.rs`). `manifest.rs` sets `quiescent` to the opposite of that flag. `decide.rs` `seal` withholds the published offset and appends reason `history_insufficient` only when the flag is true. Otherwise `replay_ready` becomes true when the other row checks passed.

`CONTRACT.md` line 53 states the same rule and adds that WAL LSN movement does not by itself withhold a logical snapshot whose marker and counts stayed still. Line 61 then sets `replay_ready` only when `history_insufficient` is false. That is the frozen claim under review.

A value update that leaves the marker row and `count(*)` of `shop.entry` unchanged is invisible to the flag. These writes do that:

- `UPDATE shop.entry SET amount = ...` or `SET memo = ...` (the count stays 90000).
- `UPDATE` or `INSERT` on `shop.party` or `shop.account` (the entry count is untouched).
- `UPDATE` of a control row that is not `oo_key_fixture.cut_marker`, including watermark columns other than that marker.
- A commit on the database that has not been dumped yet, between step 3 and step 4.

The control LSN does not observe an agent-only write, so `wal_moved` can stay false while the agent dump differs from the gold `COPY`. An LSN change on the control database does not say which dump contains the new bytes, and the contract says that change does not withhold the seal. Marker equality, entry-count equality, and the control LSN therefore do not establish that the two dumps and the gold files are the same cut.

On the live run the marker held, both entry counts were 90000, both LSNs were `0/2B41638`, and the tool wrote `history_insufficient=false` and `quiescent=true`. Nobody wrote during that window. A quiet window is not the proof the flag claims. `history_insufficient=false` cannot certify unproven quiescence.

## Mandatory backup correction

The product tree stays readonly. The author of the backup must change the seal before this E51 can be approved. No narrower reading of the same contract is accepted.

`history_insufficient` must stay true, `quiescent` must stay false, the published offset must stay withheld, and `replay_ready` must stay false, unless the backup has one cut that covers every sealed relation in both databases. Marker and count samples may remain as extra signals. They are not that cut.

One of these must be true for the whole interval from the first gold `COPY` through the second dump, and the manifest must record which one:

- Writers are blocked on both `oo_a356_control` and `oo_a356_agent` for that interval, and the manifest records that both databases rejected user writes until both dumps finished.
- Or both dumps and the gold `COPY` files are taken from one coordinated snapshot that both databases share, and each sealed relation's digest is captured inside that snapshot and bound to the dump hashes.

Until that record exists, sealing `history_insufficient=false` on a still marker is the defect. SP14-W04 requires insufficient history to hide replay and to go through a reviewed resnapshot, not to publish the stored offset. This slice does not implement that parent rule, and this verdict does not close W04.

## WAL movement is recorded and does not certify PITR

`wal_moved` is `lsn_before != lsn_after`. This cut's LSNs were equal, so the manifest and the happy report both say `wal_moved` false. `decide.rs` writes `"pitr_ready": false` in the readiness JSON. The happy report's `pitr_ready` is false. An LSN change on a future cut would set `wal_moved` true and would still leave `pitr_ready` false. This run did not observe an LSN change.

## Manifest hash binds the dump bytes

`manifest.sha256` is the SHA-256 of `manifest.json`. The independent oracle recomputed that digest and matched the sidecar. Each dump name in the manifest matched the SHA-256 of the pristine dump file copied out before the corrupt-byte probes. Later corrupt restores operated on a scratch copy and left the oracle's pristine dumps unchanged.

## Refusals stay unread and do not rewrite a populated database

`restore` reads the sealed manifest first. A missing manifest returns `manifest_absent` and a bad sidecar returns `manifest_hash_mismatch`, both through `refuse`, before `ensure_restore_db`. A dump hash mismatch is `verify_names` and also returns before `ensure_restore_db`. Those three live probes exited 4, set `replay_ready` false and `offset_published` null, and left both restore databases absent (`missing_dbs.txt`, `corrupt_dump_dbs.txt`, `corrupt_manifest_dbs.txt` are `none`).

`ensure_restore_db` creates a missing restore database. When the database already exists and has user objects, it returns `restore_database_not_fresh` and does not issue `DROP` or `--clean`. The second restore exited 4 with that reason. Afterwards `shop.entry` was 90000, `p00001` was `n00001`, and `p90000` was 0 on the restore.

## The published offset is the stored control position

`publish_offset` takes `stored` and `_other` and publishes `stored` when history is sufficient. `offset_min_used` is constant false. The live call is `publish_offset(manifest.history_insufficient, &core.offset_stored, Some("1"))`. The ignored argument is `"1"`. The stored watermark position is `100000`. `declared_next_pos` is `100001`. The happy report published `100000` with `offset_min_used` false. The minimum of `"1"` and `100000` is `1`, and that value was not published. The minimum of the stored position and `declared_next_pos` would also be `100000`, so those two numbers alone do not show the function. The ignored `"1"` does.

## Enum and calendar stay with author a365

`tools/calendar-store/migrations/oo_calendar_control_v1.sql` creates schema `oo_dict` and view `oo_dict.calendar`. `tools/enum-store/migrations/oo_enum_control_v1.sql` creates schema `oo_dict` and view `oo_dict.enums`. The E51 fixture applies calendar on `oo_a356_control` and enum on `oo_a356_agent`, so the two `CREATE SCHEMA` statements never meet. On `oo_a361_schema_probe` the calendar file exited 0 and the enum file exited 3:

```
psql:/schemas/tools/enum-store/migrations/oo_enum_control_v1.sql:252: ERROR:  schema "oo_dict" already exists
```

That is the integration gap. No other pair was loaded. Author session a365 owns the schema correction. This review does not patch either migration, the fixture, or the product. The collision is not the mandatory backup correction above, and it is not a reason to waive the quiescence withhold. The family `enum_calendar_same_database` stays in `remaining_state_families`.

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

Verdict `WITHHOLD` on original frozen E51. `w04_gate` remains `open`. The 101 oracle passes stay as recorded and were not rerun. This evidence does not accept the parent goal.
