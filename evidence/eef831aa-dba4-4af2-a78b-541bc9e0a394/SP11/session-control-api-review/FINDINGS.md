# Findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a394`. Independent findings for the a387 session-control-api package at `53017ed29c98c49fb44a7eb18d5df6d5de273046`. These findings do not accept SP11 or the parent goal.

## The token revoke is a PostgreSQL row

`gateway-token-store` inserts `oo_token_control.revocation` for scope `session`, with the project id and session id set and `principal_id` empty. The oracle counted that table. It was 0 before cancel and 1 after cancel, including after the replay cancel. `oo_token_control.token_record` stayed at 1 row for the session. The close path's lookup returned revoked, because a still-valid token would have failed `prove_revoked` and the HTTP close would not have been 200. This is a stored revocation. It is not a process-local boolean.

## Mapping and permit are owned durable rows

`oo_session_api.proxy_mapping.state` moved from `present` to `deleted`. `oo_session_api.permit.state` moved from `held` to `released`. Both changes were visible in the poll while the release-spawned child or the query backend was still alive. `map_ip` was `10.88.0.8` on the first session and on the second session. That address is the literal written by this crate. The tables live in PostgreSQL, so they survive the API process, and they are this crate's own schema. S6, S7, and S10 evidence hashes are sha256 of constant JSON labels. The oracle checked those hashes and also read the row states. The label hash does not by itself prove the row bytes.

## The S1 child is spawned by release

`resource.harness_pid` was null before cancel, and no checkpoint file existed. `session_release::run` spawns `/bin/bash` with a TERM trap that sleeps 6 seconds, writes the pid into the checkpoint, and then stops it while the backend cancel runs. The poll classified that live cmdline as `release_fixture_bash`. `fixture_stage` then returned `fixture_persisted`, phase `releasing`, retryable true, and `acceptable()` false. The oracle records `fixture_stage_counted_as_close` false. HTTP finish on a new process is the close that returned 200.

## S5, S8, S11, and the restart

Default snapshot path was absent. The closed receipt has S5 `skipped` and `actual_absence` false. Planting `snapshot.bin` produced HTTP 409 `snapshot_present_uncollected` and left the store phase unclosed. Removing the file allowed 200 `closed`. After close, the workspace directory was gone, which is the S8 absence this crate records together with `/proc/{pid}` absence. The pool slot used by the session was `destroyed` and a replacement empty slot existed. Phase in `session_store::get` was `closed`.

The author's live restart calls `fixture_stage` in the test process and finishes over HTTP on the same server. This review killed that server, removed `port.txt`, started a new process, and checked that the pid changed before the 200 `closed`.

## Certification and production

Every closed receipt in the oracle had `reducer_physical_release_certified` false, `production_signed` false, and `deployment` `localprocess`. `GetReceipt.physical_release_certified` was false. The session state has no certification field. Production `serve` exited 2 with stderr `runtime_unavailable` and did not write a port. The pinned image has no `runsc`, and this review did not add one. A present `runsc` would still stop at `production_driver_absent` according to `startup_gate`; that branch was not executed here.

## Identity and transport

The listener is HTTP/1.1 on loopback. The principal and project on `current` come from the consumed grant and the bindings file. Cross-person and cross-project reads returned 403 in both directions. A client-supplied `principal_id` returned 422. PostgreSQL client connections used `SslMode::Require`, and `pg_stat_ssl` reported `true` for the API runtime backend. The frozen API does not perform an HTTPS or OIDC login. The plan sentence that asks for verified HTTPS identity is outside this package. This review does not rewrite that plan sentence and does not shrink the card to the loopback grant.

## Stale writes do not move the head

After the turn, revision was 3 and owner generation was 1. A heartbeat transition with the expected revision lowered by one, and a second with the expected generation lowered by one, both came back acknowledged, refused, and stale. A following `get` showed the same revision, the same generation, and the same phase. The certification flag on those receipts was false.

## Failed attempts

Attempt 1 compiled the archive tests through live, then `a394_oracle` failed to compile: the SHA-256 state was an unconstrained integer, and the cancel closure moved `conversation`, the cookie, and the csrf token. Clippy exited 101 for the same compile errors. `docker-run.exit` was 1.

Attempt 2 fixed that compile. Lib, contract, lock, live, and the oracle exited 0. Clippy exited 101 because `Sample` stored `token_rows`, `stage`, `harness_pid`, and `workspace_exists` without reading them, and because five `serde_json::to_value` calls borrowed a `Copy` phase.

Attempt 3 removed those. The oracle exited 0 again. Clippy exited 101 because `Shot.checkpoint_pid` was stored and never read. That field was removed. Clippy was then run alone, without repeating the green product tests, and exited 0.

## Resources

Smoke and each proof container were one linux/arm64 Rust builder, 6 GiB, 1 CPU, CapDrop ALL, `HOME=/tmp`, private cargo and target volumes. One private PostgreSQL 17.10 stayed up for the oracle runs and was removed after the reports were written. See `cleanup.txt`. Foreign containers were not removed. `docker prune` was not run.
