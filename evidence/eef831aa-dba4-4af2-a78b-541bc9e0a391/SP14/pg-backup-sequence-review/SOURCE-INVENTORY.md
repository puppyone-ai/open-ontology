# Source inventory

Product archive `95c33fca0b22b38a2e08c08bb5e2a050f0bf44ec`, subject `pg-backup-restore: fence sequences with OWNED BY and restore the cut`. Worktree `tools/pg-backup-restore` matches that commit. `gold.rs` blob `2564210b226be3d6980265a1dc429cfadf2a613f`. No root manifest change. No cargo update. No generate.

Read for this review:

| Path | Use |
| --- | --- |
| `tools/pg-backup-restore/CONTRACT.md` | Cut rule, WAL flag, unproven restore |
| `tools/pg-backup-restore/src/cut.rs` | Fence held rule, operator check, user sequences |
| `tools/pg-backup-restore/src/snapshot.rs` | `OWNED BY` DO block, SQLSTATE parse, catalog kinds |
| `tools/pg-backup-restore/src/ops.rs` | Backup order, refuse before `CREATE DATABASE`, hash tree |
| `tools/pg-backup-restore/src/pg.rs` | `pg_dump` arguments and `PGAPPNAME` |
| `tools/pg-backup-restore/src/fixture.rs` | Frozen sequences and roles |
| `tools/pg-backup-restore/src/protocol.rs` | Operator keys, session `a356` |
| `tools/pg-backup-restore/Cargo.toml` | Rust 1.94, empty dependency list |
| `tools/pg-backup-restore/Cargo.lock` | `package_names=1`, `source_lines=0` |
| PostgreSQL 17.10 `src/backend/commands/sequence.c` | `ShareRowExclusiveLock`, `need_seq_rewrite`, identity `0A000`, cache lock |

The PostgreSQL mechanism notes in `mechanism/results.txt` were checked against the running 17.10 server, not by patching the server.

Recipes, byte or recorded hash:

| Recipe | SHA-256 |
| --- | --- |
| a274 `pg-wrap.sh`, copied here | `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb` |
| a274 `up.sh` origin, this `up.sh` is the UUID-private adaptation | `ec1dd1087fddfebfe22c89ee21677ed6c81eba75f7ba4e6a55b2d867daa02c76` |
| a274 focused `launch.sh` | `e9fd0027356f5fc90ebe56c0070f1078ba9f0f8d6468fbbecd4db98b4ff16910` |
| a274 `sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |

Images, local only, no pull:

| Image | Digest |
| --- | --- |
| `docker.io/library/rust` | `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` |
| `docker.io/library/postgres` | `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` |

Evidence owned by this session is this directory. `runtime/` is gitignored. Other sessions' containers were left running.
