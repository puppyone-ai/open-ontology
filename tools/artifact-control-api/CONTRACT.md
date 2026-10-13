# oo.artifact-control/v1

Author freeze for card `eef831aa-dba4-4af2-a78b-541bc9e0a398`, before implementation.
This document is the card contract. It is not acceptance of SP11-W03, SP05-W06, AT-FILE-05, AT-RT-05, AT-SEC-03, a parent goal, runsc, or a customer deployment.

The service takes one verified current scope and one completed `query-result-export` output, seals the CSV with root-lock `ring` 0.17.14 `AES_256_GCM`, and serves list, detail, and download only while that same principal, project, session, role, policy, and knowledge tuple is still the verified scope. Caller JSON is not a grant. A previous URL is not a grant.

`query-result-export` at `tools/query-result-export` is the fixed producer. This crate does not edit it. The live input is the compiled `query-pg` stdout `tools/query-result-export/tests/fixtures/query-clerk-order_doc-star.stdout`, SHA-256 `54cabda641bf1a2128e0246a78eb1a761895a0aa5a4226d136ad8aec399ad6eb`, length 1027. This card does not hand-write a replacement result. `hand-formula.jsonl` is not an input.

## Acceptance criteria

- AC-01. Every consume, list, detail, download, accept, revoke, and cleanup reads the scope file and its MAC again. The MAC is HMAC-SHA256 under the 32-byte key file. A bad MAC, a symlink, or a changed byte is `scope.rejected`.
- AC-02. `principal_id`, `project_id`, `session_id`, `role`, `policy_id`, `knowledge_id`, `knowledge_digest`, `grant`, `claims`, `identity`, `signed_url`, `url`, `token`, `absolute_end_unix`, and `revision` in a request body are `self_asserted` and write nothing.
- AC-03. Accept runs the configured `query-result-export` binary. Only `complete=true` with matching source and CSV SHA-256, size, and `text/csv` is sealed. The plaintext is not a PostgreSQL column.
- AC-04. The key file is mode `0600`, exactly 32 bytes, and not a symlink. `key_source=kms` is not implemented and is not simulated. Ciphertext is `ring-aes-256-gcm` in the object directory. Metadata is PostgreSQL.
- AC-05. List, detail, and download authorize from the current scope. The download body is the CSV. The file is read in 8192-byte chunks, the plaintext size is capped at 33554432, and the response sets `Content-Length`. No `Location` and no query signature is accepted.
- AC-06. A role, policy, or knowledge change for the same session returns 403. A different principal or session returns 403. Revoke returns 403 on the next download. The live check measures each of these under 5 seconds.
- AC-07. Object expiry is `created_unix_ms + 86400000`. Cleanup is idempotent. A second process and a PostgreSQL restart still open an unexpired object. Expired bytes are unlinked and are not downloaded.
- AC-08. `workspace_enabled` defaults to false. `true` in config or in a request is `workspace.disabled` and creates no workspace object.
- AC-09. A checksum mismatch or a secret-scan hit records `artifact_loss`, deletes the producer plaintext, and does not increase `absolute_end_unix`. The anchor trigger rejects a larger end.
- AC-10. Intake names are one file component. `..`, symlinks, and `http://` or `https://` request values are refused. The service does not fetch a URL. Tokens and the key stay out of Git.
- AC-11. Responses carry `production_signed=false`, `runsc=not_claimed`, `customer_integration=not_claimed`, and `parent_complete=false`.

## Scope evidence

The operator file is canonical UTF-8 JSON, keys in this order, no extra whitespace:

`version`, `principal_id`, `project_id`, `session_id`, `role`, `policy_id`, `knowledge_id`, `knowledge_digest`, `absolute_end_unix`, `revision`.

`version` is `oo.artifact-control/scope/v1`. Identity fields match `^[a-z][a-z0-9_]{0,62}$`. `knowledge_digest` is 64 lowercase hex digits. `absolute_end_unix` and `revision` are non-negative integers. The MAC file is 64 lowercase hex digits and an optional trailing LF. The first accept or loss inserts the session anchor. A later larger end is `session.extended` and updates nothing.

## HTTP

JSON requests are at most 16384 bytes. Query strings are rejected. Mutations require `Origin` equal to `https_origin` and `X-OO-CSRF`. Comparison uses `ring` constant-time equality. The cookie `oo-artifact` is 43 base64url characters. PostgreSQL stores only SHA-256. Responses send `Cache-Control: no-store`.

| Method and path | Success |
| --- | --- |
| `POST /control/v1/session/consume` | 200, `Set-Cookie`, CSRF, scope tuple |
| `GET /control/v1/artifacts` | 200, own active rows, `signed_url=null` |
| `POST /control/v1/artifacts` | 201 stored, or 200 replay, or 409 `artifact_loss` |
| `GET /control/v1/artifacts/{id}` | 200 manifest |
| `GET /control/v1/artifacts/{id}/content` | 200 CSV bytes |
| `POST /control/v1/artifacts/{id}/revoke` | 200 revoked |
| `POST /control/v1/cleanup` | 200, idle or expired count |

`POST /control/v1/artifacts` body is `version`, `source_file`, `columns`, `idempotency_key`, and optional `workspace`. `source_file` is one intake-root file name. `idempotency_key` matches `^[A-Za-z0-9_-]{16,128}$`. The same key and body hash returns the stored JSON. A different body hash is `idempotency.conflict`.

## Errors

Messages are static. `retryable` is true only for `dependency.not_ready` and `commit.unknown`.

| Code | HTTP |
| --- | --- |
| `malformed`, `self_asserted`, `path`, `budget` | 400 |
| `unauthenticated` | 401 |
| `forbidden`, `scope.rejected`, `origin.rejected`, `csrf.rejected`, `workspace.disabled`, `aead.rejected`, `revoked`, `expired` | 403 |
| `not_found` | 404 |
| `method.not_allowed` | 405 |
| `session.extended`, `idempotency.conflict`, `artifact_loss` | 409 |
| `dependency.not_ready`, `commit.unknown` | 503 |

`artifact_loss.reason` is `checksum`, `secret_scan`, `incomplete`, or `producer`. Secret scan matches `-----BEGIN PRIVATE KEY-----`, `-----BEGIN RSA PRIVATE KEY-----`, `-----BEGIN OPENSSH PRIVATE KEY-----`, an `AKIA` key shape, or the raw 32-byte key inside the producer bytes.

## Storage

Schema `oo_artifact_control`. The migration runs once in a transaction. The same SHA-256 replays. A different hash is `drift`. Sessions set `search_path` to `pg_catalog`, `lock_timeout`, `statement_timeout`, and `synchronous_commit=on`. Clients require TLS. Advisory lock `7398000398`. Runtime SQL has no `DELETE`. `artifact_loss` update and delete raise `42501`. A larger `session_anchor.absolute_end_unix` raises `42501`.

Object files are `{object_dir}/{32 hex}.bin`, mode `0600`. Startup removes only orphan files of that name and `.{32 hex}.partial`. Other names stay.

## Process

`artifact-control install --config <file>` and `artifact-control serve --config <file>`. The config points at files. Passwords, the AEAD key, and TLS private keys are not arguments and not environment variables. Exit 0 is install success or a listening server. Exit 1 is usage or malformed input. Exit 2 is a refused rule. Exit 3 is `commit.unknown` or a connection failure.

## Out of scope

runsc certification, a customer IdP, xlsx, a signed-URL TTL, S3, and phase closure. Those gaps stay uncertified.
