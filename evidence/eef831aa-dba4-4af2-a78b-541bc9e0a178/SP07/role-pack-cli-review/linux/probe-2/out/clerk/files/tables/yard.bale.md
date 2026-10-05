# yard.bale — 货包

tier: `file`
grain: one row per bale
primary key: `bale_id`

Row security: rls. Cross-row samples and most_common values are omitted. Global statistics are omitted. This compiler did not re-prove row security.
Estimated row count is omitted.

| column | label | type | semantic | pii | notes |
| --- | --- | --- | --- | --- | --- |
| `bale_id` | 包号 | bigint |  | none | 本季到港重量 |
| `mass_kg` | 净重 |  |  | none | unit kg |
| `badge` | 工牌 |  |  | personal | Samples are omitted because pii is personal. badge note stays |

## relationships

None.
