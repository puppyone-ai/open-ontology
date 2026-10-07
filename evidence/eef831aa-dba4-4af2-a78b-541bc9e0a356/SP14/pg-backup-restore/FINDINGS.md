# Findings

## No second backup tool

No tool under `tools/` already performed this PostgreSQL backup and fresh-database restore. `local-pg-mirror` is a read-only snapshot copy. It was not wrapped and not edited.

## Failed attempts

1. `linux/build-attempt-1/` docker exit 101. `cargo test` compiled the library, then `tests/oracle.rs` failed because `Secret` did not implement `Debug`. An unused `File` import was also warned. Fix: redacting `Debug`, drop the unused import. Protocol flags were not changed. Logs kept.

2. `linux/build-attempt-2/` tests passed (11). Clippy `-D warnings` failed on `result_unit_err`, `needless_borrow`, `collapsible_if`, `type_complexity`, `ptr_arg`, `manual_slice_fill`, and `needless_range_loop`. Fixes stayed inside those lints. The reviewed SHA-256 compression schedule was not rewritten. One `needless_range_loop` allow remains on that function. Logs kept.

3. `live/attempt-1-glibc/` phase `load-fixture` exit 1, before any SQL:

```
/var/lib/postgresql/a356-work/pg-backup-restore: /lib/aarch64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found (required by /var/lib/postgresql/a356-work/pg-backup-restore)
```

The release binary is the pinned Rust 1.94 gnu build (Debian glibc 2.41). The pinned PostgreSQL 17.10 image linker rejected it. `pg_dump` was not started. Dump and restore flags were not changed.

Attempt 2 started the same binary with a private copy of `ld-linux-aarch64.so.1`, `libc.so.6`, and `libgcc_s.so.1` from the Rust image, placed at `/var/lib/postgresql/a356-glibc`. `pg/loader-usage.exit` is 2 (usage, no operator). Child processes still call `env_clear`, so `pg_dump`, `pg_restore`, and `psql` remain `/usr/bin` binaries from the PostgreSQL image. `pg/glibc.SHA256SUMS` is the hash of those three loader files. The copy container was `--network none`, `--cap-drop ALL`, 256 MiB, label `oo.kind=glibc-src`, and it was removed. It was not a Rust builder.

## Live proof

`linux/continue-live.sh` ran on the already-up database `oo-a356-pg`. It did not start a second PostgreSQL and did not start a Rust builder. Phase exits are under `live/phases/`. `live/continue-live.exit` and `live/run-all.exit` are 0.

The shop `numeric(18,4)` text from PostgreSQL matched the Rust formatter. No formatter change and no second build were required.

Refused restores write `replay_ready=false` and a null RPO/RTO (`live/restore-missing-readiness.json` and the two corrupt reports). The happy restore then created the fresh databases and reported `replay_ready=true`. A restore database that already had user tables would have been refused as `restore_database_not_fresh`, so the earlier refusals did not leave a restored catalog behind.

`PGOPTIONS` uses `statement_timeout=180000`, `DateStyle=ISO,YMD`, `TimeZone=UTC`. The live dump finished inside that statement timeout.

The post-cut rows were inserted only after the manifest seal. Source still has them. The restore report says they are absent from the restore databases. Issued keys in the sealed gold are `k-0001` / `member-a` / generation 1 and `k-0002` / `member-b` / generation 2. `key_reused` is false. `offset_min_used` is false. The published offset is the stored control position `100000`, and the decoy position is not combined with it.

Password bytes were read from a mode-0600 file on the data volume. They were not placed on the orchestrator argv. A scan of this evidence tree for the password file returned no path. A scan for a PEM header returned no path.

## Limits

The ready bit covers this fixture's logical restore on this private instance. It does not cover the remaining families listed in `REPORT.md`. Cluster-wide roles created for the fixture are not part of `pg_dump` of the two databases (`cluster_roles` remains open). Jobs control SQL was not installed. The caller is the local operator file, not a public authentication check.
