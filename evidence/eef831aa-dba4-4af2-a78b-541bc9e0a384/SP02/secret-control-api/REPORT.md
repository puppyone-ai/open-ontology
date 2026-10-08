# Secret write and rotation

Session `eef831aa-dba4-4af2-a78b-541bc9e0a384`. Card scope is the secret write, resolve, rotate, and revoke loop. Verdict: author delivery of `tools/secret-control-api` and this evidence packet. SP02-W04 is not accepted. The parent goal is not accepted. This packet is not an independent review.

The live proof that this report cites is the existing same-PostgreSQL resume. Builder `oo-a384-proof` had already exited. It was not started again. PostgreSQL `oo-a384-pg` was still up when these exits were read, and owned cleanup removes it after this report.

## Frozen contract

`tools/secret-control-api/CONTRACT.md` freezes AC-01 through AC-15, the OpenAPI operations, and the error table. The service binds write, rotate, revoke, and status to a live `project.admin` grant from `GET /control/v1/identity/current` on the real `control-identity` binary. It does not link that crate and it does not read `oo_identity_control`. Values are sealed with root-lock `ring` 0.17.14 `AES_256_GCM`. `key_source=kms` exits 2 with `kms.external` before the key file is opened. Responses carry `customer_kms=external_gate` and `mtls_ca=not_issued`.

## Toolchain

Smoke, before product edits, is `linux/raw/smoke/exit.txt` = 0. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, linux/arm64, recorded in `pg/image.txt`. `linux/raw/cargo-update.txt` is `cargo-update-invoked=no`. No root manifest change.

Recipe origin commit `a587031c719a484830d78e493605f48e2056cae4`. `pg/pg-wrap.sh` matches `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f`. `tools/sha256_manifest.rs` matches `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. `pg/up.sh` keeps that origin's image digest, capability set, memory, CPU, private bridge, and secret-file entrypoint. Names are a384-private. See `linux/raw/recipe-origin.txt`.

## Raw exits

Write builder `8b3a876b3717f0c3b4a4b088a61e616816f5e8dc69c3457cb56eb43591301257`. Restart builder `d1f96965fc2339f697eadc2f60adb085edf5db61db8a45a04b65eebde46d454c`. Both are the pinned Rust image, cap-drop ALL, no-new-privileges, 6 GiB, 1 CPU.

| File | Exit |
| --- | --- |
| `linux/raw/rustc-version.exit` | 0 |
| `linux/raw/cargo-build-identity.exit` | 0 |
| `linux/raw/cargo-clippy.exit` | 0 |
| `linux/raw/cargo-test-focused.exit` | 0 |
| `linux/raw/builder-write.exit` | 0 |
| `linux/raw/cargo-test-restart.exit` | 0 |
| `linux/raw/builder-restart.exit` | 0 |
| `linux/raw/phase-marker.exit` | 0 |
| `linux/raw/run-proof.exit` | 0 |
| `linux/raw/launch.exit` | 0 |

`linux/raw/phase.txt` is `live_secret=done`. `linux/raw/pg-restart.txt` is `restarted`. `linux/raw/witness-disposition.txt` is `witness_absent=true`. `linux/raw/launch.log` ends at `launch_ok` after `pg_restart` and `restart_builder`. Earlier `write_failed=1` lines in that log are attempts 01 through 12 and stay in the log.

Clippy command: `cargo clippy --locked --offline --all-targets -- -D warnings`. Focused test: `cargo test --locked --offline -- --test-threads=1 --nocapture`. Contract tests passed: `aad_changes_when_namespace_changes`, `caller_json_is_not_authority`, `kms_refuses_before_opening_a_key_file`, `migration_has_no_plaintext_column_and_defers_the_head_key`, `nist_aes_256_gcm_empty_plaintext_matches_ring` (5 passed). `live_secret_loop` passed in the write phase (`live_secret=write_done`, 1.26s) and again after the PostgreSQL restart (`live_secret=done`, 0.13s).

## HTTP and ciphertext

`linux/raw/author-proof.json` has 23 cases. `customer_kms` is `external_gate`. `mtls_ca` is `not_issued`.

| Case | HTTP | Code |
| --- | --- | --- |
| create | 201 | ok |
| idempotent_replay | 200 | ok |
| idempotency_conflict | 409 | idempotency.conflict |
| write_existing | 409 | version.conflict |
| status | 200 | ok |
| rotate | 200 | ok |
| stale_rotate | 409 | version.conflict |
| resolve | 200 | ok |
| retired | 409 | version.retired |
| cross_namespace | 403 | workload.rejected |
| scope_mismatch | 403 | forbidden |
| unauthenticated | 401 | unauthenticated |
| csrf | 403 | csrf.rejected |
| origin | 403 | origin.rejected |
| revision | 409 | revision.conflict |
| cross_project | 403 | forbidden |
| query | 400 | malformed |
| rotate_workload | 403 | workload.rejected |
| parallel_rotate | 200 and csrf.rejected | one winner |
| revoke | 409 | revoked |
| fixture-reviewer | 403 | forbidden |
| fixture-owner | 403 | forbidden |
| aad_and_key | 403 | aead.rejected |

`linux/raw/run.json` keys are `deployment`, `namespace`, `previous_version`, `project_id`, `secret_ref`, `version`, `workload`. `secret_ref` is `secret://lab/alpha/builder/db`, version 2, previous version 1, project `alpha`, namespace `builder`, workload `urn:oo:workload:builder.one`, deployment `lab`. The object has no `value`.

The write-phase test also drives a scripted-nonce half write before the `secret-control` binary path. A repeated 12-byte nonce returns `version.conflict`, the head stays at version 1 with one row, and the next distinct nonce commits version 2 with two rows. That assertion is inside `live_secret_loop`. It is not a separate JSON case. The restart phase reads the same volume, opens the current ciphertext, and refuses the retired version.

## Acceptance map

- AC-01. Contract test `caller_json_is_not_authority`. Live calls use the identity binary. Missing cookie is `unauthenticated` 401.
- AC-02. `fixture-reviewer`, `fixture-owner`, and `cross_project` are `forbidden` 403. The admin session completed create, status, rotate, and revoke.
- AC-03. NIST empty-plaintext known answer passed. Management `run.json` and status carry metadata. Resolve is the component path.
- AC-04. `kms_refuses_before_opening_a_key_file` passed. The master key file is mode 0600 under the host state directory and is not in Git.
- AC-05. The `secret-control` binary takes nonces from the OS. The scripted collision hits `secret_version_nonce_unique` and does not activate the new version.
- AC-06. Contract test `aad_changes_when_namespace_changes`. Live case `aad_and_key` is `aead.rejected` 403 for a changed namespace AAD and a changed 32-byte key on ciphertext read from PostgreSQL.
- AC-07. `resolve` 200 on the verified workload certificate. Status has no value. Query strings are `malformed` 400.
- AC-08. `cross_namespace` is `workload.rejected` 403. `scope_mismatch` is `forbidden` 403.
- AC-09. `stale_rotate` is `version.conflict` 409. The nonce collision leaves version 1 active. `parallel_rotate` records one 200 and one `csrf.rejected`, and the version count stays (2, 2).
- AC-10. `revoke` is 409 `revoked` on a later resolve.
- AC-11. `retired` is 409 `version.retired`.
- AC-12. After `docker restart` of `oo-a384-pg`, `cargo-test-restart.exit` is 0 and `phase.txt` is `live_secret=done`.
- AC-13. Evidence scan found no private-key blocks and no 32-byte `a` run. `run.json` has no resolve field. Attempt 07 stores `bootstrap_token=REDACTED`. The witness file was overwritten and `witness_absent=true`.
- AC-14. Every recorded success object used by the test asserts `customer_kms=external_gate`. The proof object sets `mtls_ca=not_issued`.
- AC-15. This report does not accept SP02-W04, a parent goal, or an independent review.

## Open gates

Customer KMS unwrap, mTLS CA issuance, a customer IdP, foreign connection-pool refresh, and Gateway or Valkey namespace storage stay open. The test issuer is a fixture. Frontend B12 operations pages are out of scope. The ledger was not edited.

## Failed attempts

Archives under `linux/raw/failed-attempts/` stay in the packet.

1. `01-compile`. Library type errors in `ids.rs` and `tlsutil`.
2. `02-clippy-result-unit-err`. Clippy on a `Result` unit error.
3. `03-clippy-config-debug`. `Config` must not implement `Debug` because it holds the master key.
4. `04-live-compile`. Live test type errors. Library compiled.
5. `05-live-move`. `absent` moved a `String` that was used again.
6. `06-clippy-collapsible-if`. Two `collapsible_if` errors. Tests skipped.
7. `07-login-refused`. Clippy 0, contract 5 passed. Login ran before the identity listener was ready. Bootstrap token redacted.
8. `08-half-create-403`. `GET current` rotates the CSRF hash, so the following mutation was `csrf.rejected`.
9. `09-half-create-503`. CSRF probe reached identity (`bootstrap.rejected`). Secret version count stayed 0.
10. `10-confirm-ok-503`. Confirm classified `bootstrap.rejected`. The create still returned 503 and committed no row.
11. `11-pg-client`. Driver error with no SQLSTATE, before PostgreSQL reported an error.
12. `12-jsonb-parameter`. Idempotency insert described a `String` parameter as `jsonb`. The statement is now `($4::text)::jsonb`.

The green resume is the `linux/raw` exit set above. It is not archived as a thirteenth failure. Debug prints were removed before that run. `rg eprintln` on `tools/secret-control-api` finds no matches.
