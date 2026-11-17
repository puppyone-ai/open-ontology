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
