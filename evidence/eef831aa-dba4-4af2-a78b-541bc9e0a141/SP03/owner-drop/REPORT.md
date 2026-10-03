# REPORT — a141 blocked owner Drop

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a141`. Sole coder, no subagents and no worktree.

Fixed input `58afa97ded85a7dd89c72532bfc8973c45255d7c`, tree `543fd103532a8366ce92a0b4eaea4e089eb76d2d`. Source commit `6b95d6f2fd2259edfcbffc27a6338518a64dd0fc`, parent `6cdb559b45ea9d0fed709ca74d80eecf264d8016`, subject `connectors: drop the reader socket after bounded rollback`. The four files are `crates/oo-connectors/src/snapshot.rs`, `crates/oo-connectors/src/session.rs`, `crates/oo-connectors/tests/pg_reader_drop_live.rs`, and `contracts/source/pgsql/reader-lifecycle-v1.md`. `stream.rs`, `pg_reader_cancel_live.rs`, and `pg_reader_keys_live.rs` match `58afa97`.

Every Linux run is `git archive` of `58afa97` plus that run's overlay. The pipeline checkout was not moved. a139 evidence `2ae9f09fff809be98396de9b096667375c123645` was not modified.

## Images and isolation

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `--pull never`. Container user `0:0`, `--cap-drop ALL`, no-new-privileges, memory 8 GiB, cpus 4, pids 4096, `HOME=/tmp`. Private network `oo-a141-eef831aa-net` (`0f390b284260`). No host network, docker socket, or HOME mount. Cargo and target volumes `oo-a141-eef831aa-cargo` and `oo-a141-eef831aa-target`.

PostgreSQL image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `linux/arm64`. `pg/init.stdout` records `init_ok version=17.10 (Debian 17.10-1.pgdg12+1)`. Container `oo-a141-pg`, no host port. `pg/up.exit=0`, `pg/init.exit=0`. Password files stayed under `/tmp/oo-a141-eef831aa-dba4-4af2-a78b-541bc9e0a141/secrets`. `pg/pointer-keys.txt` lists pointer fields only.

One owned Postgres and one owned Rust builder at a time. Host `cargo` / `rustc` / `rustdoc` / `rustup` was 0 at the green-3 and clippy-1 launches (`linux/green-3/capacity.txt`, `linux/clippy-1/capacity.txt`).

## Exits

| run | docker | drop | cancel | keys | clippy | note |
| --- | --- | --- | --- | --- | --- | --- |
| `linux/smoke` archive only, before edits | 0 | | | | 0 | lib 0, clippy component added |
| `linux/red-1` new drop test on `58afa97` | 101 | 101 | | | | preserved |
| `linux/green-1` socket drop, stale stat snapshot | 101 | 101 | | | | preserved, cancel and keys not run |
| `linux/diag-1` | | | | | | console and stacks preserved; container exited 101 |
| `linux/diag-2` | | | | | | console and stacks preserved; container exited 101 |
| `linux/green-3` refreshed observation | 1 | 0 | 0 | 0 | 1 | clippy missing from the fresh image |
| `linux/clippy-1` same overlay hashes | 0 | | | | 0 | component added, then clippy |

green-3 host config: `ExitCode=1`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=oo-a141-eef831aa-net`, `Memory=8589934592`, `CapDrop=[ALL]`. clippy-1 host config matches those isolation fields with `ExitCode=0`. Drop finished in 0.49 s. Cancel finished in 22.49 s. Keys finished in 1.24 s. Clippy `Checking` finished in 1.22 s after the component download.

## Oracles

Drop absence and reader SQL: `linux/green-3/drop-outcome.txt`. Server lines for pids 2386 and 2387, including the reset at `22:22:46.437`: `linux/green-3/postgres-window.log`.

Cancel: `linux/green-3/cancel-outcome.txt`.

Keys: `linux/green-3/keys-outcome.txt`.

Stale-snapshot contradiction: `linux/diag-2/console.log` and `linux/diag-2/stacks.txt`.

Overlay identity for green-3 and clippy-1: `expect-snapshot.sha256`, `expect-session.sha256`, `expect-stream.sha256`, `expect-lifecycle.sha256`, `expect-drop-test.sha256`, and `archive-sha256.txt` match across the two directories.

## Attempts kept

`linux/red-1`, `linux/green-1`, `linux/diag-1`, `linux/diag-2`, and `linux/green-3` were not overwritten. `linux/smoke` is the pre-edit library and clippy smoke.

## Cleanup

`cleanup.txt` names this handle's containers, volumes, network, and `/tmp` paths. They are removed after the evidence commit. No prune. No foreign containers.
