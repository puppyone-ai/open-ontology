# Handoff

Owned paths only:

- `tools/pg-backup-restore/**`
- `testing/real-db/pg-backup-restore/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a356/SP14/pg-backup-restore/**`

`development_unit` is false. `parent_acceptance` is false. Do not mark SP14-W04, AT-OPS-06, or a parent goal accepted from this packet.

## What the next reader can rely on

A private PostgreSQL 17.10 logical backup of `oo_a356_control` and `oo_a356_agent` restored into `oo_a356_restore_control` and `oo_a356_restore_agent` on that same instance. The ready report is `live/restore-readiness.json`. The sealed manifest is `live/manifest.json`. Missing and corrupt inputs exited 4 with `replay_ready=false`.

Measured RPO is 0 seconds and measured RTO is 1 second for this run. `production_slo_certified` is false. `pitr_ready` is false. `w04_gate` is `open`. `all_state_certified` is false.

## What is still required before W04 can close

The report lists these remaining families: `git_mirror`, `object_store`, `secret_store`, `audit_anchors`, `valkey_buffer`, `jobs_control`, `gateway_tokens`, `file_hosting`, `ontology_publication`, `agent_pipeline_schema`, `entity_materialized_tables`, `cluster_roles`, `wal_archive`, `pgbackrest`, `enum_calendar_same_database`.

Enum and calendar were proved in separate databases because both migrations create `oo_dict`. That split is a fixture placement, not a product decision that enum belongs in the agent database.

## How the binary was run

The Rust 1.94 binary does not start under the PostgreSQL image libc. `linux/private-glibc.sh` copies three loader files from the pinned Rust image and execs `ld-linux-aarch64.so.1 --library-path` that directory. PostgreSQL client tools stay on the image. A rerun of `linux/run-all.sh` refuses when `linux/build/inside.exit` already exists, and `pg/up.sh` refuses when an owned PostgreSQL container is already running. Preserve those logs. Do not start a second builder or a second database.

## Cleanup

`linux/cleanup.sh` removes containers, volumes, and the network only when both labels match `oo.owner=a356` and `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a356`. It also removes `/tmp/oo-a356-eef831aa-dba4-4af2-a78b-541bc9e0a356`. It does not prune. `cleanup-done.txt` in this directory is the completion record. `SHA256SUMS` is produced after that file exists, by `pg-backup-restore hash-tree` on linux/arm64, not by the host `shasum` and not by Python.

No push, release, image publish, or customer call belongs to this handoff.
