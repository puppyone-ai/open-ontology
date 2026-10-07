# Report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a334`. Owner a334. Fixed archive `63566b60aabdf29a29bfe55c4866edf3c4081cfd`. This is the integration repair of the closed a324 local relay. It is not a new countable acceptance unit. It does not close a parent, the full plan, or a release.

## Result

One `grok-4.7` chat completion succeeded. Committed `ontology-dialogue propose` returned a proposal. An operator approval file was written. `query-pg` was spawned. The guard refused the model statement, exit 6. No audit file was written. `query_id` is null. `rows` is null. `citation` is null. The model SQL was not executed. That is a withhold, not a query receipt.

The proposal SQL, unchanged, is:

```text
SELECT COUNT(headcount) AS visible_headcount_values FROM depot.counter;
```

`auto_execute` is false. `model_sql` is `untrusted`. `authenticated` is false.

## What was collected

At collection the host relay was still listening on `127.0.0.1:18317` (pid 97342). Postgres `oo-a334-pg` was up. No a334 Rust builder was running. Those processes were the ones already started for this task. No second relay, builder, or database was created for this report.

| Step | Evidence | Exit |
|---|---|---|
| Linux pre-edit smoke | `linux/preoverlay` | docker 0, test 0 |
| Linux tests and Clippy after the relay edit | `linux/proof-1` | docker 0, test 0, clippy 0, 27 passed |
| Linux tests and Clippy after the fixture-literal redaction | `linux/proof-2` | docker 0, test 0, clippy 0, 27 passed |
| Host Mach-O build | `host/build.exit` 0, `host/fetch.exit` 0, `host/offline-build.exit` 101 | 101 is the empty `CARGO_HOME` probe |
| Models GET through the relay | `host/models.json` | HTTP 200, `required_model` `grok-4.7`, `model_available` true, `bearer_bytes` 44 |
| First chain, no chat sent | `linux/chain` | docker 3, propose 3 |
| One dialogue propose | `linux/chain-2` | docker 0, propose 0 |
| Live fixture plus one execute | `linux/execute` | live test 0, `live_done=1`, execute 6 |

The models listing is not the completion. The completion is `linux/chain-2/proposal.json`.

## 426 diagnosis

One bounded diagnosis, recorded in `diag/426-meta.json`, used model `grok-4.3`, `stream` false, and a JSON schema. The upstream status was HTTP 426, `content-type` `application/json`, 150 bytes. The JSON `error` string says Grok CLI `0.2.120` is older than `1.0.13`. The body did not contain the local key. There was no second diagnosis and no retry.

The relay does not rewrite `model` and does not turn a non-200 into success. The later propose kept `model_id` `grok-4.7` and `stream` false. For that one call, two local xAI auth files were given the public headers `x-grok-client-version: 1.0.44` and `User-Agent: xai-grok-workspace/1.0.44`, because the proxy applies those custom headers after its built-in `0.2.120` pin. Token-bearing backups stayed under the private tmp directory. Cleanup restored the two files from that backup and then removed the backup.

## Propose

`linux/chain` preserved the failed attempt. `/tmp` in that container is `noexec`, so `/tmp/emit-pack` was not executed. `ontology-dialogue` then exited 3 with stderr `ontology-dialogue: config`. No chat request was sent. `linux/chain/console.log` and `docker-run.exit` 3 stay as they are.

`linux/chain-2` compiled the pack helper onto the executable target volume and left the readonly toolchain file in place. Rust `1.94.0` fetched and built `ontology-dialogue` from the fixed archive with `--locked`. One propose used origin `https://host.docker.internal:18317`, path `/v1/chat/completions`, model `grok-4.7`, the public relay CA, and the 44-byte client-token file. Propose exit is 0. The proposal protocol is `oo.ontology-dialogue.proposal/v1`. The client token is not in the copied proposal, pack, or config.

A capacity deferral before that retry is preserved under `linux/chain-2/defer-1`. The first gate counted another task's host `cargo` as this task's builder. The retry counted this task's own compiler and the global builder total. Global was under 10 and this task had no builder, so the container started.

## Execute withhold

`linux/execute` built `query-pg`, `install-column-acl`, `provision-mapping-reader`, `pg-onboarding`, and `answer-citations` from the fixed archive, offline after `--locked` fetch. The reviewed live test `OO_A300_LIVE=1` prepared database `oo_a300` on `oo-a334-pg` (PostgreSQL `17.10`). `live/oracle.txt` records `north_exit=0`, `north_rows=3`, `count_exit=0`, `count_receipt=clean`, and `live_done=1`. The live count statement is the fixture's `SELECT count(*) FROM depot.counter`. It is not the model statement and it is not this proposal's receipt.

The approval file names the proposal hash, SQL hash, tuple hash, and pack digest from the proposal. `authenticated` is false. `trust` is `local_operator_declared`.

`ontology-dialogue execute` exit is 6. Stderr is `ontology-dialogue: guard`. Stdout is `oo.ontology-dialogue.execution/v1` with `spawned` true, `child_exit` 6, `child_token` `guard`, `query_receipt` `absent`, `query_id` null, `summary` null, `rows` null, `rendered` null, and `citation` null. `audit-absent.txt` records that no audit file was written. Exit 6 does not admit the statement, so the user SQL did not run.

The fixed catalog loader resolves only `pg_catalog.count(*)`. `COUNT(headcount)` is not that aggregate-star call, so the guard refuses it. The model text was not rewritten and was not sent again.

## Relay contract

Contract `oo.local-llm-relay/v1` plus `oo.local-llm-relay.client-token/v1` is in `testing/real-db/local-llm/CONTRACT.md`. The client token is 32 to 256 visible ASCII bytes, minted as 32 random bytes and standard base64 (44 characters), mode `0600`. Dialogue's bearer minimum stays 16. The relay checks the client token on the downstream `Authorization` header and substitutes the upstream token only on the upstream `Authorization` header. Listen stays `127.0.0.1`. The upstream stays `http://127.0.0.1:8317`. The allowlist stays `GET /v1/models` and `POST /v1/chat/completions`. A non-200 upstream status is forwarded. An event stream becomes one JSON envelope only when finish reason and usage are both present and the body ended on an event boundary. A cut stream is HTTP 502.

The public test literal that collided with the 6-byte local key was replaced with `synthetic-client-token-abcdefghijklmn` before commit. A scan of the crate and this evidence tree found no remaining key bytes. The key file `/tmp/oo-customer-demo-20261007/llm-token` is still mode `0600` and 6 bytes. It was not mounted into a container and was not deleted.

## Builders and cleanup

Each Linux builder was 6 GiB, 1 CPU, `--cap-drop ALL`, no privilege, no host network, no Docker socket, and no host home. The execute container used pid limit 256, matching the a300 proof-8 recipe. Postgres had no host port. Cleanup exit 0 removed the relay process, `oo-a334-pg`, the four a334 volumes, the a334 network, and `/tmp/oo-a334-eef831aa-dba4-4af2-a78b-541bc9e0a334`. No foreign container was removed and nothing was pruned.

## Follow-up

The historical `COUNT(headcount)` proposal remains a failed query. This follow-up did not rewrite that SQL and did not run it again. It did not read, write, or search auth files, backups, or credential-bearing config.

Contract addition `oo.local-llm-relay.public-client-version/v1` was written before the relay change. The relay now sets `x-grok-client-version: 1.0.44` and `User-Agent: xai-grok-workspace/1.0.44` on each upstream request. Caller values for those headers are not forwarded. `linux/proof-2` is the pre-edit smoke: its source hash matched the crate before this edit. `linux/proof-3` is the post-edit run: Rust `1.94.0`, 28 tests passed, Clippy exit 0, lock subset clean, docker exit 0. `linux/proof-3/attempt-1` is a Docker bind failure for `/tmp` (exit 125). The retry used `/var/tmp`.

`host-2` built the Mach-O relay after a transient crates.io HTTP/2 failure preserved in `host-2/attempt-1` (fetch exit 101). A first listen failed with the static token `tls` under LibreSSL (`host-2/attempt-tls`). The retry used OpenSSL 3.6.4. Models GET was HTTP 200 with `grok-4.7` available. That listing is not a completion.

One new question, `What is the stored row count of depot.counter?`, and a new pack that names `SELECT count(*) FROM depot.counter` and `SELECT headcount FROM depot.counter`, were sent once through `linux/chain-3`. Dialogue stderr is `ontology-dialogue: provider`. No proposal file was written. The proxy log for that request is HTTP 426, 1.435s, `POST /v1/chat/completions`, base URL `https://cli-chat-proxy.grok.com/v1`. The response body was not retained. There was no retry and no second model. `query-pg` was not started. `linux/execute-2` was not launched. Postgres `17.10` did start for the query that never ran; `pg-2/up.exit` is 0. `pg-2/attempt-1` records that LibreSSL rejected `x509 -ext` before the container existed.

This follow-up does not produce a query receipt. Parent acceptance, a full-goal close, and a release are out of scope.

## Readiness 2026-10-08

The repaired endpoint accepted one new `grok-4.7` completion. The question was `List each visible headcount in depot.counter from smallest to largest.` Its question document sha256 is `35d7471e3b9598d16de0f07aa99079a1e4f1719527a9eaac35f33b01cade8f01`. `linux/chain-4/phases/propose.exit` is 0. The proposal SQL, unchanged, is:

```text
SELECT headcount FROM depot.counter ORDER BY headcount
```

`sql_sha256` is `beab58df6cd5599039e8b70e4f612fc12923b300479ee2563f021c1c338c4957` (54 bytes). `proposal_sha256` is `6c50244e456458c035fe4b3fd9dbca59f4dc279bae9778436b6f163184c5105b`. `pack_digest` is `29ba57598c850e6321945a0a4335b7df1416178d40fd7f13c5c0d0f40ceddbe2`. `auto_execute` is false. `model_sql` is `untrusted`. The propose config `model_id` is `grok-4.7`.

That statement is the supported shape `SELECT headcount FROM depot.counter ORDER BY headcount`. After the proposal was on disk, the operator wrote `linux/chain-4/approval.json` for those four hashes. `linux/chain-4/operator-decision.txt` records `rewritten=false` and `automatic=false`.

`linux/execute-3/phases/execute.exit` is 0. `execute.stdout` has `query_receipt` `clean`, `child_exit` 0, `query_id` `q_3782300da36af286`, `returned_rows` 3, `truncated` false, and typed rows `int` 1, `int` 2, `int` 3. The citation status is `ok`, session `sess-a334c`, turn `turn-a334c`, and the admitted citation uses that query id. `original_sha256` equals the proposal SQL hash. `executed_sha256` `ed6e42d0833eb27ef9fde69b26604c4c552d9efef9164708188c676937ec9b82` is the fixed guard's deparse hash. The request file still carries the model SQL text. The audit file is `linux/execute-3/audit.jsonl`.

The crate content hash equals `linux/proof-3/source-after.sha256` (`de2ab0ad2b5ae483880f44a9a37005e6f0f6523a87d5afc143a6fa96f629ca77`). That run is the Linux Rust `1.94.0` evidence for this tree: test exit 0, Clippy exit 0, docker exit 0. No relay source changed, so no new Linux builder ran. `readiness/source-note.txt` records the comparison.

`host-3/offline-build.exit` is 101 on the empty private cargo home. `host-3/fetch.exit` is 0 and `host-3/build.exit` is 0. Host `rustc` is 1.98.1. Smoke exit is 0. Models GET is HTTP 200 with `grok-4.7` available and `bearer_bytes` 44. That listing is not the completion. `pg-3/up.exit` is 0, server version `17.10 (Debian 17.10-1.pgdg12+1)`, SAN `DNS:oo-a334-pg`, no host port.

`linux/execute-3/phases/test-live.exit` is 0 and `live/oracle.txt` ends with `live_done=1`. That suite's `count_receipt=clean` is the fixture `count(*)` case. Its north case uses the same SQL text, so the fingerprint matches `3782300da36af286`. The model receipt is `execute.stdout`. The suite also records `http_execute_called=false`, `write_exit=6`, and `south_exit=6`.

The historical `COUNT(headcount)` guard refusal and the chain-3 HTTP 426 withhold stay in place. Auth files, backups, and credential-bearing config were not read or written. `CLEANUP-readiness.txt` records relay pid 94158 stopped, `oo-a334-pg` removed, the cargo, dialogue-target, and pgdata volumes removed, the task network removed, and the private `/var/tmp` tree removed. The token file remains mode `0600`, 6 bytes. `auth_store_touched=false`. Foreign Postgres `oo-a341-pg` and the proxy on `127.0.0.1:8317` were left running.

Parent acceptance, a full-goal close, and a release are out of scope.
