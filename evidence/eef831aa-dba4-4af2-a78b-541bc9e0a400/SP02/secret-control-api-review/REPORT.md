# Independent review of the secret card

Session `eef831aa-dba4-4af2-a78b-541bc9e0a400`. Verdict for the frozen card only: APPROVE_LIMITED.

The reviewed tree is commit `bb831355662c1dffd76b86bf60417158f4513134` (`secret-control: record the secret write and rotation loop`). The live proof ran from a `git archive` of that commit for `tools/secret-control-api` and `tools/control-identity`. The working tree of those two directories matches that commit. This review did not edit them.

SP02-W04 is not accepted. The parent goal is not accepted. Customer KMS, mTLS CA issuance, a customer IdP, and foreign connection-pool refresh stay outside this verdict.

## What was proved

Contract `oo.secret-control/v1` in the frozen `tools/secret-control-api/CONTRACT.md` (AC-01 through AC-15). The service calls the `control-identity` binary over `GET /control/v1/identity/current`. It does not link that crate and it does not read `oo_identity_control`.

Resolve authority is the verified client-certificate SAN URI `urn:oo:workload:` plus the granted name. The independent test sent `x-oo-workload: urn:oo:workload:agent.one` on every resolve. The builder certificate still opened `secret://lab/alpha/builder/db` and the response workload was `urn:oo:workload:builder.one`. A body field named `workload`, and the raw `getIdentityCurrent` envelope posted as the resolve body, both returned `malformed`. `try_from_caller_json` on the live current envelope returned `unauthenticated`. An agent certificate on the builder secret returned `workload.rejected`. A foreign CA and a missing client certificate failed in the TLS handshake (`http` 0, `transport.rejected`). `POST /component/v1/resolve` on the management listener returned `not_found`.

Values are `ring-aes-256-gcm` in private PostgreSQL. Wrong namespace AAD, wrong version AAD, and a flipped master-key byte fail `open` on ciphertext read from that database. A repeated scripted nonce and a stale `expected_version` leave the previous version active. After `docker restart` of the same volume, the current ciphertext still opens and the retired and revoked rows stay refused.

## Toolchain

Smoke, before this review's overlay, is `linux/raw/smoke/exit.txt` = 0. Proof builder `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8, cargo 1.94.0. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, linux/arm64, in `pg/image.txt`. `linux/raw/cargo-update.txt` is `cargo-update-invoked=no`. No root manifest change. No dependency update.

Recipe origin commit `a587031c719a484830d78e493605f48e2056cae4`. `pg/pg-wrap.sh` is `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f`. `tools/sha256_manifest.rs` is `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. `pg/up.sh` keeps that origin's image digest, capability set, memory, CPU, private bridge, and secret-file entrypoint. Names are a400-private. See `linux/raw/recipe-origin.txt`.

## Raw exits

Write builder `456816afa9fd8c247ca03c5cabdd1df48389dee4e59ee48d154e22cc680d0677`. Restart builder `10a88c6f8224e66ccd4a5677f1eb2c7e72e50308efc1a8ab786426bdb6a88db2`. Both are the pinned Rust image, cap-drop ALL, no-new-privileges, privileged false, 6442450944 bytes (6 GiB), 1 CPU, network `oo-a400-eef831aa-net`.

| File | Exit |
| --- | --- |
| `linux/raw/rustc-version.exit` | 0 |
| `linux/raw/cargo-version.exit` | 0 |
| `linux/raw/cargo-build-identity.exit` | 0 |
| `linux/raw/cargo-clippy.exit` | 0 |
| `linux/raw/cargo-test-contract.exit` | 0 |
| `linux/raw/cargo-test-focused.exit` | 0 |
| `linux/raw/builder-write.exit` | 0 |
| `linux/raw/phase-marker-write.exit` | 0 |
| `linux/raw/cargo-test-restart.exit` | 0 |
| `linux/raw/builder-restart.exit` | 0 |
| `linux/raw/phase-marker-restart.exit` | 0 |
| `linux/raw/run-proof.exit` | 0 |
| `linux/raw/launch.exit` | 0 |

`linux/raw/phase.txt` is `live_secret=done`. `linux/raw/pg-restart.txt` is `restarted`. `linux/raw/witness-disposition.txt` is `witness_absent=true`. `linux/raw/launch.log` records attempt 01 `write_failed=1`, then `resume_write_builder`, `pg_restart`, `restart_builder`, and `resume_ok`.

Clippy: `cargo clippy --locked --offline --all-targets -- -D warnings`. Contract then independent ran one after the other with `--test-threads=1`. The author `tests/live` binary was not the oracle, and `OO_A384_LIVE` was not set. Contract tests passed: `aad_changes_when_namespace_changes`, `caller_json_is_not_authority`, `kms_refuses_before_opening_a_key_file`, `migration_has_no_plaintext_column_and_defers_the_head_key`, `nist_aes_256_gcm_empty_plaintext_matches_ring` (5 passed). `independent_secret_card` passed in the write phase (`live_secret=write_done`, 1.48s) and after the PostgreSQL restart (`live_secret=done`, 0.14s).

`linux/raw/phase-marker-write.log` still contains the attempt 01 line `write marker missing`. The success path does not truncate that log. The exit file is 0, and the write-phase test stdout contains `live_secret=write_done`.

At both resume capacity gates, `own_rust=0`, `global_rust=1`, `host_cargo=0`, `own_pg=1`. The other Rust builder belonged to another session. This task ran one builder at a time.

## HTTP and ciphertext

`linux/raw/review-proof.json` has 27 cases. `fixed_commit` is `bb831355662c1dffd76b86bf60417158f4513134`. `customer_kms` is `external_gate`. `mtls_ca` is `not_issued`. `mtls_ca_issued_by_service` is false. `customer_idp` is `not_claimed`. `foreign_pool_refresh` is `not_claimed`. `resolve_authority` is `verified_client_certificate`.

| Case | HTTP | Code |
| --- | --- | --- |
| create | 201 | ok |
| idempotent_replay | 200 | ok |
| idempotency_conflict | 409 | idempotency.conflict |
| write_existing | 409 | version.conflict |
| status | 200 | ok |
| unauthenticated | 401 | unauthenticated |
| csrf | 403 | csrf.rejected |
| origin | 403 | origin.rejected |
| cross_project | 403 | forbidden |
| query | 400 | malformed |
| resolve_cert_ignores_caller_header | 200 | ok |
| resolve_caller_json_workload | 400 | malformed |
| resolve_caller_json_envelope | 400 | malformed |
| resolve_other_cert | 403 | workload.rejected |
| resolve_foreign_ca | 0 | transport.rejected |
| resolve_no_client_cert | 0 | transport.rejected |
| namespace_mismatch | 403 | namespace.rejected |
| cross_namespace | 403 | workload.rejected |
| management_cannot_resolve | 404 | not_found |
| stale_rotate | 409 | version.conflict |
| rotate_workload | 403 | workload.rejected |
| rotate | 200 | ok |
| stale_after_rotate | 409 | version.conflict |
| retired | 409 | version.retired |
| aad_and_key | 0 | aead.rejected |
| revoke | 409 | revoked |
| fixture-reviewer | 403 | forbidden |

`aad_and_key` is a local `open` of a PostgreSQL ciphertext row. It is not an HTTP response. The correct AAD opens. A namespace change, a version change, and a flipped key byte fail.

`linux/raw/run.json` names `secret://lab/alpha/builder/db` at version 2, previous version 1, revoked ref `secret://lab/alpha/builder/gone`, project `alpha`, namespace `builder`, workload `urn:oo:workload:builder.one`, deployment `lab`. It has no `value`.

`linux/raw/restart-proof.json`: current HTTP 200, version 2, retired `version.retired`, revoked `revoked`, `value_matched_witness` true, `spoof_header_ignored` true.

`linux/raw/pg-after-restart.txt` was read from the same volume after that restart. Ciphertext bytes were not selected. Six version rows, six distinct nonces, cipher `ring-aes-256-gcm`, nonce length 12. `builder/db` version 1 is retired and version 2 is active. `builder/gone` is revoked. `builder/half` is version 2 active and version 1 retired, two rows. `agent/db` is version 1 active for `urn:oo:workload:agent.one`. Head `allowed_workload` matches those grants.

The scripted half-write is inside `independent_secret_card`. A repeated 12-byte nonce returns `version.conflict` and leaves head version 1 with one row. The next nonce commits version 2 with two rows. The post-restart catalog matches that outcome. The `secret-control` binary path then proved stale `expected_version` 9 and a second stale `expected_version` 1 after the successful rotate. Counts stayed (1, 1) and then (2, 2).

## Acceptance map

- AC-01. Contract test `caller_json_is_not_authority`. Live `try_from_caller_json` on the current envelope is `unauthenticated`. Missing cookie is 401 `unauthenticated`. Resolve cases above refuse caller JSON and a spoof header.
- AC-02. The admin session completed create, status, rotate, and revoke. `fixture-reviewer` and `cross_project` are 403 `forbidden`. This run did not submit steward, auditor, or business_owner.
- AC-03. NIST empty-plaintext known answer passed. Management responses in the live cases have no `value`. Resolve is the component path.
- AC-04. `kms_refuses_before_opening_a_key_file` passed. The live `key_source=kms` install exited 2 with `kms.external` and left the named key path absent. The file master key mode checked in the test is 0600. That file is under the host state directory and is not in Git.
- AC-05. Stored nonces are 12 bytes and the six rows have six distinct nonces. The scripted collision is the library `ScriptedNonce` path against this PostgreSQL. The binary path was not instrumented to trace `getrandom`.
- AC-06. Contract test `aad_changes_when_namespace_changes`. Live `aad_and_key` fails closed on wrong namespace AAD, wrong version AAD, and a wrong key. HTTP `aead.rejected` was not issued by a route in this run.
- AC-07. `resolve_cert_ignores_caller_header` is 200 with the certificate workload. Status has no value. Query strings are `malformed`. Management resolve is 404.
- AC-08. Body namespace `agent` on the builder secret is `namespace.rejected`. The builder certificate on the agent secret, and the agent certificate on the builder secret, are `workload.rejected`. The agent certificate opens only the agent value. That last success is an assertion in the passing test.
- AC-09. `stale_rotate` and `stale_after_rotate` are `version.conflict`, and the half-write collision does not add a row. `rotate_workload` does not change the head. This run did not repeat a parallel rotate.
- AC-10. `revoke` is 409 `revoked` on a later resolve. The head row stays revoked after the PostgreSQL restart.
- AC-11. Pinned version 1 is 409 `version.retired` and the response has no value. The same refusal holds after the restart.
- AC-12. `docker restart` of `oo-a400-pg` kept the volume. `cargo-test-restart.exit` is 0. `phase.txt` is `live_secret=done`. The restart resolve matched the witness bytes.
- AC-13. The witness scan found no copy in the raw logs. `witness_absent=true`. `review-proof.json` and `run.json` have no secret value. The witness file was overwritten and removed before this report.
- AC-14. Recorded gates are `customer_kms=external_gate` and `mtls_ca=not_issued`. The test minted its own client CA with openssl. The service did not issue it.
- AC-15. This review does not accept SP02-W04 or a parent goal.

## Open gates

Customer KMS unwrap, mTLS CA issuance, a customer IdP, foreign connection-pool refresh, and Gateway or Valkey namespace storage stay open. The test issuer is the frozen fixture copied as `overlay/independent/idp.rs` (`29a4b070f4c5f76f4417fcb8f9b9f94216826f8591edaf6b01413b2f2fe4fa8a`). The ledger was not edited.

## Failed attempts

`linux/raw/failed-attempts/01-typecheck/` is the only failed resume of this review. Cargo clippy exited 101 on five type errors in the review overlay. No HTTP call ran. The product tree was not edited. The green resume reused the same private database volume and cargo volume.
