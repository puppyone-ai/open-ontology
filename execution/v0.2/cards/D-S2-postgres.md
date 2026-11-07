## Card D-S2 — lane D (PostgreSQL): logical replication C1, C5, slot lifecycle

TODO: S2-D-01. Lane label: `d`. Spec: REQ-ENG-03, AT-ENG-01. Playbook docs/product/02 §5.
Builds on your S0/S1 work on your lane branch; rebase on v0.2/product-loops first.

Deliver: `ChangeSource` for PostgreSQL via pgoutput — preflight of slot/publication existence and
privileges (slots and publications are DBA-created R1 objects; the product never creates them unless the
existing docs allow it — check docs/database-support/03 write classification), protocol version
negotiation by server version (v1–v4), publish_via_partition_root handling, earliest_available() from
the slot's restart_lsn/confirmed_flush_lsn, PG 18 idle_replication_slot_timeout invalidation detection →
explicit rebaseline (never silently recreate a slot), WAL retention preflight. Chunk watermarks with
pg_current_wal_lsn and window correction as in the playbook; C5 digests through digest_range with the
four layers; no-change cycle = zero payload / rewrite / generation. Reuse tools/postgres-preview and
crates/oo-pipeline pg_current* code; don't duplicate.
Tests per version 13–18: T1 with window correction; T2 C1 + C5 three cycles incl. no-change; slot
invalidation (18) → explicit rebaseline. Use lane H's oo-test/oracle if integrated.
Write scope: crates/oo-connectors/src/postgres/**, root PG modules only as needed, crates/oo-connectors/tests/postgres_*,
tools/postgres-preview/**, testing/engines/postgres/** (not seed/). Prefix `connectors/postgres`.
