# review-ledger v1

Contract `oo.review-ledger/v1`. Schema `oo_review_ledger`. This is the SP07-W05 / SP02 durable S9/S10 queue for one private project. It is not SP07 acceptance, not publication, and not management RBAC.

Frozen before the store code. The schema name is not `jobs`, `audit`, `identity`, `oo_cdc_offset`, `oo_enum_control`, `oo_dict`, `oo_calendar_control`, `oo_session_control`, `oo_token_control`, or `_oo`.

## Amendment

Frozen before the code that closes the auditor batch write on `f862683945a6e5a8803e3f9450d0f51cde5a98ad`. This amendment does not accept SP07-W05 and does not add a component.

`batch` authenticates the caller, then refuses `role=auditor` with `role.forbidden` before an existing-batch read, before any item decision, and before a batch row or a batch audit event. An item refusal is not a reason to insert the batch. `accept_all: true` still returns `batch.accept_all` before authentication and writes nothing.

`history` is read-only for `reviewer`, `owner`, and `auditor`. The caller is a control-file principal, and that principal has a non-empty scope. History returns the project record. It does not drop rows outside that scope, and it inserts nothing. An auditor history call is not a grant, not publication, and not a batch.

A description-pack receipt is stored only after `apply_description_pack` returns `packed` or `reviewed_unchanged` with unauthenticated, unpublished, and ungranted statements, and a later transaction still sees the same decision body and head candidate. The receipt stores the command-file SHA-256, the decision candidate SHA-256, and the SHA-256 of the compiler document. `git_applied`, `published`, and `curated_files_written` stay false. The compiler document is not a git apply and not a publication. A malformed command is `compiler.refused` and writes no receipt.

`commit.unknown` means the commit reply was lost. The transaction may have committed or may have aborted. The library does not retry the call and does not send a second commit. The process exit is 3. A caller that retries without reading history can double-apply or hit `idempotency.conflict`. This crate does not inject a lost reply.

## Authority

Public management authenticated identity is not implemented. This crate does not verify OIDC, does not accept a role from a request header, and does not accept a role from the request body.

Each call reads a platform control file and a private credential file. The credential is hashed and matched to exactly one principal. That principal has exactly one `role`: `reviewer`, `owner`, or `auditor`. A second role, a `roles` array, or a config that sets `authority` to anything other than `not_verified` is refused. `management_identity` must be `not_claimed`.

The local-operator fixture may call the library and the CLI. Every result still says `authenticated: false`, `authority: not_verified`, and `management_identity: not_claimed`. A declared reviewer label is not an owner and is not a grant.

Scope is the control file's object list. The request cannot carry `scope`. An object outside that list is `scope.unauthorized`.

`grant`, `grant_roles`, `certified`, `publish`, `apply`, `role`, `authorization`, `authenticated`, `headers`, `visible_roles`, `column_acl`, and `auto_accept` on a request are `not_grant`. No row is written. This crate does not `GRANT` data roles and does not certify a metric, an enum, or an owner.

`owner_certification` is allowed only when the caller role is `owner` and the principal id equals the queue row's declared `owner_label`. A reviewer attempt is `reviewer_not_owner`. The stored row still has `grants = not_authorized` and `curated_applied = false`.

## Queue

`enqueue` stores one head and one append-only revision for `dataset` or `column`. The hashes are `candidate_sha256`, `input_sha256`, `evidence_sha256`, `schema_sha256`, `curated_sha256`, and `source_sha256`. `candidate_protocol` is only `oo.builder.description-candidate/v1` or `oo.builder.description-candidate/v2`. This crate does not define a new S9 compiler schema.

The same candidate and the same hashes replay. A different candidate at `expected_revision` equal to the head inserts the next revision and leaves the old revision in place. The head status is never `applied` or `published`.

## Decision

`decide` locks the head and, in one transaction, inserts the decision and one audit event, then commits. A failure of the audit insert rolls the decision back.

The decision is refused, and nothing is written, when:

| Code | Condition |
|---|---|
| `revision.conflict` | `expected_revision` is not the head revision |
| `candidate.changed` | `candidate_sha256` is not the head candidate |
| `stale` | input, evidence, schema, curated, or source hash differs |
| `idempotency.conflict` | the same key has a different canonical body |
| `pii.unknown` | classification `pii` is missing or `unknown` |
| `scope.unauthorized` | the object is outside the principal scope |
| `reviewer_not_owner` | a reviewer records `owner_certification`, or the owner principal is not `owner_label` |
| `not_grant` | the request asks for a grant, certification, publication, or a caller-selected role |
| `role.forbidden` | an auditor writes |

Replay of the same key and the same body returns the stored row and does not insert a second audit event.

A stored decision always has `curated_applied = false`, `published = false`, `authenticated = false`, and `grants = not_authorized`. The decision transaction does not call a compiler and does not write curated files.

### Allowed decision fields

`label_description` choices are `approve`, `reject`, and `defer`. `reject` and `defer` have no `fields`. `approve` fields are only `label`, `description`, and, for a dataset, `grain`. Each value is `{overwrite: false}` or `{overwrite: true, current_sha256}`. Confidence, caveats, open questions, PII, semantic type, classification, status, and unit are not approval fields.

`classification` choice is `confirm`. Fields are only `pii` (`none`, `personal`, or `sensitive`), `semantic_type` (`^[a-z][a-z0-9_]{0,63}$`), and `state` (`confirmed`). `unknown` PII is `pii.unknown`. The caller cannot set `auto_accepted`. Confirmation does not grant a role and does not publish the column.

`owner_certification` choice is `record` and has no fields.

## Batch

`accept_all: true` is `batch.accept_all` and writes nothing. An auditor is `role.forbidden` before any item decision and before the batch row. Otherwise each object is its own decision transaction. One later transaction records the batch. Status is `complete` only when every item was stored. Any refusal leaves `partial`. Neither status applies curated text or publishes. A high-risk item is not bypassed by the other items.

## Compiler receipt

`receipt` is a second step. It does not run inside the decision transaction. The command file bytes must hash to the decision `source_sha256`. The crate then calls the existing compiler:

| `compiler` | Function | Accept |
|---|---|---|
| `description-review` | `description_review::apply_description_review` | outcome `reviewed`, protocol `oo.builder.description-review/v1` or `v2`, compiler `oo-builder/description-review/v1`, statements unauthenticated and unpublished, and the outcome decision carries this candidate, schema, and evidence hash |
| `description-pack` | `description_pack::apply_description_pack` | only after every `required_columns` entry has a stored `confirm` classification; outcome `packed` or `reviewed_unchanged`. The receipt binds that command hash, the decision candidate hash, and the compiler-document hash |

`classification.missing` returns before the pack compiler is called and writes no receipt. A compiler error, a stale queue reason, or a hash mismatch writes no receipt. There is no pending receipt row.

After the compiler returns an acceptable document, a new transaction re-reads the decision. If the head candidate or revision moved, the compiler bytes are discarded and no receipt is stored. The receipt stores the command hash, the approved candidate hash, and the outcome hash. `git_applied`, `published`, and `curated_files_written` are false. The decision row is not updated.

## Install

`migrations/oo_review_ledger_v1.sql` runs once in one transaction. The manifest stores that file's SHA-256, the project id, and the control-file SHA-256. The same triple replays. A different SQL hash is `install.drift` and is not repaired. A different control file on a later call is `control.drift`.

The install role is `oo_review_ledger_admin`. The runtime login is a member of `NOLOGIN` `oo_review_ledger_runtime`. The migration does not create roles, does not create a customer grant, and does not install an audit-collector certificate.

Decisions, revisions, audit events, receipts, and batches are insert-only. The head row may move forward to a higher revision. `synchronous_commit` is on for these transactions. A lost commit reply is `commit.unknown`. The outcome is ambiguous, and the library does not retry it.

## CLI

`review-ledger <install|enqueue|decide|batch|receipt|history> <request.json>`

Exit 0 is `persisted`, `replay`, `partial`, `complete`, `bound`, or `history`. Exit 2 is `refused`. Exit 3 is `commit.unknown`. Exit 1 is usage. Stdout is one JSON object. Passwords and credentials are files named by the request. They are not arguments and not stdout.

## Still required

Management OIDC and RBAC, the HTTP control API, Git apply of curated files, publication, the audit collector and its certificate, customer grants, and column visibility grants are not this crate. A reviewer confirmation does not make a column visible to an agent role. Unclassified and unknown columns stay ungranted. SP07-W05 is not accepted by a green test.
