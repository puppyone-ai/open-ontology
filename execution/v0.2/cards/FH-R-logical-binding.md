## Card FH-R — publication binding on logical identity (S6-B-02; D23)

Branch `v0.2/fh-r` from `1f267b6f5b95180139196fbb90c5b2e716d14699`. Commit as the owner (no agent trailers), trailer `Plan: S6-B-02`. Read plan §4 row
S6-B-02, §5 T9-10, D23. Your files: `crates/oo-gateway/**`, pipeline migrations numbered **0135–0139 only** (if the
binding needs target-side state), new `tools/oo-test/t9_restore.py`. Do NOT edit `tools/oo-load/**`, other
`crates/oo-pipeline/**` source, `deploy/lite/**`, `crates/oo-cli/**`, `execution/**`, `specs/**`.

Defect: `crates/oo-gateway/src/publication_binding.rs` (`receipt_in_snapshot`, ~line 380) binds publications to table
OIDs, `relfilenode` and type OIDs, so a logical backup/restore (`pg_dump`/`pg_restore`) or a migration invalidates every
publication with PUBLICATION_DRIFT.

Build: bind on logical identity instead — schema-qualified relation name, relkind, ordered column definitions
(attnum order, attname, `format_type(atttypid, atttypmod)`, attnotnull), normalized view definitions, plus an identity
record written by the publisher (publication id, generation, relation identity digest) in the target `_oo` schema that
only the publisher role can write. Threat model first (write it in the report): the current OID binding stops an
attacker or a mistake from swapping a published table/view, changing columns, or redirecting a view; show for each
attack how the new binding still refuses it (including: drop + recreate same name with different data, rename swap of
two tables, view redefinition, column type/typmod change, a non-publisher role creating a lookalike). Keep the
`receipt_in_snapshot` API and error codes; old receipts: define a one-time upgrade path or a clear refusal with a
documented re-publish step. Lane P adds per-table freshness state `stale` + reason in `_oo.table_freshness` (column
names in its report; read defensively): surface it in query warnings and the freshness endpoint so an Agent sees which
tables are stale and why.
Tests: all existing gateway tests; new negative tests for every attack above; `t9_restore.py` T9-10 live: publish on
PostgreSQL 17, `pg_dump` + `pg_restore` the target into a fresh cluster, Gateway queries succeed; then each tamper
case is refused. Also a physical `pg_basebackup` restore still works. Run `cargo test --locked -p oo-gateway`,
clippy -D warnings, fmt. List files bound in `supply-chain/secret-review.json` that you changed. Report: commits,
threat table, tests with counts, live results, gaps.
