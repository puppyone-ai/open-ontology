# wharf.crate — 货箱

tier: `file`
grain: one row per crate
primary key: `crate_id`

Row security: rls. Cross-row samples and most_common values are omitted. Global statistics are omitted. This compiler did not re-prove row security.
Estimated row count is omitted.

| column | label | type | semantic | pii | notes |
| --- | --- | --- | --- | --- | --- |
| `crate_id` | 箱号 | bigint |  | none | 本季到港重量 |
| `mass_kg` | 净重 |  | money | none |  |
| `badge` |  |  |  | personal | Samples are omitted because pii is personal. |
| `mark_ok` |  |  |  | none | 可见标记说明 |
| `status` |  |  |  | none | enum `wharf.crate.status` |

## relationships

- `crate_scale` wharf.crate.mass_kg -> wharf.scale.mass_kg
