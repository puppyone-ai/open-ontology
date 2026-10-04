# dock.scale — Scale

tier: `file`
grain: one row per scale
primary key: `scale_id`

Row security: none. This compiler did not query the database.
Estimated row count is omitted.

| column | label | type | semantic | pii | notes |
| --- | --- | --- | --- | --- | --- |
| `scale_id` |  |  |  | none |  |
| `mass_kg` |  | numeric |  | none | unit kg |

## relationships

- `hold_bin` dock.parcel.parcel_id -> dock.scale.scale_id
