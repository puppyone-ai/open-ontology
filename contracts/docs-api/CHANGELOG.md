# docs-api changelog

Contract between the Open Ontology docs engine (`oo serve`) and its clients: the `oo` CLI, MCP tools and the Super Ontology app.
Rules: spec `specs/super-ontology/50-commit-and-traceability.md` §6. A minor version only adds optional
fields, endpoints, or enum values; clients ignore unknown fields and show unknown enum values as unknown.

## 1.0-draft (2026-11-17, L1 freeze)

- Body: spec 10 §8.2 `$defs`, §8.3 endpoints (machine-readable list in `x-endpoints`), §8.4 error codes
  (`errors.v1.json`), §8.5 events (`events.v1.schema.json`).
- Folded in before freeze: §8.8 G1–G3, G5–G15 (G4 `review:undo` is listed with `since: "1.1"`),
  §16.10 G16–G24 (drive_rev, coverage, task groups, `:check`, updates, constraints, corrections, `as_of`).
- `ReviewKind` reserves `constraint_conflict` (D57) and `speaker_identity` (D74, used from L6).
- `errors.v1.json` adds `DOCS_SOURCE_NOT_FOUND`, `DOCS_SOURCE_INVALID_CONFIG`, `DOCS_SOURCE_PROTOCOL`,
  `DOCS_SOURCE_IO` for the closed `DocumentSource` error set, and the codes named in G7, G17, G18, G20, D65.
- Becomes `1.0` at the L2 meeting point (10 §8.7) once the real engine passes the contract tests.
- 2026-11-17 (still 1.0-draft): `errors.v1.json` adds `DOCS_PARSE_LIMIT_BLOCKS`, `DOCS_PARSE_LIMIT_CELLS`, `DOCS_PARSE_LIMIT_PAGES` (10 §4.11 limits that the parsers return as state codes).

## 1.0-draft (L4, B-S5 cards)

- `Fact.revision` (optional integer): the `If-Match` token of `POST /facts/{id}:correct`.
- `DOCS_REVIEW_MERGE_CONFLICT` now has HTTP 409: `POST /entities:merge` answers it when the merge target was
  already merged away, the types differ or the identities contradict (it stays an item level code of
  `review:decide`).
- `GET /ontology/versions` items carry `covers_drive_rev` (already in the schema); `pinned_by` is always 0
  until the share lifecycle cards maintain it.
- 2026-11-17 (still 1.0-draft): `Fact.revision` (optional, for If-Match on corrections); `DOCS_REVIEW_MERGE_CONFLICT` gets HTTP 409.

## 1.2 (L6, B-S10 cards)

- `POST /v1/docs/drives/{id}:rebuild` (`since` 1.2): the one manual full rebuild (10 4.2, D23). Body `RebuildRequest`
  `{"confirm": "<drive id>"}`, required; without it or with another id the answer is 400 `DOCS_ENGINE_BAD_REQUEST`
  (`/confirm`). Answer 202 `RebuildInfo`. Every file with a result becomes `outdated`; the engine reprocesses them
  at priority 900, two at a time, and each new extraction goes through DOC-G10 (a `reprocess_diff` review item when
  it changes more than 30% of the file's facts or drops a fact a live Share uses).
- `FileSummary.progress.unit` and the `file.progress` event add `ms` (done and total are milliseconds of the source
  timeline of an audio or video file, 10 17.3).
- `errors.v1.json` adds `DOCS_SOURCE_MEDIA_TOO_LONG`, `DOCS_PARSE_MEDIA_HOST_DECODE_MISMATCH`,
  `DOCS_PARSE_ASR_FAILED`, `DOCS_PARSE_DIARIZATION_FAILED` (state codes) and `DOCS_REVIEW_SPEAKER_UNCONFIRMED`
  (item level code of `review:decide`: accepting a fact whose speaker is unconfirmed).
- `ReviewKind` values `type_promotion`, `speaker_identity` and `reprocess_diff` are now written by the engine
  (enum unchanged, reserved since 1.0). Items carry: `type_promotion` `{label, instances, files, examples}`;
  `speaker_identity` `{b3, speaker, placeholder, candidates:[{name, source, block_id, quote}]}` (accept with
  `edit.name`); `reprocess_diff` `{gate, prior, added, removed, share_used_removed, version_id}`.
- `BRIEF.md` of a refreshed Share has a section "自上次以来的变化（vA → vB）" after the head (added, changed,
  retracted, fixed order); not an API field.

## 1.3 (L5, B-S7 and B-S8 cards)

- Suggest endpoints (`since` 1.3): `POST /v1/docs/suggestions` (`SuggestRequest`: one `SuggestionRecord`, `items`, or
  `documents` read from `.context/suggest/`; a share's `oodr_` credential with scope `suggest:write`, or the engine token with
  `share_id`; idempotent per share on `idempotency_key` or the content hash, no `Idempotency-Key` header), `GET /suggestions`,
  `GET /suggestions:digest?day=`, `GET /suggestions/{id}`, `GET /suggestions/{id}/diff` (a file-level draft for the revision mode;
  OO never writes the Drive, `writer` is `desktop`) and `POST /suggestions/{id}:decide` (`accept`, `reject`, `undo`).
- The `suggestion` review kind is live: `subject` is `{suggestion_id, share_id, kind}`; deciding the item through `review:decide`
  does what `:decide` does. Reason codes: `suggestion_manual_kind`, `suggestion_supersede_in_use`, `suggestion_functional_conflict`,
  `suggestion_no_scope_evidence`, `suggestion_supersedes_unknown`, `suggestion_unresolved_entity`.
- Refs credentials of `suggester` and `editor` shares carry `suggest:write`; the `editor` role can be created
  (`DOCS_SHARE_ROLE_UNAVAILABLE` is no longer returned). A credential may call `POST /suggestions` for its own share.
- `share.update_appended` entries with `summary_key` `suggestion_accepted` tell every live Share that used a superseded fact.
- `errors.v1.json` adds `DOCS_SUGGEST_NOT_FOUND`, `DOCS_SUGGEST_ROLE_FORBIDDEN`, `DOCS_SUGGEST_SCOPE_MISSING`,
  `DOCS_SUGGEST_NOT_PENDING`, `DOCS_SUGGEST_NO_EVIDENCE`, `DOCS_SUGGEST_UNRESOLVED_ENTITY`.

## 1.4 (L7, B-S11 and B-S12 cards)

- Drive config `policy` `{allow, deny}` (`DrivePolicy`) on every backend: whole segment path prefixes, `deny` wins, an empty `allow` is
  the whole Drive. Enforced in ingest (a denied path is never listed, hashed or parsed) and in the visible graph (denied files are not
  in any snapshot a pack or projection is built from).
- Backends `oss`, `cos` and `r2` are served (the S3 compatible path, `DriveConfigCloud`): `region` (oss, cos), `account_id` (r2),
  `internal`, `endpoint` override. `s3` takes the same object. Keys are never inline (`secret_ref`).
- `POST /v1/docs/drives/{id}:events` (`since` 1.4, `EventsRequest`, `EventsResult`): bucket event notifications as change hints.
  Duplicates and older events of a key are dropped, each remaining key gets one targeted sync; deletions are only reported.
- Cloud proxy mode (local entry only): `POST /v1/docs/cloud-tokens` issues an `oodc_` token bound to a tenant and a set of Drives
  (shown once); `DELETE /v1/docs/cloud-tokens/{id}` revokes it. The token works on the cloud entry (a second listener,
  `OO_DOCS_CLOUD_LISTEN`, own `Host` list) only, never on the loopback API; the cloud entry accepts nothing else and calls only shares
  (create, get, `:revoke`), `context-packs:compile` and the packs' files, on the token's Drives.
- Audit (`share.created`, `share.revoked`, `refs.accessed`, token events) in an append-only, hash chained table:
  `GET /v1/docs/audit/shares?drive_id&agent&tenant&since&until` (`AuditShares`: who shared what to which agent, when) and
  `GET /v1/docs/audit/verify` (`AuditVerify`). The agent is the request header `OO-Docs-Agent` of the call that created the share.

## 1.5 (L8, B-S13 vectors)

- `CompileRequest.vector` (optional, `since` 1.5, enum `["hashgram/1"]`): the local vector model version that ranks the
  "other related" section of BRIEF. The engine rejects an unknown value with `DOCS_ENGINE_BAD_REQUEST` (field pointer `/vector`). When omitted the
  operator setting `OO_DOCS_VECTORS` still applies, as before. The version is part of the pack parameters, so the same
  version replays byte for byte. The field is also accepted in `POST /shares` `compile`. Additive; no other change.

## 1.6 (D83, WP2: Agent tokens and the link surface)

- Agent tokens (`since` 1.6, local entry only): `POST /v1/docs/agent-tokens` issues an `ooa_` token (`AgentTokenRequest`, `AgentToken`,
  shown once; only its hash is stored) with scopes `docs:read`, `db:read`, optional `docs:suggest` and `docs:write`, a Drive set or
  `all_drives`, a TTL (60 s to 30 days, default 7 days) and, for `db:read`, the NAME of a Gateway profile (`gw_profile`: the CLI proxy reads
  the database ontology with it, the Agent never holds a Gateway token). One live token per name. `GET /agent-tokens` lists them (never a
  token or hash), `DELETE /agent-tokens/{id}` revokes (the next call is 401, at once), `POST /agent-tokens:introspect` (control token only) answers
  `active` and the principal, `GET /agent-tokens/self` is what an Agent asks about itself and the Drives it reaches.
- The engine checks the scope on EVERY route an `ooa_` token calls. The default is deny (403 `DOCS_SHARE_REFS_OUT_OF_SCOPE`, `details.reason`
  `route`, `scope`, `drive` or `drive_id_required`): `docs:read` reads entities (`drive_id` required unless `all_drives`), entity detail and `resolve`
  restricted to the token's Drives, files, blocks and content of those Drives, `GET /links`; `docs:suggest` posts a Suggest of a share of its Drives;
  `docs:write` decides links of its Drives. An entity outside the Drives is 404.
- Audit (`GET /v1/docs/audit/agents?agent&since&until&limit`, `AuditAgents`): `agent_token.issued`, `agent_token.revoked`, one `agent.read` per call
  and one `agent.denied` per refusal in the same hash chained log (`GET /audit/verify` covers them); the actor is `agent:<name>`, a call is a route
  template whose values are `{id}`.
- The projection job (engine process, no route): published document entities (and their anchored evidence) are projected into the shared
  entity namespace of the Agent DB (`_oo_docproj`), incrementally on each published version, and linked to rows of database tables by the rules below.
  Idempotent and replay safe. It does nothing without an Agent DB binding (documents only is a normal state).
- Links (`since` 1.6): `GET /v1/docs/links?doc=<entity key>|db_table=<schema.table>[&db_key=][&drive_id=][&status=live|auto|confirmed|rejected|all][&limit]`
  (`LinkList`, `LinkItem`: each link carries `provenance.file` anchors and `provenance.table_row` with the matching column and rule revision; the lookup
  works from a document entity to rows and from a row to documents), `POST /links:decide` (`confirmed`/`rejected`, kept by every rebuild),
  `POST /links:project` (run the job now, `ProjectRuns`), `GET|PUT|DELETE /link-rules` (`LinkRule`: which columns of which table match which entity type),
  `GET|PUT|DELETE /agent-db-binding` (`AgentDbBinding`: a secret reference to the Agent DB login, never a URL; also `OO_DOCS_AGENT_DATABASE_URL_REF`).
- No new error code. Additive; no existing route changes.

## 1.7 (D83, WP3: the unified query surface)

- `/v1/ontology/*` (`since` 1.7, served by the `oo serve` proxy, not the engine): `GET /search?q&type&limit`, `GET /entities/{urn}`,
  `GET /sources`, `POST /query` (`OntologySearchResult`, `OntologyEntity`, `OntologySources`, `OntologyQueryRequest`,
  `OntologyQueryResult`). One merged result set over the document ontology (engine, scoped by the `ooa_` token's Drives and scopes)
  and the database ontology (Gateway, through the Gateway profile the token names; the Agent never holds a Gateway token).
  URNs: `oo:doc/<drive_id>/<entity_key>` and `oo:db/<source>/<schema.table>/<pk>`. Every item and fact carries `provenance`
  (`OntologyProvenance`: a file anchor or a table row with `publication_id`). One side missing is a normal state (`sides.*.status`).
- `POST /query` accepts one read-only SELECT (403 `ONTOLOGY_READ_ONLY` otherwise) and needs `db:read` (403 `ONTOLOGY_SCOPE_DENIED`);
  a revoked or expired token is 401 `ONTOLOGY_TOKEN_INVALID`. The proxy forwards an `ooa_` token on `/v1/docs/*` untouched (the engine
  checks it). Proxy codes are local to the surface and not in `errors.v1.json`. Additive; no existing route changes.

## 1.8 (contract defects found against the real engine)

- `events.v1.schema.json` gets a `data` schema per event type (`x-data`, selected by `type`): what the engine sends is now
  checked field by field, not only the envelope. Shapes follow spec 10 8.5; the three that differed are made exact on the
  engine side: `share.created` and `share.state_changed` carry `{share: Share}` (the current Share, `drive_id` of the event is
  the Share's Drive), `review.changed` is sent once per Drive a decision touched with that Drive's `drive_id`, `pending` by
  kind and `by_source_changed`, and `ontology.version_published` is `{n, as_of, facts_added, facts_retracted, gates}` (the
  ontology is one history for the engine: the event names no Drive). `drive.freshness_changed` is `{drive_id, freshness}` and
  `Freshness.pending_review` (G12) is part of it. A review decision that leaves nothing to publish ends with a
  `drive.freshness_changed` for each Drive that was "publishing" only because of that job (the publish job ran, no version
  came), so a client waiting for "publishing" settles.
- Every `Page` answer carries `as_of_seq`: `GET /shares`, `GET /shares/{sid}/updates` and `GET /suggestions` did not.
- A source mark is `[src:<path>#<frag>]` (spec 10 6.2) in `Block.src`, `Evidence.src` and BRIEF; `POST /suggestions` evidence
  `ref` also accepts the same text without brackets (`src:<path>#<frag>`).
- No field or endpoint is added or removed; clients ignore unknown fields as before.

## 1.9 (D83, WP3b: full text search over facts and blocks)

- Version 1.8 is taken by the engine event shapes lane; this entry is 1.9 so the two do not collide.
- `GET /v1/docs/search?q&drive_id&limit` (`since` 1.9, `SearchResult`, `SearchHit`): PostgreSQL full text search (`simple`
  configuration, so it needs no language dictionary) over published facts and the blocks of live file versions. A CJK character is one
  token and a Chinese phrase is a phrase match, so the same query works for Chinese and English; words are prefixes and all terms must
  match. Each hit carries `evidence` (file, version, block, anchor), so a hit is never without its source. Order is `score` descending,
  then fact before block, then id (deterministic). `truncated` says when `limit` or a scan bound cut the answer. An `ooa_` token needs
  `docs:read` and a `drive_id` of its set (`all_drives` may omit it); an unknown drive is 403 as on `/entities`.
- `OntologySearchResult` items add the optional `kind` (`entity` | `fact` | `block`), `text`, `fact_key` and `predicate`; absent `kind`
  means `entity`, as in 1.7. Fact and block items keep their file anchor in `provenance`. No new error code. Additive.

## 1.10 (Desktop defects D1 and D2, lane H)

- `DriveCreate.config` also accepts `DriveConfigCloud` (one `secret_ref` to a JSON credential), the shape the engine already served for
  `s3`, `oss`, `cos` and `r2`. `DriveConfigS3` (two `{secret_ref}`: `access_key_id`, `secret_access_key`, optional `session_token`)
  is now served by the engine too, for all four backends. Before, the engine took only the single `secret_ref`, so a Desktop that
  followed the contract got 400. The two shapes cannot be mixed (400 at `/config/secret_ref`). Each reference of the pair resolves to the
  plain value (surrounding whitespace is dropped). `endpoint` is optional in `DriveConfigS3` (derived from `region` or `account_id`
  as for `DriveConfigCloud`), and it gains `account_id`, `internal`, `policy`. `GET /drives` returns the references as given.
- `Health.components.parser` is `unavailable` with `code` `DOCS_ENGINE_PARSER_UNAVAILABLE` while the configured doc-parser fails its
  `/healthz` (it was always `ok`); the engine `status` is then `degraded`. New optional components `ocr_parser` and `media_parser`
  (present only when those sidecars are configured) follow the same rule. New code `DOCS_ENGINE_PARSER_UNAVAILABLE` (no HTTP status;
  it appears in a health component only). Additive.

## 1.11 (Desktop findings, lane I)

- `POST /files:retry` also accepts a file parked as `unsupported` with `DOCS_PARSE_CODEC_UNAVAILABLE` (a recording whose codec the media
  sidecar cannot decode). That is the last step of the host decode hand-over (spec 10 17.2): the host writes the decoded WAV, frames and
  `manifest.json` under `<exchange>/<sha256 of the file>/`, then retries the file; the engine parses it again and reads the output.
  Before, the answer was `not_retryable` with `state:unsupported` and the file stayed parked. Every other `unsupported` file is still
  `not_retryable`. The file goes through `outdated` (the table's own edge) and not through `queued`; its old facts, if any, stay valid.
- Engine behaviour, no field: an object-store Drive (`s3`, `oss`, `cos`, `r2`) is polled for changes like a local one, every
  `sync.s3_poll_seconds` of `docs.json` (default 300; the key is not `object_poll_seconds`). Before, only `fs` Drives were polled, so a
  change in a bucket showed only after an explicit `POST /drives/{id}:sync`.

## 1.12 (lane L, ontology schema)

- `GET /v1/ontology/schema` (`since` 1.12; `oo ontology schema`): the database side of the unified ontology, so a client no longer reads
  `information_schema` through `POST /v1/ontology/query` (the Gateway SQL guard refuses it, sql-guard-v1 R5). Answer `OntologySchema`:
  `tables` with `columns` (name, position, type) from the Gateway's `/v1/tables` and `/v1/tables/{t}` under the token's profile,
  `relationships` from the role pack's `relationships/graph.json` (declared foreign keys and reviewed relations, each with `from`/`to` table
  and columns, cardinality, provenance), and `doc_links` (document entity to row, from the engine's link routes, only with `docs:read`).
  Needs `db:read` and a Gateway profile (403 `ONTOLOGY_SCOPE_DENIED` otherwise); `limit` 1..=100 (default 50) and the opaque `cursor`
  page the sorted table list (400 `ONTOLOGY_BAD_REQUEST` for a bad value); every relationship and link comes once, in the page of its
  `from` table. New URNs: table `oo:db/<profile>/<schema.table>`, column `<table urn>#<column>`, relationship
  `oo:db/<profile>/relationship/<id>`. No new error code. Additive.

## 1.13 (lane AH, live CLI changes in the app)

- `Event.actor` (optional object `{kind, name?}`, top level next to `type`): who caused the event. `kind` is `cli`, `app`, `mcp`, `agent`,
  `cloud` or `local`. The engine derives it per request: an Agent token gives `{kind: agent, name: <token name>}`; a cloud token gives `cloud`;
  the control token with an `OO-Docs-Agent` header gives `{kind: agent, name: <header>}`; the control token otherwise gives the client named
  by the new request header. Events that a background worker appends later for the same file or Drive (a reconcile after `POST /drives/{id}:sync`,
  parse jobs after an upload, a publish) carry the actor of the request that caused them, remembered for 10 minutes (at most 4096 keys); an event
  with no known request has no `actor`. The engine stores the actor in the event payload and lifts it out of `data`, so `data` is unchanged.
  `GET /events` replays it like any other field.
- Request header `OO-Docs-Client: cli|app|mcp` (anything else or absent is `local`). The `oo` CLI always sends `cli`; the app sends `app`.
- `POST /v1/docs/events:publish` (`since` 1.13, control token only; Agent tokens, cloud tokens and refs credentials get 403): body
  `EventPublishRequest` `{type, data}`, closed; `type` is `db.source_changed` or `db.pipeline_changed`; `data` is an object of at most 4 KiB
  with no credential keys and no `actor` key (400 otherwise). Answer `{seq}`. The event has no Drive and carries the actor of the request.
- Two new persistent event types, published by the CLI through that route when a database source or the pipeline changes (`events.v1.schema.json`
  `x-data`): `db.source_changed` `{name, action: added|removed|initialized, stack}` and `db.pipeline_changed` `{command: up|down|build|init, ok,
  stack, profile?, published?}`; `stack` is an absolute path, `published` is true only when `oo build` ended complete with a published
  publication. No new error code. Additive.
