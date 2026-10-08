# Findings

## Ordinary sequence fence holds

`ALTER SEQUENCE ... OWNED BY` of the current owner, or `NONE`, takes `ShareRowExclusiveLock` and does not change `relfilenode`, `last_value`, `is_called`, or `log_cnt`. Measured on `mech.plain` and `mech.owned` (`16391|41|f|0`, `16392|41|f|0`) and on the frozen sequences (`shop.cut_seq` `16791|41|f|0`, `oo_key_fixture.cut_seq` `16636|41|f|0`) before, during, and after the product cut.

A committed `nextval` that had filled `mech.cached` to `10|t|32` could not take another value, and `setval` could not run, while another session held `ShareRowExclusiveLock`. Both waited and raised `55P03`. The disk row stayed `10|t|32` until that transaction committed. The same shape held for `shop.cut_seq` and `oo_key_fixture.cut_seq`: external `nextval` and `setval` raised `55P03`, and the rows stayed `41|f|0`.

`CACHE` and `INCREMENT` do rewrite `relfilenode`. `ROLLBACK` restored the node and `41|f|0`. `LOCK TABLE` on a sequence is `42809`, which is why the fence is `ALTER` rather than `LOCK TABLE`.

`pg_sequence.xmin` changed inside the open `OWNED BY` transaction (`742` to `757`) and returned to `742` on `ROLLBACK`. That is the catalog tuple update. The sequence data fields did not move.

## The 100k logical backup restored

Backup exit 0, `cut_mode` `writers_blocked`, both sequence SQLSTATEs `00000`. Dumps are custom format, bound to two different `REPEATABLE READ` snapshots, with no schema filter and no table filter. Shop party, account, and entry hashes and the bundle hash match the reviewed gold. Catalog sha256 `8819c6e48c6595d8a5e1cebac12baa2576d1502bb35dda9b43f291b6f7373bcc`. Counts `2000|8000|90000`. Restored amount `1234567890.1234`. Party comment `甲注评名录`. Views `oo_dict.enums` and `oo_dict.calendar` are in the dump and in the restore.

Objects created after the snapshot export are on the source and absent from the dump and the restore: schema `oo_a391_probe`, row `7`, and the during-cut view comments. Post-cut party `p90000` and key `k-9000` were on the source and absent from the restore. After the locks were released, the source accepted the amount update and a later `nextval`.

A one-byte corrupt agent dump restored with exit 4, reason `dump_hash_mismatch`, while the restore database list was still empty. The good restore then exited 0. A second restore exited 4, reason `restore_database_not_fresh`.

WAL LSN moved from `0/2F82CD0` to `0/2F8AC58`, so `wal_moved` is true. `pitr_ready` is false. The 4 second backup and 1 second restore are this run's tool windows, not a production SLO.

The manifest's `probes_rejected` flag is written by the product. The rejection evidence is the harness stderr `55P03` on the amount update and on `nextval` / `setval`.

## Identity is withheld, not dropped

The loaded fixture had zero sequences of `deptype` `i`. The product selects every user `relkind` `S`. When `shop.a391_ident_id_seq` was present and owned by `oo_a377_owner`, the fence listed it with SQLSTATE `0A000` and `held` false, and the ordinary sequences in that same manifest stayed `00000`. `cut_mode` was `unproven`. Restore exited 4 with `cut_unproven` and the database list did not grow.

On the private server, `ALTER SEQUENCE` of that identity sequence to `NONE` and to the same column both raise `0A000 cannot change ownership of identity sequence`.

The first identity attempt did not reach this result. It is kept under `failed-attempts/identity-1/`. The retry is the measurement.

## Authorization of the cut role

The dump user is the fixture superuser `oo_a356_admin`. The cut role is `oo_a377_operator`: not a superuser, member of `oo_a377_owner`, and the sequences are owned by `oo_a377_owner`. A stranger with `USAGE` and `UPDATE` cannot `ALTER` the sequence (`42501`). The superuser grant of `oo_a356_admin` to the mechanism operator was revoked, and the membership list after that revoke does not include it.

## What this review does not close

Parent acceptance, PITR, pgBackRest, production operations, production SLO, `offset_min_used`, and `w04_gate` stay as recorded in the readiness files. The tool set `replay_ready` true on the ordinary restore. That flag is not an acceptance of E51 beyond the limited ordinary-sequence result, and it is not an acceptance of the parent goal. The 101-check oracle was not rerun. `gold.rs` is the blob already reviewed.
