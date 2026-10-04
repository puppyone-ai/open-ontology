# dock.parcel — 包裹

tier: `file`
grain: one row per parcel
source label: 码头副本
primary key: `parcel_id`

Row security: rls. Cross-row samples and most_common values are omitted. Global statistics are omitted. This compiler did not re-prove row security.
Estimated row count is omitted.

| column | label | type | semantic | pii | notes |
| --- | --- | --- | --- | --- | --- |
| `parcel_id` | 包裹号 | bigint |  | none | 本季到港重量 |
| `mass_kg` |  |  |  | none |  |
| `badge` |  |  | phone | personal | Samples are omitted because pii is personal. 工牌不进样本 |
| `status` |  |  |  | none | enum `dock.parcel.status` |
| `mark_ok` |  |  |  | none | 可见标记说明 |

## relationships

- `declared_link` dock.parcel.mass_kg -> dock.scale.mass_kg
- `parcel_scale` dock.parcel.mass_kg -> dock.scale.mass_kg
