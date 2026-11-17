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
