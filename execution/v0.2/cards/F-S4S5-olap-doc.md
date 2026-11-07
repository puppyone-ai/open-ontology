## Card F-S4S5 — lane F: P1 ClickHouse, Doris, StarRocks, MongoDB, OpenSearch/Elasticsearch

TODO: S4-F-01, S5-F-02. Lane label: `f`. Spec: REQ-ENG-01/04, REQ-SRC-10 (C4 partition refresh, C2 change streams).
Read docs/database-support/04 (OLAP and document sections), tools/{clickhouse,doris,starrocks,mongodb,opensearch}-preview
(existing preview work with evidence — converge, don't restart).
Deliver:
1. S4-F-01 ClickHouse 24.x LTS + 26.x, Doris 3.x + 4.x (`apache/doris` fe/be ≥4.1.5 for arm64), StarRocks 3.x
   (`starrocks/allin1-ubuntu`): SourceEngine per family (own VersionRule tables), snapshot chunking suited to
   columnar engines (partition/part ranges), C4 partition refresh where partitions exist, C5 otherwise;
   column default-for-missing semantics flagged (`default_for_missing`). T0–T2 per version.
2. S5-F-02 MongoDB 7.0 / 8.0 (replica set single node for change streams): document → stable projected
   columns + doc column per docs, change stream C2 with resume tokens as Watermark::Opaque, delete by key;
   OpenSearch 2.x/3.x and Elasticsearch 8.x: snapshot via scroll/PIT, no CDC (Unsupported with reason).
   T0–T2 (Mongo), T0–T1 (search engines). Check licenses: Elasticsearch default distribution is not in the
   default artifact (ELv2/SSPL rules in supply-chain/policy.md) — client-side only, record it.
Write scope: crates/oo-connectors/src/{clickhouse,doris,starrocks,mongodb,opensearch}/** (or a sibling crate if
the drivers must stay out of oo-connectors — say why), tests, testing/engines/<family>/**, the five tools/*-preview
dirs. Lock changes reported. Prefix `connectors/<family>`.
