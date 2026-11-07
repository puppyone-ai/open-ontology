## Card D-S0S1 — lane D (PostgreSQL family): engine adapter, chunking, source protection

TODO: S0-D-01, S1-D-01. Lane label: `d`. Spec: REQ-ENG-01/02, AT-ENG-01.

Read: docs/product/02-ingestion-playbook.md, docs/product/05-version-matrix-and-testing.md,
`crates/oo-connectors/src/engine/` (interface, do not modify), the existing PostgreSQL code at the root of
`crates/oo-connectors/src` (session.rs, snapshot.rs, key_plan.rs, bounded_read.rs, catalog.rs, config.rs,
access.rs) and tools/postgres-preview/ (CONTRACT.md, CURRENT-CAPTURE.md, src/).

Deliver:
1. `crates/oo-connectors/src/postgres/` implementing `SourceEngine` by wrapping the existing root PG code
   (keep existing public APIs compiling). Move reusable parts of tools/postgres-preview into the product
   crate where they belong; keep the preview tool building or document what it superseded.
2. probe(): `VersionRule` table PG 12–18 (pgoutput v1 baseline; publish_via_partition_root 13+; proto v2
   streaming 14+, v3 two-phase 15+, v4 parallel 16+; logical decoding on standby 16+; failover slots 17+;
   idle_replication_slot_timeout and non-replicated VIRTUAL generated columns 18). Requirements
   (wal_level=logical, REPLICATION attribute, SELECT grants, publication) with DBA statements; 12 noted EOL.
3. plan_chunks(): keyset compared by PG under the column collation, composite row comparison
   EXPLAIN-verified; no-key: ctid TID-range chunks inside one exported snapshot on 14+ (never a cross-cycle
   checkpoint), SingleRead on 12/13; estimates from reltuples/EXPLAIN.
4. read_chunk(): reuse snapshot/portal machinery, Arrow to ChunkSink, sha256, watermark
   pg_current_wal_lsn() (pg_last_wal_replay_lsn() on standby). watermark(), digest_range().
5. Source protection: statement_timeout + client timeout, one connection per task, duty cycle, pause on
   active-session threshold, short per-chunk transactions, no locks/objects.
6. Tests: unit tests; real engines `postgres:13…18` bookworm (pin patch tags) on a synthetic fixture under
   testing/engines/postgres/ (int PK, text PK non-C collation edges, composite, unique-only, no-key,
   ≥2,000,000 rows, growing table). T1 per version under a concurrent writer: zero diff. Record T0/T1/T8.

Write scope: crates/oo-connectors/src/postgres/** (new), the root PG modules of crates/oo-connectors/src only
as needed for the adapter, crates/oo-connectors/tests/postgres_*, tools/postgres-preview/**,
testing/engines/postgres/** (except seed/). Lock/manifest only if unavoidable (report). Prefix `connectors/postgres`.
Test commands: `cargo test -p oo-connectors --lib` plus your real-engine entry.
