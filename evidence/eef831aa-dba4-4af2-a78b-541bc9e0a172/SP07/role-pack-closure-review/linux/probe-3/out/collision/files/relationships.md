# relationships

A relationship is included only when every endpoint column is on the allowlist and the claim is approved, source-declared, or explicitly include_unreviewed. Containment and query-log counts are never copied. Name match and type compatibility are included only when both ends allow global statistics.

## hold_bin

`dock.parcel.parcel_id` -> `dock.scale.scale_id`
cardinality: `many_to_one`
join: `left`
Provenance: declared_fk (source-declared, not a grant).
