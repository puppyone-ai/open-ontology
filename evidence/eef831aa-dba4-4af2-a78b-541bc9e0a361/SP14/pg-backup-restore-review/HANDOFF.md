# Handoff

Owner of this directory: session `eef831aa-dba4-4af2-a78b-541bc9e0a361`. Verdict on original frozen E51 `1d88fe8ad9fa4a113c105e1df4d0903e517fc9a9` is `WITHHOLD`. Do not treat the 101 oracle passes as a scope waiver, as parent acceptance, or as a closed W04 gate.

## Result to carry

`history_insufficient=false` on this backup is the marker and `shop.entry` count sampled around two sequential dumps. It does not prove one quiescent cut when a value update leaves those markers unchanged. `quiescent=true` is only the opposite flag. `replay_ready` follows that flag. The mandatory correction is in `FINDINGS.md`: keep history insufficient, keep the offset withheld, and keep `replay_ready` false until both databases share one recorded cut (writers blocked on both, or one coordinated snapshot with relation digests bound to the dumps). The product tree was not edited.

`live/independent/verdict.txt` remains `failed=0`, `ok=true` from the earlier run. Those 101 checks were not rerun. RPO 1 second and RTO 1 second remain measurements. `pitr_ready` is false. The happy report published offset `100000` with `offset_min_used` false on that quiet run; that publication is part of the withheld seal, not a certificate. Refused restores left `replay_ready` false.

The `oo_dict` collision (enum migration line 252 after calendar on one database) is handed to author session a365. This packet does not patch it.

## Owned cleanup already collected

Before these verdict edits, `linux/cleanup.sh` found no container, volume, or network with `oo.owner=a361` and this session. `/tmp/oo-a361-commit-msg.txt` was removed. `cleanup-done.txt` is `done`. `linux/cleanup-collected.txt` records that collection. The hash builder used after this edit is removed before the final sums, and its cargo directory is deleted with the hash binary. No second database was started.

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

`SHA256SUMS` is produced after `cleanup-done.txt` exists, by `/tmp/oo-a361-hash-bin/pg-backup-restore hash-tree` inside the pinned Rust image (`linux/hash-tree.sh`). The binary for the verdict revision is a `cargo build --release --locked --offline --bin pg-backup-restore` only (`linux/build-hash-bin.sh`). That build does not run the 11 product tests or the 101 oracle checks. `linux/hash-pass-1` and `linux/hash-pass-2` are the earlier sums. `linux/hash-pass-3` then `linux/hash-pass-4` rehash this verdict. Each pass writes `capacity.txt` before the walk and copies `console.log`, `docker-run.exit`, `hostconfig.txt`, and `container-removed.txt` in after the container exits. Pass 4 therefore includes the finished pass 3 logs. Pass 4's own console, exit, hostconfig, and removal record are copied in after that walk. The sidecar `SHA256SUMS.sha256` is the hash of the sums body. Host `shasum` and Python are not the authority for this tree.

## Left open for the parent

`git_mirror`, `object_store`, `secret_store`, `audit_anchors`, `valkey_buffer`, `jobs_control`, `gateway_tokens`, `file_hosting`, `ontology_publication`, `agent_pipeline_schema`, `entity_materialized_tables`, `cluster_roles`, `wal_archive`, `pgbackrest`, `enum_calendar_same_database`.

No production call, no customer call, no foreign database, no push, no release.
