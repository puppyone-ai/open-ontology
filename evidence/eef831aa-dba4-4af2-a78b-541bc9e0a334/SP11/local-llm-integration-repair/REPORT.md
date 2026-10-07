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
