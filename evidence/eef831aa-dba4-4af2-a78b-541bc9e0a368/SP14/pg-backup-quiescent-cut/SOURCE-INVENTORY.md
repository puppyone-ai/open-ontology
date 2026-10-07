# Source inventory

Product base `1d88fe8ad9fa4a113c105e1df4d0903e517fc9a9`. Owned trees only.

## Recipe origins

| Origin | Path | SHA-256 |
| --- | --- | --- |
| a274 pg-wrap | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a274/SP11/pg-onboarding-review/pg/pg-wrap.sh` | `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb` |
| a274 up | same review, `pg/up.sh` | `ec1dd1087fddfebfe22c89ee21677ed6c81eba75f7ba4e6a55b2d867daa02c76` |
| a274 focused launch | same review, `linux/focused/launch.sh` | `e9fd0027356f5fc90ebe56c0070f1078ba9f0f8d6468fbbecd4db98b4ff16910` |
| a274 sha256 tool | same review, `tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |

Copied `pg/pg-wrap.sh` matches the a274 pg-wrap hash. `up.sh` keeps that server entry and uses a368 resource names. The checker compression follows the a274 schedule. Its file is not byte-identical because the `Fail` type is not linked.

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Both local `linux/arm64`. Neither was pulled.

## Product files

```
tools/pg-backup-restore/.gitignore
tools/pg-backup-restore/CONTRACT.md
tools/pg-backup-restore/Cargo.lock
tools/pg-backup-restore/Cargo.toml
tools/pg-backup-restore/src/decide.rs
tools/pg-backup-restore/src/fixture.rs
tools/pg-backup-restore/src/gold.rs
tools/pg-backup-restore/src/json.rs
tools/pg-backup-restore/src/lib.rs
tools/pg-backup-restore/src/main.rs
tools/pg-backup-restore/src/manifest.rs
tools/pg-backup-restore/src/ops.rs
tools/pg-backup-restore/src/pg.rs
tools/pg-backup-restore/src/protocol.rs
tools/pg-backup-restore/src/sha256.rs
tools/pg-backup-restore/tests/oracle.rs
tools/pg-backup-restore/src/cut.rs
```

## Evidence files before the hash tree

```
CONTRACT-FREEZE.md
FINDINGS.md
HANDOFF.md
REPORT.md
SOURCE-INVENTORY.md
cleanup-done.txt
cleanup-targets.txt
failed-attempts/attempt-1.txt
failed-attempts/attempt-2.txt
failed-attempts/attempt-3.txt
failed-attempts/attempt-4.txt
failed-attempts/attempt-5.txt
failed-attempts/check-clippy-3.log
failed-attempts/check-test-1.log
failed-attempts/check-test-2.log
linux/build/build.log
linux/build/builders-before.txt
linux/build/capacity.txt
linux/build/cargo-update-invoked.txt
linux/build/cargo-version.txt
linux/build/check-build.log
linux/build/check-clippy.log
linux/build/check-test.log
linux/build/clippy-component.log
linux/build/clippy.log
linux/build/console.log
linux/build/container-removed.txt
linux/build/docker-run.exit
linux/build/fixed-commit.txt
linux/build/host-cargo.txt
linux/build/hostconfig.txt
linux/build/image.txt
linux/build/inside.exit
linux/build/invocation.txt
linux/build/mounts.txt
linux/build/oracle-101-rerun.txt
linux/build/phases/build.exit
linux/build/phases/check-build.exit
linux/build/phases/check-clippy.exit
linux/build/phases/check-test.exit
linux/build/phases/clippy-component.exit
linux/build/phases/clippy.exit
linux/build/phases/test.exit
linux/build/rustc-verbose.txt
linux/build/rustc-version.txt
linux/build/test.log
linux/build/volume-cargo.txt
linux/build/volume-target.txt
linux/cleanup.sh
linux/inside.sh
linux/live-phases.sh
linux/pre-edit-smoke/Cargo.lock.copy
linux/pre-edit-smoke/builders-before.txt
linux/pre-edit-smoke/capacity.txt
linux/pre-edit-smoke/cargo-update-invoked.txt
linux/pre-edit-smoke/cargo-version.txt
linux/pre-edit-smoke/components.txt
linux/pre-edit-smoke/console.log
linux/pre-edit-smoke/container-removed.txt
linux/pre-edit-smoke/docker-run.exit
linux/pre-edit-smoke/fixed-commit.txt
linux/pre-edit-smoke/host-cargo.txt
linux/pre-edit-smoke/hostconfig.txt
linux/pre-edit-smoke/image.txt
linux/pre-edit-smoke/inside.exit
linux/pre-edit-smoke/invocation.txt
linux/pre-edit-smoke/lock-shape.txt
linux/pre-edit-smoke/mounts.txt
linux/pre-edit-smoke/rustc-verbose.txt
linux/pre-edit-smoke/rustc-version.txt
linux/private-glibc.sh
linux/psql-exec.sh
linux/run-build.sh
linux/run-live.sh
linux/run-smoke.sh
linux/smoke-inside.sh
live/corrupt-dump-readiness.json
live/databases.txt
live/manifest.json
live/manifest.sha256
live/password-leak-files.txt
live/phases/backup.err
live/phases/backup.exit
live/phases/backup.out
live/phases/backup.pid
live/phases/cut-check.err
live/phases/cut-check.exit
live/phases/cut-check.out
live/phases/existing-amount.txt
live/phases/load-fixture.err
live/phases/load-fixture.exit
live/phases/load-fixture.out
live/phases/locks-seen.txt
live/phases/mark-post-cut.err
live/phases/mark-post-cut.exit
live/phases/mark-post-cut.out
live/phases/restore-corrupt-dump.err
live/phases/restore-corrupt-dump.exit
live/phases/restore-corrupt-dump.out
live/phases/restore-existing.err
live/phases/restore-existing.exit
live/phases/restore-existing.out
live/phases/restore-post.txt
live/phases/restore.err
live/phases/restore.exit
live/phases/restore.out
live/phases/source-post.txt
live/private-key-files.txt
live/probes/amount-during-lock.txt
live/probes/committed.exit
live/probes/count-after.txt
live/probes/count-before.txt
live/probes/cut-sessions-after.txt
live/probes/dbs-after-corrupt.txt
live/probes/dbs-before-corrupt.txt
live/probes/locks-seen.txt
live/probes/marker-after.txt
live/probes/marker-before.txt
live/probes/rejected.err
live/probes/rejected.exit
live/probes/restore-amount.txt
live/probes/source-amount.txt
live/raw-exits.txt
live/restore-existing-readiness.json
live/restore-readiness.json
live/run-live.exit
live/timing.txt
oracle/Cargo.lock
oracle/Cargo.toml
oracle/src/check.rs
oracle/src/main.rs
oracle/src/sha256.rs
origin/recipes.txt
pg/capacity.txt
pg/caps.json
pg/container.id
pg/glibc-capacity.txt
pg/image.txt
pg/inspect.txt
pg/loader-usage.err
pg/loader-usage.exit
pg/loader-usage.out
pg/password-mode.txt
pg/pg-dump-version.txt
pg/pg-restore-version.txt
pg/pg-wrap.sh
pg/ports.json
pg/private-glibc-ldd.txt
pg/psql-version.txt
pg/secrets-path.txt
pg/server-san.txt
pg/server.txt
pg/tool-dir.txt
pg/up.exit
pg/up.sh
pg/volume-data.txt
```

`SHA256SUMS` is the product `hash-tree` output for this directory after cleanup. The command skips that file and its sidecar.
