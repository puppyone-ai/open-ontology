# docs-api changelog

Contract between the Open Ontology docs engine (`oo serve`) and Puppyone Desktop Super Drive.
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
