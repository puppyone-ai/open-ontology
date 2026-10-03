# REPORT — a129 receive-view shrink repair

The meter no longer panics when the sum of the two live `BytesMut` windows falls. The read path, the copy path, and the protocol errors are unchanged.

| | |
| --- | --- |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a129` |
| Source commit | `79515fcf7e7032c8c212540eaae4b48a5957e3a2` |
| Source parent | `3f66773adf1afe378fc73a8ca56abf25bcb556cf` |
| Source tree | `b78f84497484f6232aecf0b51ffcc6d27be9fd7c` |
| Archive base | `ccd93e1adc34b24ab998d90972549f8cffb59f0d` |
| Archive tree | `ef43877346afadcdccaf26fe6982c6b4e8b65902` |
| Archive parent | `182702e6e67d16de10f280eaf422fce4e2f5a5f2` |
| Image | `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` |
| OS / arch | linux / arm64 |
| Toolchain | rustc 1.94.0 (`4a4ef493e` 2026-03-02), cargo 1.94.0 (`85eff7c80` 2026-01-15), host `aarch64-unknown-linux-gnu` |
| Override | `RUSTUP_TOOLCHAIN=1.94.0` (`rust-toolchain.toml` still says 1.98.1) |
| bytes | 1.12.1, pinned `=1.12.1` only in the container copy of sqlx-core |

`cargo test -p sqlx-core` cannot run here: sqlx-core is not a workspace member. The unit test is copied to `/tmp/sqlx-core-pkg`, the copy's bytes dependency is pinned, and `cargo test --manifest-path /tmp/sqlx-core-pkg/Cargo.toml --lib` runs without `--offline`. Workspace tests use `--locked --offline`. No repository manifest was edited.

## Source hashes

RED is the panicking worktree (`red-init/red-source.sha256`). GREEN is the committed repair (`green/green-source.sha256`).

| File | RED | GREEN |
| --- | --- | --- |
| `buffered.rs` | `07552f04868da7d77995458379f2da57782fc28994d962f9bd8e94e2b1d61f98` | `33d460f5005050a9cc3db6f90b022ef51b872e57a7552eec5103b31e10f0e1ba` |
| `PATCH.md` | `cedcf55faccb1e407f81329f1c3c008b3b519fc021c6188b216f69bf1b890c95` | `ef04fa1fec87e47eff8396e0dce91fa00a726c9bc9fc9bb64483816f314a8b60` |
| `pg_wire_view_shrink.rs` | `9b81825f1a21e0e581f31d9bedb62d1b52f420e9b60dc3983e1cc263efb86268` | `9b81825f1a21e0e581f31d9bedb62d1b52f420e9b60dc3983e1cc263efb86268` |

The RED `buffered.rs` still contains both `checked_sub` expects and 512 frames in 64-byte chunks. The GREEN file is the meter fix with 128 frames. `red/red-source.sha256` is an earlier chunk shape that did not panic.

## Exits

| Run | Result |
| --- | --- |
| `smoke/` lib | 0, 55 passed |
| `smoke/` clippy `-p oo-connectors --all-targets -- -D warnings` | 0 |
| `red/` `cargo test -p sqlx-core` | 101, package is not a workspace member. This is not the panic. |
| `red/` `pg_wire_view_shrink` | 0, 1 passed. Plaintext TCP did not panic. |
| `red-init/console-offline.log` | 101, fresh resolve could not see `async-fs` offline |
| `red-init/` unit | 101. Panic at `buffered.rs:590:18`: `incoming view shrank during advance`. `docker-run.exit` 101. Finished in 0.00s. |
| `green/oom-512/` unit | 101, signal 9 SIGKILL. `OOMKilled=true`. Same fixed meter, 512 frames. |
| `green/oom-512/` tcp / lib / bounds / accounting / portal / clippy | 0 / 0 (55) / 0 (13) / 0 (12) / 0 (6) / 0. `docker-run.exit` 101. |
| `green/` unit | 0, 1 passed, 0.00s. `OOMKilled=false`. |
| `green/` tcp | 0, 1 passed, 0.77s |
| `green/` lib | 0, 55 passed |
| `green/` `pg_wire_bounds` | 0, 13 passed |
| `green/` `pg_wire_accounting` | 0, 12 passed |
| `green/` `pg_portal_bounds` | 0, 6 passed |
| `green/` clippy | 0 |
| `green/docker-run.exit` | 0 |

Builder limits: one container, `--cap-drop ALL`, `no-new-privileges`, memory 8g, cpus 4, pids 4096, bridge `oo-a129-eef831aa-net`, user 0:0, `HOME=/tmp`, `--pull never`. No PostgreSQL. No host cargo during these runs.

Raw consoles, hostconfig, and invocation files are in `smoke/`, `red/`, `red-init/`, `green/oom-512/`, and `green/`.
