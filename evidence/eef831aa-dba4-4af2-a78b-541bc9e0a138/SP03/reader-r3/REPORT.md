# REPORT — a138 reader R3

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a138`. Fixed input `dd5bf44160f949c0ac21a0ce3ddf5ee5e90b4200`. Two source commits on `master`:

- `8bda0cd48976b23d268d125b34bc8130d94b8253` keys planning and chunk reads.
- `58afa97ded85a7dd89c72532bfc8973c45255d7c` parent `8bda0cd48976b23d268d125b34bc8130d94b8253`, tree `543fd103532a8366ce92a0b4eaea4e089eb76d2d`. Cancel and unpublished-run close.

`error.rs` and `config.rs` are unchanged from `dd5bf441`. No vendor, manifest, lock, ledger, docs, or pipeline edits.

## Images and toolchain

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `--pull never`. Container user `0:0`, `--cap-drop ALL`, no-new-privileges, memory 8 GiB, cpus 4, pids 4096, `HOME=/tmp`. No host network, docker socket, or HOME mount.

PostgreSQL image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `linux/arm64`, server `17.10 (Debian 17.10-1.pgdg12+1)`. Container `oo-a138-pg`, network `oo-a138-eef831aa-net`, no host port, memory 512 MiB, cap-drop ALL plus the image's chown set. `init.exit=0`, `up.exit=0`.

Smoke before product edits: `linux/smoke/lib.exit=0`, `linux/smoke/clippy.exit=0`.

## Final exits

`linux/cancel/` is the last cancel container, `oo-a138-msrv-cancel`, host config `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=oo-a138-eef831aa-net`, `Memory=8589934592`, `CapDrop=[ALL]`.

| phase | exit |
| --- | --- |
| overlay | 0 |
| toolchain | 0 |
| lib (63 tests) | 0 |
| pg_reader_keys_live | 0 |
| pg_reader_cancel_live | 0 |
| clippy component | 0 |
| clippy `-D warnings` | 0 |
| docker run | 0 |

Elapsed inside the container: 37 seconds. Vendor `sqlx-postgres` still warns that two `#[expect(dead_code)]` lints are unfulfilled. Those warnings do not fail the build. Clippy on `oo-connectors` finished with no warnings.

`linux/keys/` is the keys-only run that preceded the cancel commit. `lib.exit=0`, `keys.exit=0`, `clippy.exit=0`, `docker-run.exit=0`. Its `keys-outcome.txt` is the first green keys oracle. The cancel container ran the keys test again after `pull_one` changed. That second keys oracle is `linux/cancel/keys-outcome.txt`.

## Oracles

Keys, from `linux/keys/keys-outcome.txt`: collation `af-NA-x-icu` orders `a,A,ä,b,B,z` and `C` orders `A,B,a,b,z,ä`. Sparse chunk 0 matches the repeatable-read cut. A writer commit during the old cut is invisible to that cut. The fresh generation reads the new cut. The no-key table finishes as generation 3 with no resume cursor. Reader SQL audit: 2290 statements, 5 writer writes, zero DML, DDL, COMMIT, or OFFSET on the reader.

Cancel, from `linux/cancel/cancel-outcome.txt`:

- Cancel before start, generation 1, after the data backend was `idle in transaction`.
- Cancel while the drive was waiting for demand, generation 2.
- Cancel while the scan was blocked on `ACCESS EXCLUSIVE`, fact `cancelled`, generation 3.
- Dropping `next_batch` left the data backend `active`. Cancel then released generation 4.
- After a full drain of ids `1,2,3`, cancel rather than finish, generation 5.
- Deadline `Instant::now()+20s`, statement timeout 60s, lock timeout 120s. Fact `deadline` at 19787 ms. Generation 6.
- `finish` after `RunCancel::request` returns fact `cancelled`.
- Admin `pg_terminate_backend` on the data pid and `backend_start`. Next read fact `database`, SQLSTATE `57P01`. `finish` did not return `SnapshotFinished`.
- Writer inserted id 4. Fresh generation 9 read `[1, 2, 3, 4]`. The old partial batch was `[1]`.
- Reader audit on the whole server log: 4852 `oo-pg-source` statements, 6 `pg_cancel_backend` calls, 11 writer writes, zero reader DML, DDL, COMMIT, or OFFSET. No `pg_terminate_backend` on the reader application.

The second keys run in that same container repeated the collation, sparse cut, late-writer, and no-key checks. Its statement count is higher because the server log was not truncated.

## Failures overwritten in cancel/console.log

`linux/cancel/console.log` is the passing run. Earlier cancel containers wrote the same path and were replaced.

1. Test compile exit 101. `pin!` kept the `next_batch` borrow alive across `owner.cancel()`. `Result::expect_err` required `Debug` on `StreamItem`. `finish` did not need `mut`.
2. Lib compile exit 101. The open `Result` still borrowed the data connection when rollback used it (`E0499`).
3. Lib 0, keys 0, cancel exit 101 at 15.92s. Panic: receiving pull never became active. That loop read `pg_stat_activity` before it polled `next_batch`.
4. Lib, keys, and cancel passed. Clippy exit 101: `clippy::question_mark` on `finish_cancel` in `snapshot.rs`.

Keys failures fixed before `8bda0cd4` are not in the retained `linux/keys/console.log`. They were an ambiguous page-count column `n` (`42702`), a no-key audit that required quoted identifiers, Clippy argument-count and `collapsible_if` findings, and `ALTER ROLE ... SET log_statement` which the reader rejected as `startup_options`. The role setting was reset. The retained keys console is the passing run.

Owned containers, volumes, network, and `/tmp/oo-a138-*` paths were removed after the runs. `cleanup.txt` is that record. `hashes.sha256` covers every other file in this directory.

## Not claimed

`bounded_reader` and `type_support.converted` stay false. Resume cursors, CDC, the registry API, R4, and full SP03-W03 are unsigned. No ledger checkboxes were edited.
