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
