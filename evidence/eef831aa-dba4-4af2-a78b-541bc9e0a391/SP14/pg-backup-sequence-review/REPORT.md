# Original E51 sequence cut

Session `eef831aa-dba4-4af2-a78b-541bc9e0a391`. Independent review of the immutable product archive `95c33fca0b22b38a2e08c08bb5e2a050f0bf44ec`. The product tree was not modified. `git diff 95c33fca0 -- tools/pg-backup-restore` is empty.

Verdict: `APPROVE_LIMITED` for the frozen ordinary-sequence complete logical backup. Parent acceptance, PITR, production SLO, full SP14, and the parent goal stay open.

## Verdict

The frozen databases `oo_a356_control` and `oo_a356_agent` have no identity sequences. On that scope the cut holds `SHARE` on user tables and `ShareRowExclusiveLock` on user sequences by restating `ALTER SEQUENCE ... OWNED BY` to the current owner or `NONE`. `relfilenode`, `last_value`, `is_called`, and `log_cnt` stay put. `pg_dump` is a full custom dump of each snapshot, and the 100k restore matches the sealed shop gold. A corrupt dump is refused before `CREATE DATABASE`. A second restore of the sealed tree is refused as `restore_database_not_fresh`.

An identity sequence is not omitted. Adding `shop.a391_ident_id_seq` (`deptype` `i`, owner `oo_a377_owner`) makes the fence record SQLSTATE `0A000`, `sequence_fence.held` false, and `cut_mode` `unproven`. Restore of that manifest exits 4 with reason `cut_unproven` and does not create a database.

`replay_ready` and `cut_proven` in the successful readiness file are the tool's report for that ordinary restore. They are not this review's acceptance.

| Flag | Value |
| --- | --- |
| verdict | `APPROVE_LIMITED` |
| product | `95c33fca0b22b38a2e08c08bb5e2a050f0bf44ec` |
| parent_acceptance | false |
| pitr_ready | false |
| production_slo_certified | false |
| all_state_certified | false |
| production_ops | false |
| offset_min_used | false |
| w04_gate | open |
| development_unit | absent |
| identity | withhold when present |

## What ran

Linux arm64 smoke used `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` before the live run. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Lock shape `package_names=1`, `source_lines=0`. Smoke inside exit 0. The smoke container was removed.

One later builder, 6GiB and 1 CPU, ran `cargo test --locked --offline` and `cargo clippy -D warnings`. Lib tests 5 passed. Oracle tests 14 passed. Clippy exit 0. Release binary copied out. `oracle-101-rerun.txt` is 0. `cargo-update-invoked.txt` is `no`. `gold.rs` blob `2564210b226be3d6980265a1dc429cfadf2a613f`. The builder was removed before PostgreSQL started. Global rust-builder count at the glibc copy was 1, own count 0, host cargo 0.

One private PostgreSQL 17.10, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, 1GiB and 1 CPU, no published port. `pg_dump`, `pg_restore`, and `psql` are `(PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`. The recipe `pg-wrap.sh` matches a274 `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb`. The password file in the container is mode 600. The private loader usage exit is 2 and the stderr contains `usage: pg-backup-restore`.

## Mechanism on a database outside the product sources

Database `oo_a391_mech`. Official PostgreSQL 17.10 `AlterSequence` takes `ShareRowExclusiveLock`. `OWNED BY` is the option that does not set `need_seq_rewrite`. `CACHE` and `INCREMENT` do rewrite. Identity ownership change raises `0A000`. `LOCK TABLE` on a sequence raises `42809`. `nextval` locks again when the backend transaction id differs from the cached sequence.

| Check | Result |
| --- | --- |
| `OWNED BY NONE` and same column | `relfilenode\|last_value\|is_called\|log_cnt` stayed `16391\|41\|f\|0` and `16392\|41\|f\|0` during the transaction and after `ROLLBACK` |
| `pg_sequence.xmin` | plain `742` during the open transaction, `757` inside it, `742` after `ROLLBACK` |
| `CACHE 1` / `INCREMENT BY 1` | relfilenode changed (`16391` to `16402`, then `16391` to `16403`) and `ROLLBACK` restored `16391` and `41\|f\|0` |
| `LOCK TABLE mech.plain` | notice `42809` |
| Identity `OWNED BY NONE` and same column | `mech.ident_row_id_seq`, both notices `0A000 cannot change ownership of identity sequence` |
| Stranger with `USAGE` and `UPDATE` | notice `42501` |
| Operator | `rolsuper` false, `ALTER` left `16391\|41\|f\|0` |
| `GRANT oo_a356_admin TO oo_a391_operator` | exit 0, then `REVOKE` exit 0 |
| Cached `nextval` after a committed prime | stderr `55P03 cached-nextval`, disk stayed `10\|t\|32` while `ShareRowExclusiveLock` was held |
| `setval` during that hold | stderr `55P03 cached-setval` |
| After commit | new session `nextval` returned `11`; `mech.plain` stayed `41\|f\|0` |
| `pg_dump --format=custom` while `ShareRowExclusiveLock` was held on `mech.plain` | exit 0, SQL contains `CREATE TABLE mech.item` and `setval('mech.plain', 41, false)` |

`assert.exit` is 0. The same transaction can hold `RowExclusiveLock` and `ShareRowExclusiveLock` together; the waiting sessions still received `55P03` and the on-disk sequence row did not move. `xmin` movement is the catalog tuple update inside the open transaction. It is not a sequence rewrite. `ROLLBACK` restored the old `xmin`.

## Frozen 100k cut

`load-fixture` exit 0. Identity `deptype` `i` count was 0 on both source databases. Roles: `oo_a356_admin` superuser, `oo_a377_operator` not superuser and a member of `oo_a377_owner`, `oo_a377_owner` not superuser. Both `cut_seq` relations are owned by `oo_a377_owner`.

Backup exit 0 in 4 seconds. `cut_mode` `writers_blocked`. `history_insufficient` false. `quiescent` true. `sequence_fence.held` true, method `alter_sequence_owned_by`, lock mode `ShareRowExclusiveLock`. Both sequences SQLSTATE `00000`. Snapshots `0000000C-0000000D-1` and `00000013-0000000A-1`, both `REPEATABLE READ`. Entry count 90000 before and after. Shop gold matches the reviewed files: party `62230764135b6d717fe792dffa77d776d0e7cdc7ff2123fd049ce4f56f06fa5b`, account `fef89f25c29e9ed5b9c36d0bf37f3056433c33ade60b90978d4f91e7c0c7a958`, entry `a11369572ab597c2211d47df03b92c2a1b2da88451bfb5439c1d7ecf2cb10450`, bundle `225c88a436be7735f2df8fddaedc372728c4bd05f07616371e4ca8017d2993d1`. Catalog sha256 `8819c6e48c6595d8a5e1cebac12baa2576d1502bb35dda9b43f291b6f7373bcc` (1 comment, 2 sequences, 25 tables, 2 views).

Sequence rows stayed `relfilenode|41|f|0` before, during, and after the cut (`16791` on `shop.cut_seq`, `16636` on `oo_key_fixture.cut_seq`). During the dump the cut backend's granted modes on `shop.cut_seq` were `AccessShareLock`, `RowExclusiveLock`, and `ShareRowExclusiveLock`. External `UPDATE shop.entry` amount `1.0001` for `entry_id` 3, and `nextval` / `setval` on both preexisting sequences, each failed with `55P03`. After the cut sessions were gone, the same amount update returned `1.0001`. A later source `nextval('shop.cut_seq')` returned `41` and left `41|t`.

Dump flags, one line per database: `format=custom`, `deferrable=absent`, `schema_filter=absent`, `table_filter=absent`, snapshot id present. Dump text contains `CREATE VIEW oo_dict.enums`, `CREATE VIEW oo_dict.calendar`, `COMMENT ON TABLE shop.party IS '甲注评名录'`, and `setval(..., 41, false)` for both sequences. It does not contain `a391-during-cut` or `oo_a391_probe`.

While the dumps were running, both databases accepted a comment change, `CREATE SCHEMA oo_a391_probe`, table `added`, insert `7`, sequence `seq`, `nextval`, and view `v`. Restore probe schema count is 0 on both restore databases. Source probe schema count on `oo_a356_agent` was 1. Both probe schemas contained relation kinds `S`, `r`, and `v`, row `7`, and sequence state `1|t|32`.

WAL LSN moved from `0/2F82CD0` to `0/2F8AC58`. `wal_moved` is true. The contract records that as `wal_moved` only. `pitr_ready` stays false.

Corrupt restore: one byte of `oo_a356_agent.dump` flipped, restore exit 4, readiness reason `dump_hash_mismatch`, restore database list `none` before and after. The pristine bytes were copied back. Host sha256 of both dump copies matches the manifest: agent `8cee207f80b0579e7c21873276981ee1fe5122a0aa78b499e5dba2add496cfce`, control `082134565d9e9a44d431d64467526cde34383605ba9fab53a2b1ea75816af7d9`.

Good restore exit 0 in 1 second. Readiness: `checkpoint_copy_ok`, `comment_ok`, `counts_ok`, `decimal_ok`, `shop_gold_ok`, `post_cut_absent_on_restore`, `post_cut_present_on_source` true. `offset_published` `100000`. `offset_min_used` false. `parent_acceptance` false. `rpo_window_seconds` 5 and `rto_seconds` 1 are this run's tool fields. `production_slo_certified` is false.

Restored sequences `41|f|0`. Restored `shop.entry` `entry_id` 3 amount `1234567890.1234`. Counts `2000|8000|90000`. Party comment `甲注评名录`. Views `oo_dict.enums` and `oo_dict.calendar` present with empty comments. Source party `p90000` count 1, restore count 0. Restore key `k-9000` count 0, source count 1 before the later identity preparation. Second restore exit 4, reason `restore_database_not_fresh`.

The 101-check campaign was not rerun.

## Identity withhold

The first identity attempt is `failed-attempts/identity-1/`. The create query aborted on a `"char"` concatenation, and the backup then exited 1 because the cut role could not use schema `oo_a391_probe`. That restore exit 4 had no manifest.

After the success-path files were saved, the source was put back on the sealed gold image: `shop.entry` amount `3` returned to `1234567890.1234`, party `p90000` was deleted, and key `k-9000` was deleted. Probe objects were given to `oo_a377_owner`. The restore databases were left as restored.

The retry created `shop.a391_ident_id_seq`, owner `oo_a377_owner`, `deptype` `i`. Backup exit 0. Manifest:

| Sequence | SQLSTATE | held |
| --- | --- | --- |
| `oo_a391_probe.seq` on both databases | `00000` | true |
| `shop.cut_seq` | `00000` | true |
| `oo_key_fixture.cut_seq` | `00000` | true |
| `shop.a391_ident_id_seq` | `0A000` | false |

`cut_mode` `unproven`. `history_insufficient` true. `sequence_fence.held` false. Restore exit 4, reason `cut_unproven`. Database list before and after that restore: `oo_a356_restore_agent`, `oo_a356_restore_control`, `oo_a391_mech`. No new database.

## Authorization

`pg_dump` and the fixture connection use `oo_a356_admin`, which the image creates as a superuser. The cut script `SET ROLE oo_a377_operator`. That role is not a superuser, cannot create roles or databases, inherits `oo_a377_owner`, and owns the sequences through that membership. A role with only `USAGE` and `UPDATE` on `mech.plain` cannot `ALTER` it (`42501`). After the measured `REVOKE`, `oo_a377_operator` is a member of `oo_a377_owner` only.

## Limits

This packet does not accept the parent goal, PITR, pgBackRest, production SLO, or the rest of SP14. `w04_gate` stays open. There is no development unit. The ordinary path is a complete logical backup of the frozen databases, with the identity limit above.
