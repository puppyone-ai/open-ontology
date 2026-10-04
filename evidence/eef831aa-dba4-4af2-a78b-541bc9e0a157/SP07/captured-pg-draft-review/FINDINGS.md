# Findings

Verdict: **APPROVE LIMITED**.

## What this capture proves

`compile_captured_pg_catalog` accepts a read-only PostgreSQL 17.10 `PgSession::catalog` document for `ledger.party` and `ledger.invoice` and seals an evidence envelope plus a structure draft. The generated relationship `invoice_party_fk` is `many_to_one`, `declared_fk`, status `draft`, confidence null, join hint `left`. Email stays `text` with null PII and null semantic type. Both queue items `pii/ledger.party/email` / `pii_unknown` and `semantic/ledger.party/email` / `semantic_unknown` are pending and not publishable.

Reordering tables, columns, and privileges keeps the sealed bytes. Swapping primary-key member order changes the schema hash and is rejected as `hash.schema` when the old hash is left in place. `type_support.converted: true` is `type.converted`. A comment body field is `field.unknown`. A duplicate JSON key is `json.duplicate_key`.

Adding `ledger.party.note` changes the schema hash from `a0efbfc8b5bc7536a121208a1c8e3ff41b16cdda4018b5618b047f40d415271c` to `c123519256e1c88ab063a41f250ac02483822983d3cf95c364d25983e86fde4d`. The prior email accept stays a pending `stale_decision` with detail `stale_schema_hash`. The curated echo keeps label `Correspondence` and PII `sensitive`. The effective label stays `email` and effective PII stays null.

A fresh reject of `column/ledger.party/id` removes `invoice_party_fk` from the effective layer, keeps it in the generated layer, and queues `relationship_endpoint_not_effective` with detail `to_column_not_effective`. Generated and effective layers passed `oo_core::oos::validate_dataset` and `validate_relationships`.

The draft provenance stays `pg_catalog_integration=not_accepted` and commit `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74` (`draft.rs:2381-2382`, checked by `captured_pg.rs:177-207`). The wrapper stays `live_catalog_review=pending`, `row_import=not_proven`, `grants=not_authorized` (`captured_pg.rs:147-161`) after a fresh accept and after the stale decision. Reader `oo_a157_reader` executed 32 extended-protocol statements and 2 simple statements, with 0 data writes.

Hash bindings are the existing `config_hash`, `type_hash`, and `evidence_hash` functions. `selection_from_catalog` repeats the set that `check_selection` compares. A mismatch fails closed inside `compile_structure_draft`.

## What stays open

Row import is `not_proven`. Grants are `not_authorized`. Publication is `unavailable`. Certification is `none`. No model was called. The builder field `source_connection=not_used` describes the compiler. The live catalog read happened in the evidence probe through `oo-connectors`, outside the builder crate.

a086 findings 3 and 4 and a094 findings 1, 2, and 5 stay the previously approved slices. This diff does not edit `apply_reviews`, `relationship_gap`, or `nfc.rs`. This review did not rerun the 118-case closure matrix or the Unicode campaign.

SP07 is open. This verdict does not register a new approval from `type_support.converted`.

## Author process deviation

`evidence/.../a156/.../pg/audit.py` is Python. It remains as the author wrote it. The a157 SQL audit is `probe/src/bin/sql_audit.rs`.

## Failures kept

`probe/attempt-1` cargo exit 101: the snapshot mount was read-only, so Cargo could not update the snapshot lock. The rerun mounted the snapshot copy writable, added only package `a157-captured-pg-review` to that copy, and left the repository `Cargo.lock` at blob `ef30377cade0ad07ba4192125f434b01452a5622`.
