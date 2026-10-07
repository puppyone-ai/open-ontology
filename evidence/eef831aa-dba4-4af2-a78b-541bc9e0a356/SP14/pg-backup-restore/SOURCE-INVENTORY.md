# Source inventory

Session `eef831aa-dba4-4af2-a78b-541bc9e0a356`. Fixed archive `7a6f7eac8b8041eae215c087d1a80930437871ce`.

## Orchestrator

Std-only crate. Edition 2024. `rust-version` 1.94.0. `Cargo.lock` has no third-party packages. Root workspace manifests were not edited.

- `tools/pg-backup-restore/CONTRACT.md`
- `tools/pg-backup-restore/Cargo.toml`
- `tools/pg-backup-restore/Cargo.lock`
- `tools/pg-backup-restore/.gitignore`
- `tools/pg-backup-restore/src/lib.rs`
- `tools/pg-backup-restore/src/main.rs`
- `tools/pg-backup-restore/src/decide.rs`
- `tools/pg-backup-restore/src/fixture.rs`
- `tools/pg-backup-restore/src/gold.rs`
- `tools/pg-backup-restore/src/json.rs`
- `tools/pg-backup-restore/src/manifest.rs`
- `tools/pg-backup-restore/src/ops.rs`
- `tools/pg-backup-restore/src/pg.rs`
- `tools/pg-backup-restore/src/protocol.rs`
- `tools/pg-backup-restore/src/sha256.rs`
- `tools/pg-backup-restore/tests/oracle.rs`

## Harness

- `testing/real-db/pg-backup-restore/CONTRACT.md`
- `testing/real-db/pg-backup-restore/linux/inside.sh`
- `testing/real-db/pg-backup-restore/linux/run-all.sh`
- `testing/real-db/pg-backup-restore/linux/continue-live.sh`
- `testing/real-db/pg-backup-restore/linux/live-phases.sh`
- `testing/real-db/pg-backup-restore/linux/private-glibc.sh`
- `testing/real-db/pg-backup-restore/linux/cleanup.sh`
- `testing/real-db/pg-backup-restore/pg/up.sh`
- `testing/real-db/pg-backup-restore/pg/pg-wrap.sh`

`pg/pg-wrap.sh` is the reviewed a274 file, SHA-256 `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb`.

| Recipe | Origin path under the a274 review | SHA-256 |
| --- | --- | --- |
| pg-wrap | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a274/SP11/pg-onboarding-review/pg/pg-wrap.sh` | `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb` |
| up.sh | same review, `pg/up.sh` | `ec1dd1087fddfebfe22c89ee21677ed6c81eba75f7ba4e6a55b2d867daa02c76` |
| focused launch | same review, `linux/focused/launch.sh` | `e9fd0027356f5fc90ebe56c0070f1078ba9f0f8d6468fbbecd4db98b4ff16910` |
| sha256 tool | same review, `tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |

`up.sh` is adapted (names, 1 GiB, this fixture). The SHA-256 library keeps that compression function. Its file hash differs because the CLI wrapper was not copied.

## Schema files applied

Hashes are SHA-256 of the migration bytes, stored in the manifest and in each `install_manifest` row.

| Id | Database | SHA-256 |
| --- | --- | --- |
| cdc-offset-store-v1 | `oo_a356_control` | `74d333dd7567132ff19f22db2d2ce97af9e91c3a148ee0cac9249ababdb0ec1f` |
| session-store-v1 | `oo_a356_control` | `7b1baab3bbe350765e58e72a802f332e83198517b3dde8503849c604ed6758a2` |
| calendar-store-v1 | `oo_a356_control` | `ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520` |
| enum-store-v1 | `oo_a356_agent` | `b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd` |

Relative paths are under the operator `schema_root`: `tools/cdc-offset-store/migrations/oo_cdc_offset_control_v1.sql`, `tools/session-store/migrations/oo_session_control_v1.sql`, `tools/calendar-store/migrations/oo_calendar_control_v1.sql`, `tools/enum-store/migrations/oo_enum_control_v1.sql`. Those migrations are not owned by this slice and were not edited.

## Images

| Use | Reference |
| --- | --- |
| Rust builder and private loader | `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` |
| PostgreSQL 17.10 | `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` |

Both inspect records say `os=linux` and `arch=arm64`.

## Evidence kept with this inventory

`linux/pre-edit-smoke/`, `linux/build-attempt-1/`, `linux/build-attempt-2/`, `linux/build/`, `pg/`, `live/` including `attempt-1-glibc/`, dumps, checkpoint gold, readiness JSON, and these four documents. Shop `COPY` bodies are represented by their manifest hashes. The password, TLS private keys, and the release binary are not in this tree.
