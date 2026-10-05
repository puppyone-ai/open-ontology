# a216 cancel reservation review

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a216`. Independent review of frozen commit `8434b451b597177a4a8b60f9d5031dc73cbf14bf` ("gateway: own each cancel reservation with its future"). Parent `99b25ff3d8cfefe11484017e3e31cfb6d3c9e4db`. Tree `b3f85cd335ab3b1f77d44a5dc6371422ef04a0db`.

The commit contains only:

- `crates/oo-gateway/src/query_executor.rs`
- `crates/oo-gateway/tests/query_executor_pg_live.rs`
- `contracts/gateway/query-executor-v1.md`

Author evidence is a210 `31767bbcbdf7dadad390f604d17d923c50329d22`. The request this review checks is a205 `cd1e7492b20fc0dd1c0ac0261ac547ebfeace671`, verdict REQUEST_CHANGES, on unfixed source `0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1`. This review did not edit product sources, manifests, the lock, or the ledger. a207 remains the connector-manifest and root-lock writer.

No `AGENTS.md` is present. The contract section is "Cancel, failure, reuse" in `contracts/gateway/query-executor-v1.md`. SP06-W04, SP06-W05, and SP06-W06 stay open. `UNTRUSTED_ADMISSION` stays `Blocked`. This review does not accept a parent item and does not approve a release.

## What was reviewed

`CLEANUP_BOUND` is `Duration::from_millis(750)` at `query_executor.rs` line 104. `CancelHandle` is `Clone` and has no `Drop`. The future that calls `try_reserve_cancel` owns the reservation through `ReservationGuard`. Drop before `cancel_query` releases only that generation and leaves the session open. Drop after transport fail-closes that generation with no await. A clone that did not reserve returns `NotSent`. Producer exit with a reservation still set fail-closes; an empty reservation leaves the session open.

Smoke hashed the immutable archive before the review overlay existed. Executor SHA-256 `7604e5ee02bd493c5cc03e355c4c98f0c69f36bd601a07dcb6b76b67153df094`. Archive live test `fb775ed2d040bb266d453fa27282494c89f6dcf1867a146712047b91453cb0f2`. Contract `ec218bb896c7a56600cfc108ae40bd8c2f3c0c90b791fa0ff663af3d08fc9a47`. Those three blobs are in `source/blobs/`. The proof overlay replaces only the live test. Its SHA-256 is `878989945a62db5bfa327d7c4371c0d437ed7accda99f5e299c9bb34ae7bffa0` (`linux/proof/overlay-sha256.txt`). The proof's executor and contract hashes stay on the frozen blobs (`linux/proof/owned-sha256.txt`).

## Linux Rust 1.94 and PostgreSQL 17.10

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, rustc `1.94.0` (`4a4ef493e 2026-03-02`). `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel `1.98.1`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, server `17.10 (Debian 17.10-1.pgdg12+1)`, container `5564f7610ecd`. Private network `oo-a216-net`. Cargo volume `oo-a216-cargo`, target volume `oo-a216-target`, data volume `oo-a216-pgdata`.

Each Rust builder drops all capabilities, sets no new privileges, memory 6 GiB (`6442450944`), 1 CPU (`NanoCpus=1000000000`), no host network, and no published ports. PostgreSQL is 1 GiB, 1 CPU, `CapDrop=["ALL"]`, no published ports. The server certificate names `oo-a189-pg`, `oo-a216-pg`, `oo-a216-proxy`, and `oo-a216-observe`. The Rust builder mounts `ca.crt` plus the two password files. It does not mount `server.key`.

Recipe origin is `recipe-origin.txt`. Launch shape comes from a210 commit `31767bbc` and the a200 archive check at commit `a587031c` (`linux/smoke/launch.sh` SHA-256 `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163`). `extract-libclang.sh` SHA-256 `009abe681bef27f4abee7fa2f9d1d6a320a292d71c6ba7d801bdcdad0f027003` and `pg/pg-wrap.sh` SHA-256 `c159c8a156c7f7fcb64962d3e01a0def5edb5000754ee6a4512ba01aaa4f5836` are unchanged copies. Memory moved from 8g to 6g, CPUs from 4 to 1, and the script cap from 6 to 10.

| Run | Result |
|---|---|
| `linux/smoke` | Exit 0 before the overlay. Container `6016d8d31bc5504220e341103e5e626110b8d1a5d50115b892d94fd824400d07`. Archive of `8434b451`, overlay absent. Lib tests 20 passed. Clippy `-D warnings` finished in 1m 26s, exit 0. `fence.txt` records line 104 at 750 ms and `CancelHandle` `Drop` absent. Capacity `host_cargo=0 rust_builders=1 global=1 cap=10`. The other builder was `oo-a215-preoverlay`. `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, memory `6442450944`, `NanoCpus=1000000000`, `PortBindings={}`. |
| `linux/proof/attempt-1` | Exit 101. Container `eec22c0146c5d248fa053fa1a7c95877439b11c6f4f7c0ddb128b9ce7fc3eeb5`. Lib tests 20 passed. `live_cancel_raii_review` panicked because `drop(stream)` ran while the cancel proxy was still armed. `why-stopped.txt` records the harness stall. |
| `linux/proof/attempt-2` | Exit 101. Container `ed1b9049220e7e9427b4792525908fb7902663732bfd055262374fb0f15cca97`. The proxy was disarmed before `drop(stream)`. The next execute was still `closed`. A consumer drop closes the session on its own. Preserved. |
| `linux/proof/attempt-3` | Exit 101. Container `e79e00458a246cd5029d7087ed7ea4b6c02d28e0ef37a6f347d7a6c627d80567`. `before_transport` passed. `review_brief` returned before `pg_stat_activity` showed the query. Preserved. |
| `linux/proof` | Exit 0. Container `ed141a47fff60895fe40d8cd10afca1de014d2eef2401a0c8f003635796825a0`. Capacity `host_cargo=0 rust_builders=0 global=0 cap=10`. Lib tests 20 passed. `live_cancel_raii_review` passed in 4.06s. Clippy `--lib --test query_executor_pg_live -D warnings` finished in 1.81s, exit 0. Same 6 GiB, 1 CPU, `OOMKilled=false`, `Privileged=false`, `PortBindings={}`. |

`linux/proof/phases/` exits are 0 for clean, libclang, unsafe, lib-test, lib-guard, live, clippy-component, and clippy. The live campaign `live_permission_bound_select` was not run.

## Oracle

`linux/proof/oracle.txt` matches `live/oracle.txt`. `OO_A216_LIVE=1`.

```text
review=ran
untrusted_admission=blocked
healthy_select=rows:1 status:succeeded rollback:yes open:yes
before_transport=join:aborted clone:not_sent during:busy holds:0 read:ok rows:1 idle:ok open:yes next:started gen_before:1 gen_at_release:1 unexpected:0
owner_abort=join:aborted row:row idle:ok open:no during:busy clone:not_sent first:closed after_1200ms:closed retry:not_sent backend:gone codes:sslrequest holds:1 unexpected:0
abort_then_drop=join:aborted idle:ok drop_ms:0 open:no status:partial rollback:no sqlstate:none clone:not_sent next:closed backend:present codes:sslrequest holds:1
leftover=alive_at_hold:true alive_after_idle:false join:timeout-none idle:ok drop_ms:740 open:no next:closed backend:present codes:sslrequest,sslrequest holds:2
review_result=compared
```

`before_transport` aborted the owning future before any SSLRequest (`holds:0`). The generation stayed 1. The blocked select then finished (`rows:1`) and the next execute was `started` with the session open. `owner_abort` and `abort_then_drop` held SSLRequest, the clone stayed `not_sent`, and the next execute after the producer exited was `closed` at once and, for the abort case, after 1200 ms. `abort_then_drop` is `partial`, rollback unconfirmed, backend row present. `owner_abort` reads `backend:gone` after the short brief finished and the session closed. That activity read is not evidence that cancel stopped PostgreSQL. `leftover` closed the session. The owning future had already returned `Timeout` by the end of `wait_idle` (`alive_after_idle:false`, `join:timeout-none`), and the producer wait was 740 ms with two SSLRequest holds. The isolated empty-versus-held leftover check is the library test, which passed.

## Limits

- `abort_then_drop` leaves the backend row present and does not confirm rollback.
- The live leftover case does not keep the owning future alive until after the producer goes idle. `producer_leftover_reservation_fail_closes_and_an_empty_one_does_not` is the isolated check.
- The after-transport live lines do not print the generation. Correct-generation release and fail-close are the two `ReservationGuard` library tests, re-run here.
- Full gateway public admission stays `Blocked`.
- SP06-W04, SP06-W05, and SP06-W06 stay open.
- This review does not accept a parent item and does not approve a release.
