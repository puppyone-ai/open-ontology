# Handoff

Owner of this directory: session `eef831aa-dba4-4af2-a78b-541bc9e0a361`. Do not treat it as parent acceptance or as a closed W04 gate.

## Result to carry

Independent oracle `live/independent/verdict.txt`: `failed=0`, `ok=true`. Fresh restore of cut `cut-a356-1` on one private PostgreSQL 17.10. RPO 1 second, RTO 1 second, both measurements. `pitr_ready` false. `offset_published` `100000` with `offset_min_used` false. Refused restores left `replay_ready` false. The same-database enum apply fails at `CREATE SCHEMA oo_dict` after the calendar migration. Details are in `FINDINGS.md` and `REPORT.md`.

## Raw exits

- `linux/pre-edit-smoke/inside.exit` = 0
- `linux/build/phases/test.exit` = 0 (11 passed)
- `linux/build/phases/clippy.exit` = 0
- `linux/build/phases/oracle-test.exit` = 0
- `linux/build/phases/oracle-clippy.exit` = 0
- `linux/build/docker-run.exit` = 0
- `linux/build-attempt-1/phases/oracle-clippy.exit` = 101
- `linux/run-all.exit` = 3 (first `\copy`, missing directory)
- `linux/resume-tail.exit` = 0
- `live/phases/oracle.exit` = 0
- `live/run-all.exit` = 0
- `pg/up.exit` = 0
- `pg/loader-usage.exit` = 2 (usage, no operator)

## Cleanup

`linux/cleanup.sh` removes containers, volumes, and the network only when both labels match `oo.owner=a361` and `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a361`. It also removes `/tmp/oo-a361-eef831aa-dba4-4af2-a78b-541bc9e0a361`. It does not prune and it does not remove other owners. `cleanup-done.txt` is the completion record. `/tmp/oo-a361-hash-bin` is removed after `SHA256SUMS` is written.

`SHA256SUMS` is produced after `cleanup-done.txt` exists, by `/tmp/oo-a361-hash-bin/pg-backup-restore hash-tree` inside the pinned Rust image (`linux/hash-tree.sh`). Each pass writes `capacity.txt` before the walk and copies `console.log`, `docker-run.exit`, `hostconfig.txt`, and `container-removed.txt` in after the container exits, so the walk does not hash a growing log. The second pass therefore includes the finished first-pass logs. The second pass's own console, exit, hostconfig, and removal record are copied in after that walk. The sidecar `SHA256SUMS.sha256` is the hash of the sums body. Host `shasum` and Python are not the authority for this tree.

## Left open for the parent

`git_mirror`, `object_store`, `secret_store`, `audit_anchors`, `valkey_buffer`, `jobs_control`, `gateway_tokens`, `file_hosting`, `ontology_publication`, `agent_pipeline_schema`, `entity_materialized_tables`, `cluster_roles`, `wal_archive`, `pgbackrest`, `enum_calendar_same_database`.

No production call, no customer call, no foreign database, no push, no release.
