# Writer-blocked cut repair

Session `eef831aa-dba4-4af2-a78b-541bc9e0a368`. Product base `1d88fe8ad9fa4a113c105e1df4d0903e517fc9a9`. This is a repair of the original E51 backup cut. It is not a new acceptance unit. E51 stays unaccepted. `parent_acceptance` stays false. `w04_gate` stays `open`.

The cut rule was frozen in `CONTRACT-FREEZE.md` before the product edits. The program holds `SHARE` locks on every user table in `oo_a356_control` and `oo_a356_agent` from before the gold `COPY` through both `pg_dump` calls. While those locks are held it requires a rejected writer probe on each database and equal content digests of the sealed gold relations. The gold file SHA-256 from that same interval is stored on each relation. Marker and entry-count samples can withhold the seal. They cannot establish it. `quiescent` is the opposite of `history_insufficient`. An operator file that contains `quiescent` is refused. Unknown history is reported as `history_insufficient` true, with no published offset.

## Linux 1.94

Pre-edit smoke and the later build both used `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` on `linux/arm64`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. No `cargo update`. No root manifest change. The unchanged 101-check oracle was not rerun (`oracle-101-rerun.txt` is `0`).

| Phase | Exit |
| --- | --- |
| pre-edit smoke inside | 0 |
| product `cargo test --locked --offline` | 0 |
| product `cargo clippy --all-targets -- -D warnings` | 0 |
| `cut-check` test | 0 |
| `cut-check` clippy | 0 |
| release binaries | 0 |

## Private PostgreSQL 17.10

One container, `oo-a368-pg`, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, 1 GiB, 1 CPU, no published port. `pg_dump`, `pg_restore`, and `psql` reported `(PostgreSQL) 17.10`. Dump and restore flags were not changed. `pg-wrap.sh` is the a274 file `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb`.

The fixture load, backup, post-cut mark, and restore used the product binary through a private copy of the Rust image loader. The loader does not replace `pg_dump`.

Both `SHARE` locks were visible before the adversarial statement (`locks-seen.txt`: `seen=1 agent=1 control=1`). An independent session then ran `UPDATE shop.entry SET amount = 1.0001 WHERE entry_id = 3`. psql exited 3. stderr contains `55P03` and `canceling statement due to lock timeout`. The amount during the lock stayed `1234567890.1234`. The marker stayed `cut-a356-1|100000`. The entry count stayed `90000`.

After backup released the sessions (`cut-sessions-after.txt` is `0`), the same update committed (exit 0). The source amount became `1.0001`. The marker and the count did not change.

A one-byte change to `oo_a356_agent.dump` was restored before any restore database existed. Exit 4. `dbs-before-corrupt.txt` and `dbs-after-corrupt.txt` are `none`. The refusal report has `replay_ready` false, `cut_proven` false, `history_insufficient` true, `offset_published` null, and `reason` `dump_hash_mismatch`.

The good backup then restored with exit 0. The restored amount for `entry_id` 3 is `1234567890.1234`. The post-cut party `p90000` is absent on the restore and present on the source. A second restore exited 4 and left the restored amount unchanged. The success report has `replay_ready` true, `cut_proven` true, `history_insufficient` false, `offset_published` `100000`, `pitr_ready` false, `all_state_certified` false, `production_ops` false, `production_slo_certified` false, `parent_acceptance` false, and `w04_gate` `open`. `wal_moved` on this manifest is false.

`cut-check` exit 0. It recomputed the manifest sidecar, both dump hashes, and the `shop.entry` gold hash. It does not link the product crate.

## Limits

This run does not close SP14-W04, AT-OPS-06, PITR, pgBackRest, cluster roles, runsc, or `enum_calendar_same_database`. Enum and calendar migration files were not edited. Sequences and views are not in the `SHARE` set. RPO and RTO fields are measurements from this run, not a production SLO.
