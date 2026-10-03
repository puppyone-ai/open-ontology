# Report — a143 fixed owner Drop / cancel acceptance

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a143`. Slot B. Slot A (a142 mapping review) was not used.

Verdict: **APPROVE LIMITED** for the R3 cancel and unpublished-run slice. Detail in `FINDINGS.md`. Handoff in `HANDOFF.md`.

## Source and isolation

Reviewed commit `6b95d6f2fd2259edfcbffc27a6338518a64dd0fc`, tree `baaabaf1384646237e0c40bfa1914ea150d39c5b`. Archives used `git archive` from the workspace without checking the commit out. HEAD before and after each archive: `c83184f2f930435e17dd7c87d06b9d8ce7cc1554` (`linux/smoke/workspace-head.txt`, `linux/old-1/workspace-head.txt`, `linux/probe-1/workspace-head.txt`).

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `rust-toolchain.toml` in the archive says `1.98.1`; the containers force `RUSTUP_TOOLCHAIN=1.94.0` and refuse any other compiler. Postgres image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `linux/arm64`. `pg/init.stdout`: `init_ok version=17.10 (Debian 17.10-1.pgdg12+1)`. Container `oo-a143-pg` id `a463efc4da0b37a6d7b198572c3c48cdf0aa69c00b9140db94d23b7aea845181`, privileged false, network `oo-a143-eef831aa-net`, memory 536870912. Private volumes `oo-a143-eef831aa-cargo`, `oo-a143-eef831aa-target`, `oo-a143-eef831aa-pgdata`, `oo-a143-eef831aa-pglog`. `HOME=/tmp` inside the Rust containers. No host network, no docker socket, no host home mount, no shared cargo cache. Passwords stayed in `/tmp/oo-a143-eef831aa-dba4-4af2-a78b-541bc9e0a143/secrets` and were redacted out of the copied postgres windows. `pg/pointer-keys.txt` lists key names only.

One owned Postgres and one Rust builder at a time. Capacity files: smoke and old-1 and probe-1 each record `own_rust_builder_running=0` and `host_cargo_rustc_rustdoc_rustup=0` before start; probe-1 also records `own_postgres_running=1`.

## Exits

All recorded phase exits are 0. No failed console was discarded.

| Run | docker-run | phases |
| --- | --- | --- |
| `linux/smoke` | 0 | toolchain 0, snapshot 0, clippy-component 0, fetch 0, lib 0 (63 passed), clippy 0 |
| `linux/old-1` | 0 | toolchain 0, `pg_reader_drop_live` 0 |
| `linux/probe-1` | 0 | toolchain 0, `pg_reader_drop_live` 0, `pg_reader_cancel_live` 0, `pg_reader_keys_live` 0, slice 0 |

Hostconfig for each Rust container: `Privileged=false`, `NetworkMode=oo-a143-eef831aa-net`, `Memory=8589934592`, `CapDrop=[ALL]`, `OOMKilled=false`, `ExitCode=0`.

Content sha256 of the fixed archive (`linux/probe-1/archive-sha256.txt`, also the smoke expect files):

- `snapshot.rs` `7961fe972051d7ee4163eb3b38bfb0353a8a2a82b3dedb669971c323b7ffb4e8`
- `session.rs` `6933bf4e855682e6998b89c8c756f8db6239e28e4bc9a249fd34af98c088f16f`
- `stream.rs` `fe11aca7f711c97787c7420ce98e64a1fcd9c32fec6e07eb3bd65736f1013847`
- `pg_reader_drop_live.rs` `cc2d397232ce9c26fd05610b5a9d61478783830a58fe56b001e38a99de115ca9`
- `reader-lifecycle-v1.md` `f5a075b273c9c9f845aa9b714c258296b317c0f411baafc8011a130bdf7cde0b`

Old-1 session content sha `f969c8f0bf2e068ac3ba10560d15215adf69460227e559fee717a352324e14d0`. Old-1 snapshot content sha `c00cf3871af9a86788e80e78c8895742cd232045a72f509783b56f551d49c9b9`. Old-1 drop-test content sha matches the fixed file above.

## Result used for the verdict

Fixed drop: both original backends absent at 55 ms on a `pg_stat_clear_snapshot()` poll, first sample still busy at 1 ms, `snapshot_finished=0`, reader audit clean. Cancel and keys completed. Smoke lib and scoped clippy were 0 before those probes.

The one old-blob run was also absent at 58 ms under the same observer. a139 and a141 red-1 stay in their own evidence trees. See `FINDINGS.md` for which observation is a stale stat copy and which is a graceful close that had not finished.

`hashes.sha256` lists every file in this evidence directory except itself. `cleanup.txt` records removal of only this task's containers, volumes, network, and `/tmp` archive.
