# oo.secret-control/v1

Author freeze for card `eef831aa-dba4-4af2-a78b-541bc9e0a384`. This document is the card contract. It is not acceptance of SP02-W04, not acceptance of a parent goal, and not an independent review.

The service writes, rotates, revokes, and resolves secret references. It is not a customer KMS and it does not issue or rotate an mTLS CA. Frontend B12 operations pages are out of scope. The metadata-only secret reference matches the B07 secret surface: management responses carry `secret://` metadata and never a secret value.

## Producer binding

`getIdentityCurrent` is the live producer `tools/control-identity`, contract `oo.control-identity/v1`. This crate does not link that crate and does not read `oo_identity_control`. Every mutation and every status read fetches `GET /control/v1/identity/current` from the configured origin. Caller-supplied JSON is never authority. `try_from_caller_json` always fails.

The producer matrix has no `secret.write` capability. This card binds write, rotate, revoke, and status to the live capability `project.admin` on the path project. `business.record`, `review.decide`, `ontology.draft`, and `audit.read` do not write. A project that is absent from the current envelope is forbidden. Unknown capabilities and any `install_state` other than `initialized` are untyped and fail closed as `dependency.not_ready`.

The session cookie is `__Host-oo-session` (43 base64url characters). Mutations require `Origin` equal to `https_origin` and header `X-OO-CSRF` equal to the current CSRF token. Status requires the cookie and does not require `Origin`. Comparison of origin and CSRF uses ring HMAC, not `==`.

## Acceptance criteria

- AC-01. A real `getIdentityCurrent` envelope is the only authority. Caller JSON is rejected.
- AC-02. Write, rotate, revoke, and status require `project.admin` on that project. Other roles and other projects are forbidden.
- AC-03. Values are sealed with root-lock `ring` 0.17.14 `AES_256_GCM` and stored in PostgreSQL. Management responses are `secret://` metadata only.
- AC-04. The master key is a 32-byte file, mode `0600`. `key_source=kms` refuses startup with `kms.external` and does not simulate unwrap.
- AC-05. Nonces are 12 bytes from OS `getrandom` in the production binary and are unique in `secret_version.nonce`.
- AC-06. AAD binds protocol, deployment, project, namespace, secret id, and decimal version. Wrong AAD or wrong key fails closed.
- AC-07. Resolve requires a verified client-certificate workload URI plus a matching project and namespace. A management session cannot read the value. There is no plaintext GET.
- AC-08. A grant in namespace `builder` does not resolve namespace `agent`, and the reverse is also refused.
- AC-09. Rotate is a version compare-and-swap in one transaction. A unique-nonce violation or a stale version leaves the previous version active.
- AC-10. Revoke rejects a later resolve.
- AC-11. A resolve that pins a version other than the current version is `version.retired` and does not decrypt.
- AC-12. After a PostgreSQL restart the current ciphertext still opens and the retired version stays refused.
- AC-13. Plaintext is absent from logs, management responses, and Git.
- AC-14. Customer KMS and mTLS CA issuance stay external gates on every response (`customer_kms=external_gate`, `mtls_ca=not_issued`).
- AC-15. This card does not accept SP02-W04, a parent goal, or an independent review.

Also out of scope, and not mocked as done: customer IdP (the test issuer is a fixture), refresh of foreign connection pools, and Gateway or Valkey namespace storage. This process zeroizes its own buffers. It does not wipe another component's memory.

## References and names

`secret://{deployment}/{project}/{namespace}/{secret_id}` has no version. `project`, `namespace`, and `secret_id` match `^[a-z][a-z0-9_]{0,62}$`. `deployment` matches `^[a-z0-9][a-z0-9_-]{0,62}$` and comes from configuration, not from the client.

Workload URIs match the gateway grammar exactly: one SAN URI `urn:oo:workload:` plus 1 to 128 characters in `[A-Za-z0-9_.:-]`. Forged headers are not authority. The operator supplies the client CA. This service verifies it and does not issue it.

## AAD

Canonical bytes are the concatenation, for each field, of a big-endian `u16` length and the field bytes. Fields, in order: protocol `oo.secret-control/aad/v1`, deployment, project, namespace, secret id, decimal version with no leading zeros. Ciphertext is `ring-aes-256-gcm` and any other cipher name is refused before open.

## HTTP

Management listener has no client certificate. Component listener requires a client certificate. Query strings are rejected. Bodies are at most 16384 bytes. The handler budget is 5 seconds. Responses send `Cache-Control: no-store`. JSON objects reject duplicate and unknown keys.

| Method and path | Operation | Success |
| --- | --- | --- |
| `PUT /control/v1/projects/{project_id}/namespaces/{namespace}/secrets/{secret_id}` | `putSecret` | 201 metadata, replay 200 |
| `GET` the same path | `getSecretStatus` | 200 metadata plus `subject` |
| `POST .../secrets/{secret_id}/rotate` | `rotateSecret` | 200 metadata |
| `POST .../secrets/{secret_id}/revoke` | `revokeSecret` | 200 metadata |
| `POST /component/v1/resolve` | `resolveSecret` | 200 metadata plus `value` |

`PUT` of an existing secret is `version.conflict`. The first write creates version 1. Rotate requires `expected_version` and the stored workload, and installs `expected_version + 1` only when the head still has `expected_version`. Revoke marks the head and the current version `revoked`. Resolve returns `value` only when the certificate URI equals the stored grant, the reference matches the body project and namespace, and the row is the active current version. An optional version that is not current returns `version.retired` without decrypting.

Idempotency keys match `^[A-Za-z0-9_-]{16,128}$`. The store keeps the SHA-256 of the exact request bytes and the metadata JSON. It never keeps the value. The same key with a different body hash is `idempotency.conflict` and writes nothing.

## Errors

Messages are static and do not echo identifiers or values. `retryable` is true only for `dependency.not_ready` and `commit.unknown`.

| Code | HTTP |
| --- | --- |
| `malformed` | 400 |
| `unauthenticated` | 401 |
| `forbidden`, `origin.rejected`, `csrf.rejected`, `namespace.rejected`, `workload.rejected`, `aead.rejected` | 403 |
| `not_found` | 404 |
| `method.not_allowed` | 405 |
| `revision.conflict`, `version.conflict`, `version.retired`, `idempotency.conflict`, `revoked` | 409 |
| `dependency.not_ready`, `commit.unknown` | 503 |

`kms.external` is process exit 2, not a route. CLI exits: 0 ok, 1 malformed, 2 refused, 3 `commit.unknown`. Install and serve each write one JSON object and no secret.

## Storage

Schema `oo_secret_control`. Migration runs once inside a transaction. The manifest stores the SHA-256 of the migration file. The same hash replays. A different hash is drift. Sessions set `search_path` to `pg_catalog`, `lock_timeout`, `statement_timeout`, and `synchronous_commit=on`. Clients require TLS. Advisory lock `7384000384`. The head's foreign key to `secret_version` is deferred, so a commit cannot keep a head without its version row. Runtime has no `DELETE`. Audit rows are metadata only and append-only. A failed resolve audit does not return plaintext.

## Nonce injection

The `secret-control` binary asks the OS for every nonce. Library tests may supply `ScriptedNonce` so a repeated nonce hits the unique constraint and the transaction rolls back. That path is a real PostgreSQL rejection, not a stubbed success.
