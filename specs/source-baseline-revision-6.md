# Source baseline revision 6: v0.2 design backfill and docs/product scope

Date: 2026-11-04 (Asia/Shanghai).

## Authority and provenance

The user directed on 2026-11-03 that docs, check scripts and the source
baseline may be changed directly, and that v0.2 design changes be written back
into the original documents in the same period rather than deferred
([decision log](../execution/v0.2/decisions.md), Plan D7). The coordinator
selected and reviewed the edits. This is not an independent review or product
acceptance.

The input commit is `26f11610162ba926396af90095f166122010386b`. The source set now covers `docs/*.md` (19) and
`docs/product/*.md` (7), 26 files in total. No document was edited to make a
hash pass; the manifest was generated from that commit's bytes.

The exact prior manifest is preserved as
[sources-baseline-revision-5.json](sources-baseline-revision-5.json), SHA-256
`5cfcc7a39e8c01a0f4e87f593a473229a39a62974aef04df1fafca9c3bb08e7b`.
Revision 5's record, hashes and history are unchanged.

## Reviewed delta

| Area | Change |
|---|---|
| docs/product (new) | Working model, ingestion playbook, Self-build context budget, onboarding and Gateway binding, version matrix and testing, index |
| docs 03, 04, database-support 03/04/05 | Key-shape chunking with source-side collation compare; distribution factor and sampling; growth-chunk split; read-only watermarks; C5 four layers; C5→C1 multi-source handoff; retention preflight from the actual earliest log; version differences MySQL 5.7–9.x, PostgreSQL 12–18, Oracle 11g–23ai; Oracle LogMiner through online_catalog only (R1 by design, pending source audit) |
| docs 06 | Budgeter, NDJSON repair, community batching, three-pass generation, entity registry, connectivity gate G14; no project-level table cap |
| docs 07, 08 | Service-account token issue/renew/rotate/revoke/enroll, self-renew, device code, expiry monitoring, `/v1/meta`; `oo login`, `whoami`, `share-prompt`, admin commands |
| docs 11, 12 | lite profile as open-source default alongside standard; `oo.stack/v1` |
| docs 13, 14, README | Multi-engine fixtures, 20 gold questions, T0–T8; v0.2 roadmap and risks R17/R18; reading table |

## Spec impact

- New chapter 7B: REQ-ONB-01–06, REQ-TOK-01–05, REQ-BLD-01–04, REQ-ENG-01–04
  (19 requirements) and AT-ONB-01–04, AT-TOK-01–03, AT-BLD-01–02,
  AT-ENG-01–03 (12 acceptance cases), each traced to an existing SP phase and
  a v0.2 sprint. Totals become 107 requirements and 117 acceptance cases.
- REQ-SRC-12 item 4 changes the string-key chunking rule from forced binary
  collation to source-side comparison under the column's own collation with
  `EXPLAIN`-verified index use; AT-SRC-04, AT-SRC-11, DBP8 and DEC-21 text
  follow. Forced binary collation loses the index and sorts whole tables,
  which a production deployment hit on million-row `varchar` keys.
- No invariant, phase, work item or prior REQ/AT is added or removed outside
  chapter 7B.

## Script changes

`check_spec.py` reads the expected counts (107/117) and the source
directories (`docs`, `docs/product`) from constants instead of hard-coded
19/88/105. `check_v02_plan.py` is new and checks the v0.2 plan structure and
document hygiene. All three checkers keep their mutation self-tests.
