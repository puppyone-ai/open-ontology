# Findings

Independent review for session `eef831aa-dba4-4af2-a78b-541bc9e0a400`. Frozen commit `bb831355662c1dffd76b86bf60417158f4513134`. These notes judge that card. They do not accept SP02-W04 or a parent goal.

## F1. Resolve identity is the verified client certificate

The component listener requires a client certificate checked against the operator-supplied CA. Workload comes from the leaf SAN URI. The independent client attached `x-oo-workload: urn:oo:workload:agent.one` to every resolve, including the successful builder resolve. The HTTP 200 body `workload` was `urn:oo:workload:builder.one`, which is the certificate URI, and the opened bytes matched the value stored for that grant.

An extra JSON field `workload` on the resolve body returned 400 `malformed` and no value. Posting the live `getIdentityCurrent` envelope as the resolve body returned 400 `malformed`. The agent certificate against the builder secret returned 403 `workload.rejected`. A certificate from another CA, and a client with no certificate, failed before an HTTP status. The management listener has no client authentication and its `POST /component/v1/resolve` returned 404 `not_found`.

After the PostgreSQL restart, the same builder certificate opened version 2, `spoof_header_ignored` is true, and the head column `allowed_workload` is `urn:oo:workload:builder.one`.

The admin write body still carries a `workload` string. That string is the grant stored for `project.admin`. Resolve accepts it only when it equals the verified certificate URI.

## F2. Caller JSON is not an identity constructor

`try_from_caller_json` on the live current envelope returned `unauthenticated` for the admin session and again for `fixture-reviewer`. Contract test `caller_json_is_not_authority` passed. Mutations in the green run used the cookie from the real `control-identity` binary, `Origin` equal to the configured origin, and `X-OO-CSRF` taken from a fresh `GET /control/v1/identity/current`. A missing cookie is 401 `unauthenticated`. A wrong CSRF is 403 `csrf.rejected`. An `Origin` of `https://evil.example` is 403 `origin.rejected`.

## F3. A failed rotation does not activate a partial version

Library `ScriptedNonce` pushed `[1;12]`, `[1;12]`, then `[2;12]` into a server bound to this private database. The second rotate returned `version.conflict`. The following nonce committed version 2. The catalog after the restart shows `builder/half` version 1 retired, version 2 active, and no third row.

On the `secret-control` binary, `expected_version` 9 returned `version.conflict` while the head stayed at version 1 with one row. A rotate whose workload URI was `urn:oo:workload:agent.one` returned `workload.rejected` and left that count unchanged. `expected_version` 1 then committed version 2. A later `expected_version` 1 returned `version.conflict` and the count stayed two rows. The post-restart snapshot shows those two `builder/db` rows, version 1 retired and version 2 active.

Six version rows have six distinct 12-byte nonces. The colliding scripted nonce is absent, which matches a rollback of that insert.

## F4. Ciphertext survives the same-volume restart

`resume.sh` restarted `oo-a400-pg` after `live_secret=write_done`. It did not drop `oo_secret_control` between the two phases and it did not rewrite `master.key`. The restart test opened `secret://lab/alpha/builder/db` at version 2, refused pinned version 1 as `version.retired`, and refused `secret://lab/alpha/builder/gone` as `revoked`. `run.json` has no value. The host script found no witness bytes in the raw logs, overwrote the witness, and recorded `witness_absent=true`.

Local `open` with the stored nonce and the canonical AAD for deployment `lab`, project `alpha`, namespace `builder`, secret `db`, version 2 succeeded. The same call with namespace `agent`, with version 1, or with the first key byte flipped, failed. That check is recorded as `aad_and_key` with HTTP 0 because it is not a route.

## F5. Gates left outside this verdict

`key_source=kms` exits 2 with `kms.external` and `customer_kms=external_gate` before the named key file is created. Responses carry `mtls_ca=not_issued`. `mtls_ca_issued_by_service` in the proof is false. The client CA in this run was minted by the test with openssl. The IdP is the copied fixture, marked `customer_idp=not_claimed`. `foreign_pool_refresh` is `not_claimed`. Gateway and Valkey storage were not part of this card. The ledger was not edited.

This live matrix did not submit steward, auditor, or business_owner, and it did not repeat a parallel rotate. Reviewer and cross-project admin were forbidden.

## F6. Attempt 01 was an overlay type error

The first builder exited in clippy with five `E0308` errors in `tests/independent/main.rs`: `push` received a `String`, and two `resolve_transport` calls passed a namespace where a pair was required. Contract and independent tests were skipped. PostgreSQL stayed up. The product archive bytes were unchanged. `linux/raw/failed-attempts/01-typecheck/` holds that log set. The green resume recopied the overlay and reused the cargo volume and the database volume.

`linux/raw/phase-marker-write.log` still shows `write marker missing` from that attempt. `phase-marker-write.exit` for the green resume is 0.
