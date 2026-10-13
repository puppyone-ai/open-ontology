# Pending OpenSearch read contract

Frozen base `577160c22de63adb9b38e0147f3bc2da893c4d9d` exposes PostgreSQL only:

- `oo_connectors::types::TYPE_CONVERSION_PROTOCOL` = `oo.source.pgsql.types/v1`
- `TypedBatch` / `ArrowColumn` / `DecodedCell` are built from PostgreSQL column plans
- `SourceCatalog.protocol` for that crate is `oo.source.pgsql/v1`

No OpenSearch document protocol, PIT batch type, or Arrow consumer is in that commit. This preparation does not invent a substitute implementation and does not claim `oo.source.pgsql.types/v1` can read an index. DBX-W03 and DBX-W05 are not consumed: their batch workers have not delivered an immutable SHA to this task.

## Proposed API, not implemented

Protocol id: `oo.source.opensearch.batch/v1`.

Observed on this pin: `version.distribution` is `opensearch`, `version.number` is `3.9.0`, `lucene_version` is `10.5.1`, `minimum_wire_compatibility_version` is `2.19.0`, and `minimum_index_compatibility_version` is `2.0.0`. Those compatibility numbers are fields of this OpenSearch node. Admission requires `distribution=opensearch` and the pinned number.

```text
product: OpenSearch
version: pinned tested version
image_digest: sha256
license: Apache-2.0
index: owned index name
setup_principal: distinct from reader_principal
consistency_grade: B
capture_method: C5
delete_detection: reconcile
token_order: None
pit_id: opaque read cursor, not a CDC position
page_size: int
sort: one unique field order
rows: stored _source objects
presence: empty | missing | null | numeric | text | multivalued
```

Proposed Arrow mapping for a later shared contract owner:

| OpenSearch mapping | Proposed Arrow | Rule |
|---|---|---|
| keyword | utf8 | exact stored value |
| text | utf8 | use `_source` or the keyword subfield, not analyzed tokens |
| long | int64 | including the int64 min and max in this fixture |
| double | float64 | exact JSON number; no silent decimal claim |
| scaled_float | decimal128 or unscaled int64 | scale is `scaling_factor`; do not use float64 |
| multi-value | list of the child type | empty array stays an empty list |
| JSON null | null presence | key present |
| absent key | missing | not null and not empty string |
| whole `_source` | utf8 `doc` | fidelity column beside projections |

PIT cleanup is `DELETE /_search/point_in_time` with that pit id. Cancel is `POST /_tasks/{id}/_cancel` on the in-flight search. Neither is a Rust API yet.

The executable proof for this card is the Python/HTTP journey in `scripts/journey.py` against the pinned image. A Rust consumer stays pending until a coordinator-pinned contract exists.
