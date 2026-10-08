# Feedback receiver

Contract `oo.feedback.receiver/v1`. Session `eef831aa-dba4-4af2-a78b-541bc9e0a385`.

Frozen before implementation. This is the SP02-W06 / SP13-W04 component slice that accepts bounded untrusted feedback and stores review candidates. It is not full SP02-W06, not full SP13-W04, not parent acceptance, not a publisher, and not a grant installer.

The audit collector is not this process. `tools/audit-ingest` is read-only. Workload URI parsing calls `audit_ingest::certificate_uri`. This crate does not copy that parser and does not use the `audit_ingest` role or `oo_audit_ingest`.

Suggestion rules are `selfbuild_suggestions::produce`. This crate does not reimplement S6–S8 or S9. It does not invent an evidence hash: every seal is `canonical_sha256` of the value passed to `produce`. It does not treat a caller field as authentication. `authorization.authorized` is set only by this process after it has built the command from the operator files and the bounded items. `knowledge_verified`, `certified`, `published`, `grants_issued`, and `roles_changed` stay false. No Gateway config is passed, so nothing is marked admitted or verified.

## Acceptance

1. rustls HTTPS requires a client certificate. Identity is the single URI subject alternative name. `schema/registry.md` maps one URI to one project. Headers, including `X-Project`, are ignored.
2. `POST /v1/feedback` accepts one JSON body of at most 32768 bytes. The body is untrusted feedback. A larger `Content-Length` is `oversized` and stores nothing.
3. The PostgreSQL role is `feedback_receiver`. It may `INSERT` and `SELECT` only in schema `oo_feedback_receiver`. It may not `UPDATE`, `DELETE`, or `TRUNCATE`. It has no `CONNECT` on the audit database and no read of `pg_authid`.
4. The receipt key is the caller `receipt_id`. The same bytes return the original receipt and the original candidates. A different body under that receipt is HTTP 409 and commits nothing new.
5. An item whose `project` is not the certificate project is `cross_project` and commits nothing. A body `project` that is not the certificate project is `forbidden`.
6. Prompt text stays in `feedback_item.item_text` and in `source.feedback_text` of channel `untrusted_feedback`. It is not copied into SQL templates, questions, grants, or role policy.
7. Candidates are rows a review reader can query. The ack is not the only copy. `GET /v1/reviews` returns that project's stored candidates and their sources. `published`, `grants_issued`, and `roles_changed` are false. This process does not `GRANT`, does not write `curated/`, and does not call a model.
8. Other methods and paths are refused. There is no admin API and no credential API.

## HTTP

`POST /v1/feedback` with `Content-Type: application/json`. `GET /v1/reviews` with no body. The server certificate name is `feedback.local`.

Request `schema/request.v1.json`:

```json
{
  "protocol": "oo.feedback.receiver/v1",
  "receipt_id": "uuid v4",
  "project": "shop",
  "items": [
    {"project": "shop", "kind": "gap", "text": "untrusted", "target": "logistics.shipment"}
  ]
}
```

`items` length is 1 to 32. `kind` is `gap`, `reject`, `failure`, or `comment`. `text` is 1 to 500 characters with no C0 control. `target` is at most 128 characters from `[a-z0-9_.]`. Duplicate JSON keys are refused. Unknown fields are refused. These bounds are the `selfbuild-suggestions` feedback rule; this crate calls that rule rather than storing a private copy of a passed message.

Success is HTTP 200 and `schema/ack.v1.json`. `duplicate` is false on the first commit and true on a byte-identical replay. `candidates` are the S9 queue from `produce`, each with `source`. Replay after process restart reads the same rows.

`GET /v1/reviews` is HTTP 200 and `schema/review.v1.json`. The project is the certificate project. At most 256 candidates are returned, ordered by score descending, then `candidate_id`, then `receipt_id`. `truncated` is true when more rows exist. Those rows remain stored.

| condition | HTTP | `error` |
|---|---|---|
| malformed JSON, unknown field, empty items, bad uuid, bad kind or text | 400 | `protocol` |
| body larger than 32768 bytes, or `produce` refuses `scope` | 400 | `oversized` or `bound` |
| URI not in the registry, or body project mismatch | 403 | `forbidden` |
| any item project mismatch | 403 | `cross_project` |
| same receipt, different body | 409 | `conflict` |
| certificate or handshake failure | no HTTP 200 | connection closed |
| producer effects are not closed | 500 | `producer` |

The error body is `{"protocol":"oo.feedback.receiver/v1","error":"<token>"}`. Tokens are static. Feedback text is not an error token.

## Operator inputs

`--config <path>` is the only argument. The file mode is `0600` or `0400`. Keys are `listen`, `ca_file`, `cert_file`, `key_file`, `registry_file`, `pg_host`, `pg_port`, `pg_db`, `pg_user`, `pg_password_file`, `pg_ca_file`, `catalog_file`, `relationships_file`, `profile_file`, `source_id`, `role`. Unknown keys are refused. Key and password files are mode `0600` or `0400`. Their bytes are not argv, environment, logs, or git.

`catalog_file`, `relationships_file`, and `profile_file` are the sealed operator documents. The client cannot replace them. `role` is `oo_r_` plus a project token. `source_id` is a project token. `model_scope` is `none`. `schema_version` is `oos/v1`. `evidence_sha256` is the catalog seal. `negative_pairs` and `curated_overrides` are empty. `prior_candidate_ids` are at most 256 stored ids for that project, in lexicographic order. Startup calls `produce` with an empty item list and does not insert that probe.

## PostgreSQL

`migrations/oo_feedback_receiver_v1.sql`. An administrator applies it. The receiver is not the owner.

`receipt` stores the payload hash, the four seals, the evidence hash, the closed `effects`, the `dedup` object, the `scope_drops` object, and the producer `outcome`. `feedback_item` stores each kept untrusted item. `candidate` stores each queue row and its `source`. Checks reject `published`, `grants_issued`, `roles_changed`, `trusted_metric`, and `knowledge_verified` when those flags are true. `feedback_item.untrusted` must be true.

## Out of scope

Audit WAL, collector replay, Gateway `report_gap` routing, review decisions, G1–G13, publication, customer identity, and a claim that SP02-W06 or SP13-W04 is accepted.
