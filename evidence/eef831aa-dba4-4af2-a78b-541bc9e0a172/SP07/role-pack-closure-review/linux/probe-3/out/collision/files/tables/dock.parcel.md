# dock.parcel — 包裹

tier: `file`
grain: one row per parcel
source label: 码头副本
primary key: `parcel_id`

Row security: none. This compiler did not query the database.
Estimated row count is omitted.

| column | label | type | semantic | pii | notes |
| --- | --- | --- | --- | --- | --- |
| `parcel_id` | 包裹号 | bigint |  | none | 本季到港重量 |
| `mass_kg` |  |  |  | none |  |
| `badge` |  |  | phone | personal | Samples are omitted because pii is personal. 工牌不进样本 |
| `status` |  |  |  | none | enum `dock.parcel.status` |
| `mark_ok` |  |  |  | none | 可见标记说明 |

## relationships

- `hold_bin` dock.parcel.parcel_id -> dock.scale.scale_id
