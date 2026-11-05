## Card F-S5 — lane F: MongoDB change streams, OpenSearch / Elasticsearch snapshots

TODO: S5-F-02 (plan exit T0–T2). Works ahead of the S4 exit; integration after it. Worktree `v0.2/f-olap` (lane F's
OLAP work is integrated; rebase on v0.2/product-loops first). Converge `tools/mongodb-preview` and
`tools/opensearch-preview` into SourceEngine implementations (feature-gated modules under `crates/oo-connectors/src/`,
own oo-load module + one-line dispatch, matrix entry in `tools/oo-test`, pins in lane H's format), following your OLAP
structure. Granted: the `lib.rs` export lines and a feature in crates/oo-connectors/Cargo.toml; Cargo.lock only if
unavoidable (then cargo deny + cargo audit).
1. MongoDB (one current version, replica set of one for change streams): snapshot with `_id`-ordered bounded chunks;
   C1 via change streams with a durable resume token + source_epoch (replica set id / cluster time), C5 digests;
   documents map to the Agent DB as typed columns for top-level scalar fields plus a jsonb document column.
   T0, T1 (concurrent writer), T2 (three C1 + three C5 cycles; resume-token expiry → explicit rebaseline), T8.
2. OpenSearch (one current 2.x/3.x) and Elasticsearch (one current 8.x/9.x, security on): point-in-time + search_after
   snapshots, C5 digests per index/shard range; no log CDC. T0, T1, T2 (C5), T8.
Do not run privileged containers that run systemd or change binfmt/sysctl.
Resume note (E314): your first run was stopped by the host disk guard (E269). The worktree keeps your committed
c1172b1bf and uncommitted tools/oo-load changes — review them, commit what is correct, then rebase on
v0.2/product-loops (it moved a lot: typed Oracle, negative-scale decimals, SQL Server/OLAP arms in tools/oo-load).
Dispatch now deletes every Cargo target dir after a run: build what you need first.
Resume note 2 (E342): disk is no longer a constraint (≈490 GB free). D18 allows CC0-1.0 for tiny-keccak (ci/deny.toml).
Rebase, apply your `testing/engines/document/coordinator.patch` items you need to your own files only where they are
yours; shared ones (pipeline jsonb mapping, comparator hook) — implement them now as separate commits with tests (granted
for this card: crates/oo-pipeline/src/engine_load_pg.rs jsonb mapping and tools/oo-test comparator hook). Then finish
T0/T1/T2/T8 for MongoDB, OpenSearch and Elasticsearch with explicit verdict rows (tools/oo-test/verdicts.py).
