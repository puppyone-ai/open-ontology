# Report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a394`. Independent review of the a387 session-control-api package. The product tree was not edited. This file does not accept SP11, the parent goal, or a production signature.

Reviewed commit `53017ed29c98c49fb44a7eb18d5df6d5de273046`. The archive is `archive/tree`, taken from that commit for `tools/session-control-api`, `tools/session-store`, `tools/session-transition`, `tools/session-release`, `tools/gateway-token-store`, and `Cargo.lock`. `archive/inventory-check.txt` matches the author's 18 crate files, 18 of 18. Smoke on linux/arm64 Rust 1.94 ran before the oracle file existed: `linux/smoke/docker-run.exit` is 0, `linux/smoke/version.txt` is `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `linux/smoke/hash-check.exit` is 0.

## What was executed

One private PostgreSQL 17.10, database `oo_a394`, TLS required, 1 GiB, 1 CPU, no host port. The API under test listened HTTP/1.1 on `127.0.0.1` with an ephemeral port. `pg_stat_ssl` for the observer backend was `true`. Profile `production` exited 2 with `runtime_unavailable` and wrote no port. `runsc` is absent in the pinned image, so the production driver path was not injected.

Identity was the operator bindings file plus a one-time grant. `GET /control/v1/entry/session/current` returned principal `alice` and project `proj-a`. A second consume of the same grant returned 409. A create body that sent `principal_id` returned 422.

The oracle then created an alice conversation, stored a turn whose event body is `a394-transcript-marker`, and cancelled it. Before the turn, `oo_session_api.proxy_mapping` was `present` with `map_ip` `10.88.0.8`, `oo_session_api.permit` was `held`, `oo_token_control.token_record` had 1 row, `oo_token_control.revocation` had 0 rows, `backend_pid` was null, and `harness_pid` was null. An early cancel returned 409 `backend_absent` and left those credentials in place. After the turn, `pg_stat_activity` showed one active backend for `q{session}`.

During cancel, 157 samples on the first green oracle and 171 samples on the second saw a window where the revocation row count was already 1, the mapping state was `deleted`, and the permit state was `released`, while the backend count was still positive or the release-spawned child was still alive. The live child cmdline contained `/bin/bash` and `sleep 6` and was classified `release_fixture_bash`. No harness pid existed before cancel.

After HTTP 200 `closed`, the receipt had 11 steps, S5 `skipped` with `actual_absence` false, S8 `confirmed` with `actual_absence` true, the other steps `confirmed` without absence, `production_signed` false, `deployment` `localprocess`, `retryable` false, and `reducer_physical_release_certified` false. SQL showed revocation rows 1, token rows still 1, mapping `deleted`, permit `released`, present mappings 0, held permits 0, backend count 0, workspace directory gone, snapshot absent, token file gone, the used pool slot `destroyed`, and a replacement empty slot. `session_store::get` reported phase `closed` and `physical_release_certified` false. S3 and S4 evidence hashes matched the recomputed collect hash of the output-object sha256 and length. S6, S7, and S10 matched the constant labels `gateway_token_store/revoked`, `proxy_mapping/present_count_0`, and `permit/held_0`. The transcript GET still returned the marker. A second cancel returned 200 `replay` and the revocation count stayed 1.

`fixture_stage` on a second conversation returned `fixture_persisted`, phase `releasing`, retryable, and `acceptable()` false. That receipt is recorded and is not a close. The server process was then killed and a new process was started. The pids differed. HTTP cancel on the new process returned 200 `closed`, and the store head was phase `closed` with the certification flag false.

A third conversation planted `snapshot.bin` under the owner snapshot directory. HTTP cancel returned 409 `snapshot_present_uncollected` and the store phase stayed unclosed. Deleting the file and cancelling again returned 200 `closed`.

Negatives on the same database: bob reading alice's conversation returned 403, dana reading it returned 403, alice reading dana's conversation returned 403, and the lists did not contain the other conversation. `session_store::transition` with revision minus one, and again with generation minus one, both returned acknowledged, refused, stale. The head stayed at revision 3 and owner generation 1.

The same oracle passed a second time after dead-field and needless-borrow edits, with the same resource facts. `linux/proof/lint-run/test-oracle.exit` is 0 and `linux/proof/lint-run/oracle-report.json` is that run. Clippy on the following source, which only drops the unread `Shot.checkpoint_pid` field, exited 0 in `linux/proof/clippy-final`. That field was not part of an assertion or of the report JSON. The product tests were not run again after they had passed.

## Commands and exits

Author tests and the first oracle, `linux/proof`:

| check | exit |
| --- | --- |
| fetch | 0 |
| test lib and bins | 0 |
| test contract | 0 |
| test lock_subset | 0 |
| test live | 0 |
| test a394_oracle | 0 |
| clippy | 101 |
| docker run | 1 |

Lib tests: `releasing_and_unproven_absence_are_not_closed` and `sha256::tests::known_answers`. Contract: 3 passed. Lock: 1 passed. Live: `localprocess_s1_through_s11_closes_and_restarts` passed in 29.29s. First oracle passed in 29.91s. Second oracle passed in 30.33s. Final clippy `docker-run.exit` is 0.

Host config for the builders: CapDrop ALL, not privileged, network `oo-a394-eef831aa-net`, memory 6442450944, NanoCpus 1000000000, not OOM-killed. The proof recipe parent is a387 `linux/impl/launch.sh` sha256 `c522fe7bd898d640bbf9d9b4047d2bec74bb18c81e3c04fd7c75d1448bf8b88e` and `inside.sh` sha256 `c88f4b4d71fa919b81555a8d54678640cb2a92c5496770fb2b7ed8ce38e0abe3`. The PostgreSQL wrapper sha256 `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f` matches the parent byte for byte. Image pins are the rust and postgres digests named in the review prompt. No `cargo update`, no `generate-lockfile`, and no root manifest edit.

## Verdict

The localprocess close wrote a PostgreSQL revocation row, updated the owned mapping and permit rows, cancelled the `pg_sleep` backend, removed the workspace, and reached phase `closed` from a new server process. The certification flag stayed false. Production remained unsigned. `fixture_persisted` stayed a releasing receipt. This review records those observations and does not accept SP11, the parent goal, or a production signature. The original card is left at its full scope.

Raw exits are under `linux/smoke`, `linux/proof`, `linux/proof/failed-attempts`, `linux/proof/lint-run`, `linux/proof/clippy-final`, and `pg`. The oracle JSON reports are `linux/proof/oracle-report.json` and `linux/proof/lint-run/oracle-report.json`.
