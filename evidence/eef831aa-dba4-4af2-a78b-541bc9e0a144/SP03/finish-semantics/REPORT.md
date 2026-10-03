# Report — a144 finish semantics

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a144`. Slot B. Slot A was not used. This is the finish path on one PostgreSQL reader owner. It is not whole-reader acceptance and it does not sign R4.

Fixed source `6b95d6f2fd2259edfcbffc27a6338518a64dd0fc`, tree `baaabaf1384646237e0c40bfa1914ea150d39c5b`. Owned source commit `f5f0a73b0678e8b1611f1dc4a40e1dbf1d67b10e`, tree `6ec4bf24dd0111089b87b9b805e2ee71837cd34a`, parent `63161574edf932ea00e25b8a17f69b095c70051f`. Workspace HEAD at the smoke archive was `e94d70a4083b021f88e936d928c90896c32fbbbc`. It moved to `63161574edf932ea00e25b8a17f69b095c70051f` before probe-2. The three product blobs did not move:

- `crates/oo-connectors/src/snapshot.rs` `f1a71fa73064dfb3b83d7d0183f43c6e1a9751a7`
- `crates/oo-connectors/src/session.rs` `7e191eacbf61439680cbd92a7109ded6c8a05596`
- `crates/oo-connectors/src/stream.rs` `ba9b215a059440a4681578200e7d719cf22407e2`

`snapshot.rs`, `session.rs`, and `stream.rs` were not edited. The source commit adds `crates/oo-connectors/tests/pg_reader_finish_live.rs` (blob `06491036db04b58855b9e96c008d7e5e90bcbf74`) and one paragraph in `contracts/source/pgsql/reader-lifecycle-v1.md`. The live overlay still hashed the pre-paragraph lifecycle blob `0f08c66c16333f68ac30719fc4b631b0baf8c2b9`.

## Toolchain and images

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Containers set `RUSTUP_TOOLCHAIN=1.94.0` and refuse any other compiler or host. `HOME=/tmp`. Postgres image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `pg/init.stdout`: `init_ok version=17.10 (Debian 17.10-1.pgdg12+1)`. Container `oo-a144-pg` id `13b9642db3b0075c6e4118b73a837cf6fc8580f1b39e134199d2ff57f09beb24`, privileged false, network `oo-a144-eef831aa-net`, memory 536870912. Private volumes `oo-a144-eef831aa-cargo`, `oo-a144-eef831aa-target`, `oo-a144-eef831aa-pgdata`, `oo-a144-eef831aa-pglog`. No host network, no docker socket, no host home mount, no shared cargo cache. Passwords and the TLS key stayed under `/tmp/oo-a144-eef831aa-dba4-4af2-a78b-541bc9e0a144/secrets`. The probe window was checked: those three password strings are absent.

Smoke hostconfig: `Privileged=false`, `NetworkMode=oo-a144-eef831aa-net`, `Memory=8589934592`, `CapDrop=[ALL]`, `OOMKilled=false`, `ExitCode=0`. Probe-2 hostconfig matches, `ExitCode=0`. Probe-1 hostconfig matches, `ExitCode=101`.

## What ran

Smoke, before any product edit: `cargo test --locked --offline -p oo-connectors --lib` → `63 passed; 0 failed` in 0.00s (`linux/smoke/lib.exit` 0). `cargo clippy --locked --offline -p oo-connectors --all-targets -- -D warnings` exit 0. Vendored `sqlx-postgres` printed two unfulfilled `dead_code` expectations. They did not fail the crate lint.

Probe-1 compiled the first test draft on the fixed archive and failed before a live assertion. `linux/probe-1/pg_reader_finish_live.exit` is 101. `linux/probe-1/docker-run.exit` is 101. Test blob `b22ae52fc75b036cd0d8008be31fb3207dbe228d`. `StreamItem` has no `Debug`, so `expect_err` did not compile. Console and that blob stay as the failed attempt.

Probe-2 is the same archive plus the corrected test only. `linux/probe-2/pg_reader_finish_live.exit` is 0. `test result: ok. 1 passed` in 1.26s. `archive_hashes_match=1`.

`pg_reader_drop_live` and `pg_reader_cancel_live` were not rerun. The three product sources are the fixed blobs, so those tests are not affected by an edit.

## Live result

`linux/probe-2/finish-outcome.txt`, schema `r3f`, source `finish_src`, admin session `oo-fixture-admin`.

Drained `r3f.rows` ids `1,2,3` returned `SnapshotFinished` for generation 1, data pid 150, `backend_start` `2026-10-03 23:27:18.349133+00`. The first current `pg_stat_activity` sample after `finish` returned, taken with `BEGIN`, `pg_stat_clear_snapshot()`, and `ROLLBACK` on the admin session, showed those backends already absent at 1 ms. A second generation read the same ids and finished. Postgres logged `ROLLBACK` on pid 150 and pid 151, then `could not receive data from client: Connection reset by peer`.

`RunCancel::request` then `finish`, with no stream, returned `protocol` fact `cancelled`. `snapshot_finished=0`. Pids 154 and 155 logged `ROLLBACK` and then the same reset. Absent at 0 ms.

`r3f.wide` note `abcdefghij` with `StreamBudget::try_new(1, 1_048_576, 4, 1024)` made `next_batch` return `budget` fact `max_value`. `finish` returned the same fact. `snapshot_finished=0`. Data pid 156, `backend_start` `2026-10-03 23:27:19.222045+00`, logged `START TRANSACTION`, `SAVEPOINT oo_stream_lock`, `SELECT id,note FROM r3f.wide`, `Connection reset by peer`, and `unexpected EOF on client connection with an open transaction`. `reader_rollback_lines=0` and `reader_reset_lines=1`. Guard pid 157 logged `ROLLBACK` and then the reset. Absent at 0 ms. The socket drop without `ROLLBACK` did not become `SnapshotFinished`.

Reader SQL from the log mark was nonempty: 161 statements, verbs `RELEASE=2,ROLLBACK=9,SAVEPOINT=3,SELECT=139,START=8`. Zero `INSERT`, `UPDATE`, `DELETE`, `TRUNCATE`, `MERGE`, `COPY`, `CREATE`, `ALTER`, `DROP`, `GRANT`, `REVOKE`, `COMMIT`, `OFFSET`, or `pg_terminate_backend` on `app=oo-pg-source`. Fixture DDL stayed on the admin session.

## Why the sources stayed

a143 left unsigned the composition where a clear cancel flag, `settle_after` swallowing `portal_poison` into `Ok` plus `mark_skip_rollback`, and `shutdown` returning `Ok` on `skip_rollback`, could make `release_both` return `Ok` and `finish` return `SnapshotFinished`. On the reachable `max_value` path the pull returns the budget error, `prefer` keeps it, and `finish` returns it at `snapshot.rs` 599–601 after `release_both`. `FINDINGS.md` records why a scan success paired with `skip_rollback` is not reachable from the current arms, so `snapshot.rs`, `session.rs`, and `stream.rs` were left as the fixed blobs.

Independent review of `f5f0a73b0678e8b1611f1dc4a40e1dbf1d67b10e` is next. R4 stays unsigned. The ledger was not edited.
