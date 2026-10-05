# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a196`. Source commit `0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1`. Parent `39d2851cd70b2591768d85025550dde0924bf81a`. Baseline `15d23fe9f2cbc74e3f3278d70d0f56390568126e`. Tree `358029744eb8755834953fdda7bf6cfaf44edf9e`.

The next step is an independent review of this commit. This directory is the author record. It does not close SP06-W04, SP06-W05, or SP06-W06, and it does not claim a release.

## Read first

- `contracts/gateway/query-executor-v1.md`, section "Cancel, failure, reuse"
- `crates/oo-gateway/src/query_executor.rs`, `cancel` at line 877, `await_hot` at line 1279, `shutdown_local` at line 754, `budget_left` at line 2072
- `crates/oo-gateway/tests/query_executor_pg_live.rs`, `prove_cancel_transport_bounds`
- `docs/17-rust-implementation.md`, the cleanup paragraph: a late cancel must not hit a connection lent to the next user, reuse waits until cancel and cleanup finish, exceeding the cleanup limit destroys the connection, and `JoinHandle::abort` does not prove the PostgreSQL backend stopped
- `linux/smoke` for the pre-edit Rust 1.94 run
- `linux/red/attempt-1` for the preserved harness deadlock, and `linux/red` for the reproduced baseline stall
- `linux/proof/attempt-1` for the preserved clippy `dead_code` failure, and `linux/proof` for the green run
- `live/oracle.txt` matches `linux/proof/oracle.txt`
- `source/blobs/` matches the source commit. `source/commit.txt`, `source/parent.txt`, `source/baseline.txt`, and `source/tree.txt` are the hashes. `source/blob-sha256.txt` maps each blob
- `FINDINGS.md` is the author reading

## Re-run

Pinned Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` and PostgreSQL `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `RUSTUP_TOOLCHAIN=1.94.0`. The workspace `rust-toolchain.toml` still says channel `1.98.1`. A fresh container does not keep clippy. Install it inside the container that runs it. `--locked --offline` needs the warm cargo home this task deleted; a new volume has to fetch once before offline proof.

The live test expects host `oo-a189-pg`, database `oo_a189`, and the fixture directory `/run/oo-a189/fixture`. The proof PostgreSQL advertised `oo-a189-pg`, `oo-a196-pg`, and `oo-a196-proxy`. `OO_A189_LIVE=1` runs the campaign. `OO_A196_RED=1` is only the red archive harness and must stay off for the green proof.

```text
cargo test --locked --offline -p oo-gateway --lib
cargo test --locked --offline -p oo-gateway --test query_executor_pg_live -- --nocapture live_permission_bound_select
cargo clippy --locked --offline -p oo-gateway --lib --test query_executor_pg_live -- -D warnings
```

One Rust builder, 8g, and one PostgreSQL 17.10. The green run's builder saw `host_cargo=0 rust_builders=0 global=0` against a cap of 6.

## Review notes

The red run is the baseline bug: cancel TCP still blocked at 1501 ms with the backend active, and the next generation became active while that TCP was held. The green run is the repair: stalled drop 759 ms with rollback not confirmed and the backend row still present, stalled handle 753 ms with `SessionBusy` while the reservation stood and `SessionClosed` after the deadline, healthy drops at 7 ms and 6 ms with rollback confirmed. Whole-page client budget is SQLSTATE absent at 3050 ms. The older server-timer budget stayed `57014`.

`DedicatedSession` drop aborts the local producer and driver so a cancel future owned by the session is not detached. That abort is not the cancel mechanism. The stalled proofs bound `cancel_query` with `tokio::time::timeout` and drop the future when the deadline expires.

The green overlay hashes are the source commit. `linux/proof/attempt-1` passed the live campaign on the earlier bytes, where `budget_left` was unused outside the unit test, and then clippy exited 101. Do not treat that attempt's tree as the commit.

## Cleanup

`cleanup.txt` lists what this task removed after the green proof: container `oo-a196-pg` `7a524e461eff8887d3e08c0c92df784cd665bb918aa23482704837c6b544ed8c`, volumes `oo-a196-cargo`, `oo-a196-target`, and `oo-a196-pgdata`, network `oo-a196-net`, and `/tmp/oo-a196-eef831aa-dba4-4af2-a78b-541bc9e0a196`. The pinned images were left. `docker prune` was not run. Foreign containers were left. `SHA256SUMS` covers every file here except itself. A separate fixed review has not run.
