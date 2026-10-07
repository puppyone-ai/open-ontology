# E51 / SP14-W04 backup-restore review

Reviewer session `eef831aa-dba4-4af2-a78b-541bc9e0a361`. Fixed source `1d88fe8ad9fa4a113c105e1df4d0903e517fc9a9`. Author packet a356, 166 files, host `shasum -a 256 -c` exit 0 (`origin/a356-packet.txt`).

## Verdict

The private PostgreSQL 17.10 logical backup restored into fresh databases, and the independent oracle matched every restored shop row plus the checkpoint, key, calendar, enum, and history probes. `live/independent/verdict.txt` says `failed=0` and `ok=true` (101 passes).

That slice does not close SP14-W04 or AT-OPS-06. The happy readiness report keeps `w04_gate` at `open`, `all_state_certified` false, `production_slo_certified` false, `production_ops` false, `parent_acceptance` false, `pitr_ready` false, and `offset_min_used` false. Parent acceptance is not claimed.

## What ran

Linux arm64, Rust 1.94.0 `(4a4ef493e 2026-03-02)`, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, server `17.10 (Debian 17.10-1.pgdg12+1)`. Both images were already local. Nothing was pulled.

Pre-edit smoke `linux/pre-edit-smoke/inside.exit` is 0. Lock shape `package_names=1 source_lines=0`. The image components before the build were cargo, rust-std, and rustc. Clippy was added inside the builder. `linux/build/cargo-update-invoked.txt` is `no`. Product tests: `11 passed; 0 failed`. Product clippy and the independent oracle tests and clippy exited 0. Commands were `cargo test --locked --offline` and `cargo clippy --locked --offline --all-targets -- -D warnings`. No root manifest was edited. No dependency was added.

One builder, 6 GiB, 1 CPU, `linux/build/capacity.txt` was `own=0 global=1 host_cargo=0`. One private database, 1 GiB, 1 CPU, no host port. Password file mode `600`. Child `pg_dump`, `pg_restore`, and `psql` stayed `/usr/bin` from the PostgreSQL image. The review binary used a private loader from the Rust image (`pg/loader-usage.exit` is 2). Loader hashes are in `pg/glibc.SHA256SUMS`.

Frozen operator identity inside that container: owner `a356`, session `eef831aa-dba4-4af2-a78b-541bc9e0a356`, user `oo_a356_admin`, databases `oo_a356_control`, `oo_a356_agent`, `oo_a356_restore_control`, `oo_a356_restore_agent`, cut `cut-a356-1`, fence `fence-a356-restore`. Docker labels, the network, and the volumes are a361.

## Live exits

| Phase | Exit |
|---|---|
| load-fixture | 0 |
| backup | 0 |
| mark-post-cut | 0 |
| restore-missing | 4 |
| restore-corrupt-dump | 4 |
| restore-corrupt-manifest | 4 |
| restore | 0 |
| restore-existing | 4 |
| schema calendar on `oo_a361_schema_probe` | 0 |
| schema enum on the same database | 3 |
| independent oracle | 0 |
| `linux/run-all.exit` (first copy, directory missing) | 3 |
| `linux/resume-tail.exit` (oracle tail after the directory fix) | 0 |
| `linux/build-attempt-1` oracle clippy | 101 |

Missing, corrupt, and existing refusals report `replay_ready` false. Reasons: `manifest_absent`, `dump_hash_mismatch`, `manifest_hash_mismatch`, `restore_database_not_fresh`. After the missing and corrupt refusals the restore database list was `none`. After the existing-database refusal, `shop.entry` was still 90000, party `p00001` was still `n00001`, and `p90000` was still absent on the restore.

Happy report: `replay_ready` true, `offset_published` `100000`, `rpo_window_seconds` 1, `rto_seconds` 1, `shop_gold_ok` true. Manifest LSN before and after were both `0/2B41638`, so `wal_moved` is false. `last_write_unix` 1791409515 and `backup_finished_unix` 1791409516, so the manifest RPO is 1 second. Outer backup and restore spans in `live/timing.txt` are also 1 second each.

The independent oracle hashed the restored COPY text itself: party 71528 bytes, account 156574 bytes, entry 2781472 bytes, 2000 / 8000 / 90000 rows. Null amounts 3104. Comment `甲注评名录`. Watermark `100000|txn-a356-1|txn-a356-2|100001`. Post-cut `p90000` and `k-9000` are on the source and absent on the restore. Dump sidecar binds matched (`oo_a356_agent.dump` 1151875 bytes, `oo_a356_control.dump` 50052 bytes).

`docs/11-deployment-and-operations.md` §5 sets the initial control target at 5 minutes / 1 hour and the agent target at 15 minutes / 1 hour, both for pgBackRest. The numbers above are one private logical-dump measurement. They are not a production SLO certificate.

## Same-database schema gap

Calendar SQL and enum SQL each execute `CREATE SCHEMA oo_dict` (calendar migration line 222, enum migration line 252). The fixture puts calendar on `oo_a356_control` and enum on `oo_a356_agent`, so the two creates never meet. On `oo_a361_schema_probe`, calendar applied and enum stopped with:

```
psql:/schemas/tools/enum-store/migrations/oo_enum_control_v1.sql:252: ERROR:  schema "oo_dict" already exists
```

The probe database was dropped. It is absent from `live/databases.txt`. No other schema pair was attempted.

## Still open

The happy report still lists `git_mirror`, `object_store`, `secret_store`, `audit_anchors`, `valkey_buffer`, `jobs_control`, `gateway_tokens`, `file_hosting`, `ontology_publication`, `agent_pipeline_schema`, `entity_materialized_tables`, `cluster_roles`, `wal_archive`, `pgbackrest`, and `enum_calendar_same_database`. `pg_restore -l` shows 0 `ROLE` lines. This dump does not carry cluster roles, WAL archives, or pgBackRest state. A fresh cluster without the fixture roles is unproven.

Owned cleanup is `linux/cleanup.sh`. `SHA256SUMS` is produced after `cleanup-done.txt` by `pg-backup-restore hash-tree` on linux/arm64.
