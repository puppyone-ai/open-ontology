# SP14-W04 PG backup/restore report

Session `eef831aa-dba4-4af2-a78b-541bc9e0a356`. Fixed product archive `7a6f7eac8b8041eae215c087d1a80930437871ce` (ancestor of `HEAD` at the time of the run). Component E51-D1/1. Lineage `pg-backup-restore/v1`.

`development_unit` is false. `parent_acceptance` is false. SP14-W04 and AT-OPS-06 stay open. This packet is the executable slice for one private PostgreSQL 17.10 logical backup and a fresh-database restore.

## Result

The owned orchestrator loaded the fixture, sealed a manifest, refused three bad restores, and restored the sealed backup into fresh databases on the same instance.

| Phase | Exit | Meaning |
| --- | --- | --- |
| load-fixture | 0 | Control and agent databases loaded |
| backup | 0 | Custom-format dumps and manifest sealed |
| mark-post-cut | 0 | `p90000` and `k-9000` inserted after the seal |
| restore-missing | 4 | `reason=manifest_absent`, `replay_ready=false` |
| restore-corrupt-dump | 4 | `reason=dump_hash_mismatch`, `replay_ready=false` |
| restore-corrupt-manifest | 4 | `reason=manifest_hash_mismatch`, `replay_ready=false` |
| restore | 0 | `reason=ok`, `replay_ready=true` |

Happy-path report `live/restore-readiness.json`:

| Field | Value |
| --- | --- |
| caller | `trusted_local_admin` |
| replay_ready | true |
| counts_ok, decimal_ok, null_ok | true |
| shop_gold_ok, checkpoint_copy_ok | true |
| comment_ok | true |
| key_count | 2 |
| key_reused | false |
| post_cut_present_on_source | true |
| post_cut_absent_on_restore | true |
| offset_published | `100000` |
| offset_min_used | false |
| history_insufficient | false |
| quiescent (manifest) | true |
| wal_moved | true |
| pitr_ready | false |
| rpo_window_seconds | 0 |
| rto_seconds | 1 |
| all_state_certified | false |
| w04_gate | `open` |
| production_slo_certified | false |
| production_ops | false |
| parent_acceptance | false |

`rpo_window_seconds` is `backup_finished_unix - last_write_unix`. Both stamps are `1791407179`, so the measured window is 0 seconds at whole-second resolution. `rto_seconds` is 1. These are measurements on this run. They do not certify a 5-minute or 1-hour production target.

WAL LSN moved from `0/2B41778` to `0/2B417B0` during the dump. The cut marker stayed `cut-a356-1|100000` and `shop.entry` stayed 90000, so the logical snapshot was accepted. `pitr_ready` stays false. This run did not archive WAL and did not use pgBackRest.

Databases after the happy restore, from `live/databases.txt`: `oo_a356_agent`, `oo_a356_control`, `oo_a356_restore_agent`, `oo_a356_restore_control`.

## What the backup contains

One private instance. Two source databases and two fresh restore databases.

| Database | Contents |
| --- | --- |
| `oo_a356_control` | `cdc-offset-store-v1`, `session-store-v1`, `calendar-store-v1`, key fixture |
| `oo_a356_agent` | `enum-store-v1`, shop fixture (party 2000, account 8000, entry 90000) |
| `oo_a356_restore_control` | Fresh restore of the control dump |
| `oo_a356_restore_agent` | Fresh restore of the agent dump |

Shop row text matched the Rust generator (`shop_gold_ok`). Checkpoint `COPY` text matched the files taken at the cut. Enum and calendar both create `oo_dict`, so they were loaded into different databases. `enum_calendar_same_database` remains an open family.

Tools were `pg_dump`, `pg_restore`, and `psql` from `/usr/bin` in image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Each version line is `(PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`.

Dump flags: `--format=custom --no-password --serializable-deferrable --lock-wait-timeout=15000 --encoding=UTF8`. Restore flags: `--no-password --exit-on-error --single-transaction`. No `--clean`, no `DROP DATABASE`, no `pg_dumpall`.

## Status that stays false

`all_state_certified`, `production_slo_certified`, `production_ops`, `parent_acceptance`, and `pitr_ready` are false on the ready report and on the refused reports. `w04_gate` is `open`.

Required remaining state families, also listed in the report JSON:

`git_mirror`, `object_store`, `secret_store`, `audit_anchors`, `valkey_buffer`, `jobs_control`, `gateway_tokens`, `file_hosting`, `ontology_publication`, `agent_pipeline_schema`, `entity_materialized_tables`, `cluster_roles`, `wal_archive`, `pgbackrest`, `enum_calendar_same_database`.

This slice's backup is the two logical dumps plus the checkpoint `COPY` files named in `live/manifest.json`. Git, object storage, secret storage, audit anchors, and the other families above are outside the dump. No production operation, push, release, prune, or customer call was performed.

## Build

Pre-edit smoke and the release build used `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `cargo-update-invoked.txt` is `no`. `cargo test --locked --offline` reported 11 passed. `cargo clippy --locked --offline --all-targets -- -D warnings` exited 0. The crate is std-only (`Cargo.lock` version 4, empty dependency list). `unsafe_code` is forbidden.

## Packet contents

`live/manifest.json` and `live/manifest.sha256` are the sealed manifest. Sidecar digest `c2a1ab86a407d6514e6fe2e5ea748cf4d44592fc87491caad40ded08206ebe50`.

Dumps are included because both are small and the manifest names them:

| File | Bytes | SHA-256 |
| --- | --- | --- |
| `live/dumps/oo_a356_agent.dump` | 1151875 | `a1bb32e145d2676cff89b6a44e99b5aafd5e0f0f47a01a2cc07276491d1d1acc` |
| `live/dumps/oo_a356_control.dump` | 50052 | `bcd40b40b5e9b68a3703083757509b4566b9a6df4d0146b1a6c2e52d8e6fcd5e` |

Checkpoint `COPY` files are in `live/gold/`. Shop `COPY` files stayed on the database volume: `shop-party.copy` 71528 bytes, `shop-account.copy` 156574 bytes, `shop-entry.copy` 2781472 bytes. Their SHA-256 values are the `shop-*` entries in the manifest. The volume is removed by owned cleanup after this report.

Password scan of the evidence tree matched no file. Private-key scan matched no file. `pg/password-mode.txt` records mode `600` for `/var/lib/postgresql/a356-admin.pass`. The password file and the TLS key were not copied here.

`SHA256SUMS` is written by this crate's `hash-tree` command after `cleanup-done.txt` exists.
