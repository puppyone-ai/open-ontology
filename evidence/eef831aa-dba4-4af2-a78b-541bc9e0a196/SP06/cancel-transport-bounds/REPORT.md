# a196 cancel transport bounds

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a196`. This slice repairs the dedicated-session SELECT executor so cancel transport and the following cleanup share one 750 ms deadline, and a pending `CancelHandle` fences the active generation until that result is known.

Source commit `0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1` on parent `39d2851cd70b2591768d85025550dde0924bf81a`. The fixed baseline is `15d23fe9f2cbc74e3f3278d70d0f56390568126e`. The commit contains only:

- `crates/oo-gateway/src/query_executor.rs`
- `crates/oo-gateway/tests/query_executor_pg_live.rs`
- `contracts/gateway/query-executor-v1.md`

`source/blobs/` matches that commit. `source/blob-sha256.txt` records the SHA-256 of each blob. No manifest, lock, library policy, parser, pipeline, or control file was edited. `crates/oo-gateway/Cargo.toml` was dirty in the worktree from another task and was left unstaged. No parent checkbox was changed. No release is claimed. SP06-W04, SP06-W05, and SP06-W06 stay open. An independent review of this commit has not run.

## Behavior

`CLEANUP_BOUND` stays 750 ms (`query_executor.rs` line 100). The first cleanup action stores one `Instant` at `now + 750 ms`. Observe, cancel connect, TLS, the cancel send, the pinned server response, and `ROLLBACK` spend that same deadline. A later phase calls `budget_left`, which is `saturating_duration_since`. Zero remaining means the phase does not start. The clock does not grow after it starts.

`CancelHandle::cancel` (line 877) reserves the accepting generation before any await. `execute_trusted_select` returns `SessionBusy` while the producer is running or while that reservation stands. A successful query waits for the reservation to clear, then lends only when `may_lend` is true: the session is open, nothing is reserved, and no cancel request was written. Lending clears `accepting_cancel` in that same lock. If the reservation is still there when the deadline is gone, or a packet was written, the client is dropped and the socket is marked dropped. The summary status of that successful query stays `succeeded` with rollback confirmed.

A transport error or an owned-budget timeout calls `fail_close_session` (line 2144): `open` and `accepting_cancel` become false, the reservation is cleared, and the destroy watch is signaled. The error is the existing `Protocol` step `cancel`, or `Timeout` with SQLSTATE absent. The summary does not say the backend is gone. A later `pg_stat_activity` read is the only absence check. `Ok(None)`, or the same pid with a different `backend_start`, means that row is absent. An observe error stays unconfirmed. Deadline expiry is an SLO miss.

A handle that does not match the accepting generation does not reserve and does not send. Its own observe uses a separate 750 ms budget and does not start the session clock. A second cancel of a reserved generation returns `NotSent`.

`DedicatedSession` drop and `close` call `shutdown_local` (line 754), which clears the reservation, signals destroy, takes the client, and aborts the local producer and driver. That abort is local teardown. It is not `cancel_query`, and it does not prove the PostgreSQL backend stopped. The production path does not spawn the cancel future and does not read the cancel secret. `query_executor.rs` contains no `unsafe`.

The module comment and the contract no longer say a 500 ms process-isolated parser worker is absent. The parser worker exists. Its kernel sandbox is unsupported and this crate has no authentication gate, so `UNTRUSTED_ADMISSION` stays `Blocked`. Role bind and the typed cell set are unchanged.

## Linux Rust 1.94 and PostgreSQL 17.10

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, rustc `1.94.0` (`4a4ef493e 2026-03-02`). `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel `1.98.1`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, server `17.10 (Debian 17.10-1.pgdg12+1)`. Private network `oo-a196-net`. Cargo volume `oo-a196-cargo`, target volume `oo-a196-target`, data volume `oo-a196-pgdata`. The Rust builder drops all capabilities, sets no new privileges, memory 8g, 4 cpus, no host network, and no published ports. PostgreSQL is 1g, 1 cpu, with `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, and `SETGID` only. The server certificate names `oo-a189-pg`, `oo-a196-pg`, and `oo-a196-proxy`. One Rust builder and one PostgreSQL at a time. Host cargo was not run.

| Run | Result |
|---|---|
| `linux/smoke` | Exit 0 before any source edit. Container `0854f0dc24aed6707c8f2e9f3e11602167ab1f3fecfb7c4b8903df5b4b00331f`. Baseline archive. `cargo test -p oo-gateway --lib` 15 passed. Clippy `--lib --test query_executor_pg_live -D warnings` exit 0. `OOMKilled=false`, `Privileged=false`. |
| `linux/red/attempt-1` | Exit 137. The race step dropped the finished stream before `wait_idle`, so a second unbounded `cancel_query` stayed on the stalled proxy. `oracle-partial.txt` has `red_drop=blocked:true ms:1503 active:true accepts:1`. Preserved. |
| `linux/red` | Exit 0. Container hostname `8fb1320bfe10`. Same baseline archive plus the red harness. `red_drop=blocked:true ms:1501 active:true accepts:1`. `red_race=held:true next_started:true next_active:true cancel:sent`. `red_result=reproduced`. |
| `linux/proof/attempt-1` | Exit 101. Lib tests and the live campaign exited 0. Clippy `-D warnings` rejected unused `budget_left`. `oracle.txt` is that run. Preserved. |
| `linux/proof` | Exit 0. Container hostname `a7204ecf0e36`. Capacity `host_cargo=0 rust_builders=0 global=0 cap=6`. Lib tests 17 passed. `live_permission_bound_select` passed in 20.89s. Clippy `--lib --test query_executor_pg_live -D warnings` finished in 1.44s, exit 0. `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, memory `8589934592`, `PortBindings={}`. |

The green proof mounted the immutable archive and overlaid the two workspace Rust files. Their SHA-256 values match `source/blob-sha256.txt` and `linux/proof/owned-sha256.txt`. The contract hash is in `source/blob-sha256.txt`.

## Green oracle

`linux/proof/oracle.txt` is the same file as `live/oracle.txt`.

| Fact | Value |
|---|---|
| `untrusted_admission` | `blocked` |
| `deadline` | `57014 elapsed_ms:1838` |
| `inflight_cancel` | `sent 57014` |
| `late_cancel` | `not_sent then sent` |
| `client_budget` | `57014 elapsed_ms:1786 late:not_sent` |
| `drop_stream` | `partial end_audit_ok:false` |
| `finished_drop_next` | `still_active drop_ms:7` |
| `lock_drop_ms` | `7`, status `partial`, rollback `yes`, socket `yes`, end audit `yes` |
| `sleep_drop_ms` | `6`, status `partial`, rollback `yes`, socket `yes`, end audit `yes` |
| `stalled_drop` | `ms:759 status:partial rollback:no backend:present slo:miss accepts:1` |
| `stalled_handle` | `ms:753 busy:yes status:succeeded closed:true rollback:yes` |
| `whole_page_budget` | `sqlstate:none elapsed_ms:3050 rollback:no` |
| `live_result` | `compared` |

Healthy lock and sleep drops returned in 7 ms and 6 ms, both under 1 s, with rollback confirmed. The stalled drop returned in 759 ms and the stalled handle in 753 ms. Both sit inside the 700 ms to 1000 ms window the test requires. The stalled drop's backend row was still present. The whole-page client budget fired with SQLSTATE absent at 3050 ms, and that backend row was absent on a later read. The existing server-timer client budget stayed `57014`.

## Hashes

| Blob | SHA-256 |
|---|---|
| `query_executor.rs` | `2ad166e91a407f5bff39d814d7beb4a1bc69710e30851260528184f3e8558925` |
| `query_executor_pg_live.rs` | `7d704e52fc23fe7cdafb20f9830f7e4017182affa9de8304c603fefbca32f0ca` |
| `query-executor-v1.md` | `83fc0172b1d5d03c5524ad2487e9d8015f26575e684f91b82e243787d1c5b255` |

`SHA256SUMS` is the SHA-256 of every file in this directory except itself, from host `shasum`. No second Rust builder was started for that manifest.

## Cleanup

`cleanup.txt` records removal of container `oo-a196-pg`, volumes `oo-a196-cargo`, `oo-a196-target`, and `oo-a196-pgdata`, network `oo-a196-net`, and `/tmp/oo-a196-eef831aa-dba4-4af2-a78b-541bc9e0a196`. The pinned Rust and PostgreSQL images remain. `docker prune` was not run. Foreign containers were left in place.
