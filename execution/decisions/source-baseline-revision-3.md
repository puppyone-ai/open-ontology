# Source baseline revision 3 — database support expansion backfill

Status: approved by the project owner on 2026-10-09 (stated in the owner conversation: baseline edits permitted, decisions made, review passed). There is no separate reviewer sign-off file; the approval is recorded here by the author.

Fixed source commit: `b504fd8f4369750f6a329075c77dcc4bf3720702` (docs only). Previous baseline HEAD `34fae5ddd73ff250fde6f51aa681fb0f7fbd0362`; the revision 2 baseline JSON SHA-256 is kept in `supersedes_sha256_rev2` of `specs/sources-baseline.json`.

## Owner decisions

1. Connector-writable helper objects (Debezium signaling/heartbeat/flush tables and similar) are not allowed by default. They can only be enabled as class R1w after a least-privilege ADR and source-audit proof of the write scope. This narrows the old I-SRC-1 exception in `docs/03`, which had allowed a DBA-created signaling table.
2. `consistency: transactional` is valid only for T-grade CDC sources. Other sources must declare S1/S0/B and `oo validate` rejects `transactional` for them.
3. Baseline documents may be edited to match the extension design in `docs/database-support/`.

## Changed baseline documents

`docs/README.md` (index row), `docs/02` (ADR-014), `docs/03` (I-SRC-1 exception, authority notes in sections 1/2/8, structured capabilities, binary chunk ordering), `docs/04` (I-PL-6, section 5.2 scope, quality states in 5.7), `docs/05` (consistency_grade and new `_oo.table_quality` columns), `docs/06` (optional OOS `source_semantics`), `docs/07` (new quality warnings and freshness fields), `docs/12` (target/driver/budget/delete_detection, `sync` values, `snapshot` and `consistency` scope), `docs/14` (M3 database expansion pointer), `docs/16` (section 5A).

Unchanged: REQ-SRC-01 to REQ-SRC-07 semantics, I-SRC-2 to I-SRC-4, existing PG/MySQL CDC and batch behaviour, the other baseline documents (00, 01, 08, 09, 10, 11, 13, 15, 17). Documents 08 to 11 and 13 were not re-checked line by line for stale database-scope wording; this is recorded as open question Q10 in `docs/database-support/06`.

## Spec and Plan effect

Spec chapter 7A adds REQ-SRC-08 to 15 and AT-SRC-06 to 14 (88 requirements, 105 acceptance cases); the 87 work items and SP00 to SP15 exit criteria are unchanged. The DBX track in Plan chapter 4A has its own denominator.

## Pins elsewhere that now differ (owned by other work lines, not edited here)

`tools/ontology-lifecycle/tests/oracle.rs`, `tools/ontology-lifecycle/REQUIREMENTS-MAP.json`, `specs/frontend/source-baseline.md` (records an earlier read). Their owners must re-read the changed documents before re-pinning.
