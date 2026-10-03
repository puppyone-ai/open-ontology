# R2-S bounded Arrow snapshot stream

Session `eef831aa-dba4-4af2-a78b-541bc9e0a131`. Slot B. This is the author delivery for the production pull, protocol `oo.source.pgsql.stream/v1`.

Source commit `75b993d0b578fa863a32c64abc0073ed92d51a5d`. Parent `92ff8b1fdb9d3480c182689bcfda5652f4de4487`. The functional builder compiled a git archive of `ccd93e1adc34b24ab998d90972549f8cffb59f0d` (tree `ef43877346afadcdccaf26fe6982c6b4e8b65902`) plus the owned overlay. That overlay is the content of the source commit. The builder did not compile the dirty worktree or the moving vendor tree.

## Final Linux 1.94 run

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux`, `arch=arm64`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Container `oo-a131-msrv-stream`. Network `oo-a131-eef831aa-net`. Privileged false. Cap drop ALL. Memory 8589934592. No host port. Wrapper log line `EXIT:0`. `elapsed_seconds=13`.

| phase | exit |
| --- | --- |
| toolchain | 0 |
| snapshot | 0 |
| clippy-component | 0 |
| lib (`cargo test --locked --offline -p oo-connectors --lib`) | 0 (57 passed) |
| pg_stream_live | 0 (1 passed, 1.34s) |
| pg_stream_bounds | 0 (1 passed, 1.62s) |
| clippy (`cargo clippy --locked --offline -p oo-connectors --all-targets -- -D warnings`) | 0 |
| docker-run | 0 |

Archive vendor blob `crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs` matched the fixed commit: `f41e0e4dd6baca062fb332ab66da3f5e9c0600fd`.

The same image's earlier smoke run is unchanged and was not repeated. Its toolchain, snapshot, lib, clippy-component, clippy, fetch, inspect, and docker-run exits are 0.

Private PostgreSQL 17.10 container `oo-a131-pg` (`sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`) stayed the original fixture. `pg/up.exit` is 0. Privileged false. Published ports `{}`. Cap drop ALL.

## What the green tests observed

Live outcome, written before the assertions:

- Oracle before the writer: parent 4, child 3, empty 0, zero-column 2, payload 59 bytes.
- Held batch: `state=active`, `wait_event=ClientRead`.
- `executes=13`, `reserved_rows=13`, `consumed_rows=9`, `decoded_payload_bytes=59`, matching the oracle payload.
- Arrow preimage 132 bytes, Arrow assembly 5528 bytes.
- Wire view capacity peak 8192, accepted logical payload 12561, assembly bytes 951021. These are connection-life `BytesMut` view sums sampled when the portal is not borrowing the connection.
- Writer row `id=5` was visible to the admin session and absent from the snapshot cut.
- Finish, abort, and drop each released the owned backends (`pid` 366, 369, 371).
- Reader log had statements and a transaction start. Write-verb count on the `oo-pg-source` tag was 0. `live_complete`.

Bounds outcome:

- `max_value` refusal display: `source error: budget fact=max_value`. The fixture body was not in the display.
- `max_row` refusal display: `source error: budget fact=max_row`.
- Slow consumer stayed on `state=active`, `wait_event=ClientRead`. Server log line count did not change across the 400 ms idle wait, then increased by 1 after the next demand. Executes went from 1 to 2.
- Refused and slow backends were released. `bounds_complete`.

`sqlx-postgres` still prints two unfulfilled `dead_code` expectations in vendor flush and message code. Those warnings were already present on the smoke run. This delivery does not change vendor.

## Earlier exits kept in this directory

| record | result |
| --- | --- |
| `linux/functional/compile-fail-1.exit` | 101, library compile |
| `linux/functional/compile-fail-2.exit` | 101, live-test compile |
| `linux/functional/compile-fail-3.log` | live-test compile, missing semicolon; no separate exit file survived the next launch |
| `linux/functional/prior-runtime-exits.txt` and `live-fail-runtime.log` | lib 0, `pg_stream_live` 101, docker-run 101, wrapper `EXIT:101`, panic `backend 274 stayed active` |
| `linux/functional/prior-green-exits.txt` and `tests-green-clippy-fail.log` | lib, live, and bounds 0; clippy 101 |
| `linux/functional/clippy-fail-bounds.log` | live and bounds tests 0; clippy 101 on the bounds test |

`linux/functional/console.log` is the final green run only. The files above keep the failures.

Owned cleanup is in `cleanup.txt`.
