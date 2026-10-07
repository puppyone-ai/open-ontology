# Source inventory

Session `eef831aa-dba4-4af2-a78b-541bc9e0a365`. Pre-commit HEAD is `6283bc16f6d408ffa38dbbf8837cc4f80268bbcb`. Write scope is `tools/calendar-store/**`, `tools/enum-store/**`, and this evidence directory. Other product paths stayed readonly.

## Product files in the commit

Edited:

- `tools/calendar-store/CONTRACT.md`
- `tools/calendar-store/src/lib.rs`
- `tools/calendar-store/src/sql.rs`
- `tools/calendar-store/src/store.rs`
- `tools/calendar-store/tests/contract.rs`
- `tools/enum-store/CONTRACT.md`
- `tools/enum-store/src/lib.rs`
- `tools/enum-store/src/sql.rs`
- `tools/enum-store/src/store.rs`
- `tools/enum-store/tests/contract.rs`

Added:

- `tools/calendar-store/src/adopt.rs`
- `tools/calendar-store/tests/coexist.rs`
- `tools/enum-store/src/adopt.rs`
- `tools/enum-store/tests/coexist.rs`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a365/SP14/control-migration-coexistence/**`

Unchanged bytes, confirmed by an empty `git diff` before the packet:

- `tools/calendar-store/migrations/oo_calendar_control_v1.sql`
- `tools/enum-store/migrations/oo_enum_control_v1.sql`
- `tools/calendar-store/Cargo.toml`
- `tools/enum-store/Cargo.toml`
- `tools/calendar-store/Cargo.lock`
- `tools/enum-store/Cargo.lock`
- root `Cargo.toml` and root `Cargo.lock`

`tools/enum-store/src/sha256.rs` was read and copied into `linux/hash-oracle/main.rs`. The product file was not edited.

## Proof inputs that match the working tree

Compared equal to `/tmp/oo-a365-eef831aa-dba4-4af2-a78b-541bc9e0a365/archive` after `proof_ok`: both crates' `src/sql.rs`, `src/adopt.rs`, `src/store.rs`, `src/lib.rs`, `CONTRACT.md`, `tests/coexist.rs`, `tests/contract.rs`, the v1 migration, and `Cargo.lock`.

## Recipe origins cited by the scripts

Citations below are the header lines. Host `shasum -a 256` of the origin file matched the cited digest for the five files marked match. The a289 `launch.sh` file itself is not that digest; its header cites the a275 file.

| Origin | Cited digest | Check |
|---|---|---|
| `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a361/SP14/pg-backup-restore-review/linux/run-smoke.sh` | `568da75c0d77d14dc1f6c76332b357e529feea411047a36f2b846103e9f06b54` | match |
| same directory `smoke-inside.sh` | `3b5e988f96687c888ab848bffeb9e0eb6b13284dda24afc9dfc00d0b4ca5bf34` | match |
| `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a289/SP08/calendar-store/pg/up.sh` | `6eb5d3bc6106d52d7b41b0a1ae186d2e81063274868864e919b823b112e4229a` | match |
| a361 `linux/hash-tree.sh` | `c96add55ffa408b52112553345a698d36f80b6f60496a70aa45ff07d25ee2dc5` | match |
| a361 `linux/cleanup.sh` | `1af543860e0413bbf6746aad10c766681e6e8c87f3483ef10eab9d4aae901788` | match |
| a289 `linux/impl-1/launch.sh` cites a275 `SP11/session-store/linux/impl-7/launch.sh` | `a5ded8116a77af27599a2b103fdad8e7a5ae485843045b3365b5da333741a836` | citation only |
| a289 `pg/pg-wrap.sh` cites a275 `SP11/session-store/pg/pg-wrap.sh` | `d803bb0b0e284ea50bf1a21a9de5e141178027f06c19989f2f5c13fe004c2d00` | citation only |

Images, already local: Rust `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, PostgreSQL `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`.

## Readonly reviews

a361 `REPORT.md`, `FINDINGS.md`, and `HANDOFF.md` under `SP14/pg-backup-restore-review/`. That review withholds E51 and assigns the `oo_dict` collision to this session. This packet does not edit that directory.
