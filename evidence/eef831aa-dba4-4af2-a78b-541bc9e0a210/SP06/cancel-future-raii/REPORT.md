# a210 cancel future RAII

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a210`. This slice repairs the dedicated-session cancel reservation so the future that reserved a generation owns that reservation. Aborting that future after the transport has entered fail-closes the session. The next `execute_trusted_select` after the producer exits is `SessionClosed`.

Source commit `8434b451b597177a4a8b60f9d5031dc73cbf14bf` on parent `99b25ff3d8cfefe11484017e3e31cfb6d3c9e4db`. Tree `b3f85cd335ab3b1f77d44a5dc6371422ef04a0db`. The fixed baseline is `0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1` (parent `39d2851cd70b2591768d85025550dde0924bf81a`, tree `358029744eb8755834953fdda7bf6cfaf44edf9e`). The executor at that baseline is the a196/a205 source. a205 review `cd1e7492` requested this repair. The commit contains only:

- `crates/oo-gateway/src/query_executor.rs`
- `crates/oo-gateway/tests/query_executor_pg_live.rs`
- `contracts/gateway/query-executor-v1.md`

`source/blobs/` matches that commit. `source/blob-sha256.txt` records the SHA-256 of each blob. Those digests match `linux/proof/owned-sha256.txt`. No manifest, lock, library policy, parser, pipeline, or control file was edited. `crates/oo-connectors/Cargo.toml` was dirty in the worktree from another task and was left unstaged. No parent checkbox was changed. No release is claimed. SP06-W04, SP06-W05, and SP06-W06 stay open. This repair is not an independent approval of the parent goal.

## Behavior

`CLEANUP_BOUND` stays 750 ms (`query_executor.rs` line 104). `CancelHandle::cancel` (line 886) still calls `try_reserve_cancel` before any await. The owning future holds a `ReservationGuard` (line 2146). `CancelHandle` stays `Clone` and has no `Drop`. The session's clone does not own the reservation.

Drop before `cancel_query` is entered calls `release_own_reservation` (line 2194). That clears the reservation only when `cancel_reserved` is still `Some(this generation)`, leaves the session open, and notifies waiters. Drop after `enter_transport`, and drop after the future has already chosen fail-close for an observe error or a zero remainder, calls `fail_close_owned` (line 2206). That runs on the dropping thread, does not await, clears only that generation, sets `open` and `accepting_cancel` false, signals destroy, and notifies waiters. A foreign generation is a no-op. Success disarms the guard before `mark_cancel_packet`, so a reservation taken after the packet is recorded is left alone.

`execute_trusted_select` still returns `SessionBusy` while `cancel_reserved` is set (line 643) and `SessionClosed` when the session is not open. `settle_lend` (line 2280) calls `close_leftover_reservation` (line 2223) when the producer leaves without reuse, and again after `wait_cancel_settled` on the reuse path. An empty reservation is left open. A reservation that is still set fail-closes under the same lock. `shutdown_local` (line 758) is unchanged local teardown. It is not `cancel_query`, and it does not prove the PostgreSQL backend stopped. `query_executor.rs` contains no `unsafe`. `UNTRUSTED_ADMISSION` stays `Blocked`.

## Linux Rust 1.94 and PostgreSQL 17.10

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, rustc `1.94.0` (`4a4ef493e 2026-03-02`). `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel `1.98.1`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, server `17.10 (Debian 17.10-1.pgdg12+1)`. Private network `oo-a210-net`. Cargo volume `oo-a210-cargo`, target volume `oo-a210-target`, data volume `oo-a210-pgdata`. The Rust builder drops all capabilities, sets no new privileges, memory 8g, 4 cpus, no host network, and no published ports. PostgreSQL is the private server on that network. The server certificate names `oo-a189-pg`, `oo-a210-pg`, `oo-a196-proxy`, and `oo-a210-proxy`. One Rust builder for this task at a time, and one PostgreSQL. Host cargo was not used to compile.

| Run | Result |
|---|---|
| `linux/smoke` | Exit 0 before any source edit. Container `15c651584d1ad96bdda9d8b52658ccfd0fb5aa7846a257cf6a623737957c435b`. Immutable archive of `0ee7cf86`, overlay absent. Owned SHA-256 matches the baseline blobs (`2ad166e91a407f5bff39d814d7beb4a1bc69710e30851260528184f3e8558925` executor). `cargo test -p oo-gateway --lib --locked --offline` 17 passed. Clippy `--lib --test query_executor_pg_live -D warnings` finished in 23.00s, exit 0. Capacity `host_cargo=0 rust_builders=0 global=0 cap=6`. `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, memory `8589934592`, `PortBindings={}`. |
| `linux/red/attempt-1` | Exit 101. Container `88ebf61524ad1f147a55b46f31735bcb57136bb1c4cbf973c21aafe8db7b1114`. The overlaid test borrowed `session` twice in one call (`E0499`). The abort assertion was not reached. Preserved. |
| `linux/red` | Exit 101. Container `2b295af90229816d74828bcfd6676beca6af33f2e6b239202c6daf64e5f65471`. Archive executor, SHA-256 `2ad166e91a407f5bff39d814d7beb4a1bc69710e30851260528184f3e8558925`, no `ReservationGuard`. The new test overlaid. Panic: `owning cancel abort left busy after_1200ms:busy; aborted drop left busy`. `OOMKilled=false`, `Privileged=false`. |
| `linux/proof/attempt-1` | Exit 101. Container `1fa12b65fd1dda123da4d713f0751f2046e7d70a4ba3a7017678b9dcc6c5f0bb`. `cargo test --lib` finished in 0.08s and ran the smoke binary (17 tests). The mounted executor hash was already the repair, and cargo treated the package as fresh, so the live test still executed the unfixed library. Preserved. |
| `linux/proof/attempt-2` | Exit 101. Container `0eb9bc7d297edeaa01576c6423d9caf531926d07a1181eedae5108e7be672098`. After `cargo clean -p oo-gateway`, 20 tests ran. The before-transport guard passed. The two fail-close tests dropped the only `watch` receiver, so `Sender::send(true)` on tokio 1.53.1 did not store the flag. Reservation clear and `open=false` had already passed. Preserved. |
| `linux/proof` | Exit 0. Container `633d3ac31c0645d3d898ea895fb236c2e64cb761082ed5215f6bf3a506c471c7`. Capacity `host_cargo=0 rust_builders=0 global=0 cap=6`. Lib tests 20 passed. `live_permission_bound_select` passed in 22.92s. Clippy `--lib --test query_executor_pg_live -D warnings` finished in 1.33s, exit 0. `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, memory `8589934592`, `PortBindings={}`. |

The green proof mounted the immutable archive and overlaid the three workspace files. `linux/proof/phases/clean.exit`, `lib-test.exit`, `lib-guard.exit`, `live.exit`, `clippy-component.exit`, and `clippy.exit` are all 0.

## Oracles

`linux/red/oracle.txt` is the unfixed executor. `linux/proof/oracle.txt` is the repaired executor. Both ran `live_permission_bound_select` with `OO_A189_LIVE=1`.

| Fact | Red (exit 101) | Proof (exit 0) |
|---|---|---|
| `aborted_success` | `join:aborted row:row idle:ok idle_ms:487 open:no status:succeeded rollback:yes socket:yes sqlstate:none during:busy clone:not_sent during_again:busy first:busy after_1200ms:busy retry:not_sent after_retry:busy backend:gone codes:sslrequest holds:1 forwarded:1 unexpected:0` | `join:aborted row:row idle:ok idle_ms:0 open:no status:succeeded rollback:yes socket:yes sqlstate:none during:busy clone:not_sent during_again:busy first:closed after_1200ms:closed retry:not_sent after_retry:closed backend:gone codes:sslrequest holds:1 forwarded:1 unexpected:0` |
| `aborted_drop` | `join:aborted idle:ok drop_ms:722 open:no status:partial rollback:no sqlstate:none clone:not_sent next:busy backend:present codes:sslrequest holds:2` | `join:aborted idle:ok drop_ms:0 open:no status:partial rollback:no sqlstate:none clone:not_sent next:closed backend:present codes:sslrequest holds:1` |

On the red run the earlier asserts had already passed: both code strings contain `sslrequest`, both joins are `aborted`, both idles are `ok`, `during` is `busy`, `retry` is `not_sent`, and `unexpected` is 0. The failure is the closed assertion.

On the proof run the same live test still records `deadline=57014 elapsed_ms:1791`, `inflight_cancel=sent 57014`, `late_cancel=not_sent then sent`, `stalled_drop=ms:761 status:partial rollback:no backend:present slo:miss accepts:1`, and `stalled_handle=ms:752 busy:yes status:succeeded closed:true rollback:yes`. `aborted_drop` still has the backend row present and rollback unconfirmed. The success path's `backend:gone` is the activity read after that short query finished. It is not a claim that cancel stopped a backend.
