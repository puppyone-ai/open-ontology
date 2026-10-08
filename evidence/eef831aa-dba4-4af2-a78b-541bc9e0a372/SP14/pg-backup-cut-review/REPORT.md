# PG logical cut review

Session `eef831aa-dba4-4af2-a78b-541bc9e0a372`. Fixed product `20f065096fa9e20414eed8951d830333160e4af6`. Working tree at report time `e090131ed9df0f3b4d39f126ddfc389c2d129722`. `git diff 20f065096fa9e20414eed8951d830333160e4af6 HEAD -- tools/pg-backup-restore` is empty. This review did not edit the product.

Verdict: **WITHHOLD**.

Missing clause: `cut_proven` and `replay_ready` are set while mutable relations outside the SHARE proof are inside the certified dump. The fixture already has views `oo_dict.calendar` on `oo_a356_control` and `oo_dict.enums` on `oo_a356_agent`. During the held SHARE window those views were commented, and both databases gained schema `oo_a372_probe`, table `added` with row `7`, sequence `seq` at `last_value` 1, and view `v` returning `7`. The fresh restore contains each of those changes. The success report still has `cut_proven` true, `replay_ready` true, `history_insufficient` false, and `offset_published` `100000`. SHARE is `relkind` `r` and `p` only (`cut.rs` lock SQL). The recorded writer-block list has no view and no `oo_a372_probe` relation. Relation digests stay on the sealed gold list. `proven` requires the original locks, the writer probes, stable sealed digests, the marker, and the entry count. `Report::seal` sets `replay_ready` from those sealed checks.

This is the original E51 logical 100k dump slice. The open PITR and pgBackRest parent does not supply this certificate. The run is not general PostgreSQL support. The product also does not reject other state and keep the certificate on the sealed fixture alone: the other state was accepted into the same `replay_ready` report.

`parent_acceptance` stays false. `w04_gate` stays `open`. `pitr_ready` stays false. `all_state_certified` stays false. No new unit.

## Sealed row path

The sealed shop row did what the writer lock claims.

One private PostgreSQL 17.10, container `oo-a372-pg`, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, 1 GiB, 1 CPU, no published port. `pg_dump`, `pg_restore`, and `psql` each reported `(PostgreSQL) 17.10`. Tool directory `/usr/bin`. Password file mode `600`. `pg-wrap.sh` matches a274 `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb`.

The Rust fixture loaded 100000 rows. `locks-seen.txt` is `seen=1 agent=1 control=1` on `shop.entry` and `oo_key_fixture.cut_marker` before the probes. An independent `UPDATE shop.entry SET amount = 1.0001 WHERE entry_id = 3` with `lock_timeout` `1500ms` exited 3. `live/probes/rejected.err` contains `55P03` and `canceling statement due to lock timeout`. Amount during the lock stayed `1234567890.1234`. Marker stayed `cut-a356-1|100000`. `shop.entry` count stayed `90000`. After the cut sessions dropped (`cut-sessions-after.txt` is `0`), the same update committed (exit 0) and the source amount became `1.0001`. The restored amount is `1234567890.1234`. Party `p90000` is present on the source (`1`) and absent on the restore (`0`). A second restore exited 4 and left the restored amount unchanged.

Probe clock from `timing.txt`: `probe_start=1791417517`, `probe_ddl_done=1791417517`, `probe_reject_done=1791417519`, `backup_outer_end=1791417520`. After the DDL sessions returned, `agent-lock-after.out` and `control-lock-after.out` were still `1`, and the value update had not yet returned. The `55P03` and the DDL share that lock window.

Corrupt dump: one zero byte written into `oo_a356_agent.dump` before any restore database existed. `dbs-before-corrupt.txt` and `dbs-after-corrupt.txt` are `none`. Restore exit 4. Refusal report: `replay_ready` false, `cut_proven` false, `history_insufficient` true, `offset_published` null, `reason` `dump_hash_mismatch`.

Good restore exit 0. Manifest `cut_mode` is `writers_blocked`, `quiescent` true, `history_insufficient` false, `wal_moved` false, entry count `90000` before and after, LSN `0/2B49530` before and after. Dump hashes:

- `oo_a356_agent.dump` `43935e2e18d631e1a044d190a5b7e84f2f3045b6a0bc53646b38f45b63e4031d`
- `oo_a356_control.dump` `801b515442bc2575e9ae059b3f9850f35127a47b3ae7910c85a66ba467dbfbb5`

Shop gold matches the a361 independent oracle and `shop.entry` is bound to it:

- `shop-party.copy` `62230764135b6d717fe792dffa77d776d0e7cdc7ff2123fd049ce4f56f06fa5b`
- `shop-account.copy` `fef89f25c29e9ed5b9c36d0bf37f3056433c33ade60b90978d4f91e7c0c7a958`
- `shop-entry.copy` `a11369572ab597c2211d47df03b92c2a1b2da88451bfb5439c1d7ecf2cb10450`
- `shop.entry` `content_md5` `695209017bab3fc1c6cd1e9b6607ef20`

`gold.rs` git blob `2564210b226be3d6980265a1dc429cfadf2a613f` is the same at the a361 base and at `20f065096`. The 101-check campaign was not rerun (`oracle-101-rerun.txt` is `0`). `cut-check` exit 0 recomputed the manifest sidecar, both dump hashes, the sealed gold files, the checkpoint names, and the `55P03` probe. Two sequential dumps are not recorded as one shared snapshot.

`rpo_window_seconds` is 3 and `rto_seconds` is 1. Those are measurements from this run.

## Boundary inside the same certificate

View and sequence ShareLock counts during the window were `0` and `0`. Catalog rows already present: `oo_dict.enums:v` on the agent database and `oo_dict.calendar:v` on the control database. Those views are `CREATE VIEW` in the fixture SQL at the fixed commit (`oo_calendar_control_v1.sql` line 227, `oo_enum_control_v1.sql` line 257). Loaded file bytes match that commit (`ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520` and `b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd`). Enum and calendar migration files were not edited.

Each DDL step on both databases raised `step comment ok`, `step schema ok`, `step table ok`, `step insert ok`, `step sequence ok`, `step nextval ok`, and `step view ok`. Source and restore both show comment `a372-during-cut`, `oo_a372_probe.added` count `1`, sequence `last_value` `1`, and view `n` `7`.

## Linux 1.94

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. No `cargo update`. No root manifest change. No generate.

| Phase | Exit |
| --- | --- |
| pre-edit smoke inside | 0 |
| product `cargo test --locked --offline` | 0 |
| product `cargo clippy --all-targets -- -D warnings` | 0 |
| product release build | 0 |
| `cut-check` test | 0 |
| `cut-check` clippy | 0 |
| `cut-check` release build | 0 |
| load-fixture | 0 |
| backup | 0 |
| rejected update | 3 |
| mark-post-cut | 0 |
| restore of corrupt dump | 4 |
| restore | 0 |
| second restore | 4 |
| cut-check | 0 |
| run-live | 0 |

The backup binary ran through a private copy of the Rust image loader (`ld-linux-aarch64.so.1`, glibc 2.41). `pg_dump` stayed the PostgreSQL image binary. Loader usage with no arguments exited 2.

## Limits

SP14-W04 and AT-OPS-06 stay open. PITR, WAL archiving, pgBackRest, cluster roles, and `enum_calendar_same_database` stay open. `production_ops` and `production_slo_certified` stay false. `offset_min_used` stays false. a365 owns the enum and calendar product migrations.
