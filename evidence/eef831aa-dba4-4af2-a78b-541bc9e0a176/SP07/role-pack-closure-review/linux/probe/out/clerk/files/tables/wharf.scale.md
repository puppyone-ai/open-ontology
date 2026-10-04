# wharf.scale — 秤台

tier: `file`
grain: one row per scale
primary key: `scale_id`

Row security: rls. Cross-row samples and most_common values are omitted. The snapshot marks global statistics as proven (proof-scale-a176). This compiler did not re-prove that claim.
estimated rows: 23

| column | label | type | semantic | pii | notes |
| --- | --- | --- | --- | --- | --- |
| `scale_id` |  |  |  | none |  |
| `mass_kg` |  |  |  | none | unit kg |

## relationships

- `crate_scale` wharf.crate.mass_kg -> wharf.scale.mass_kg
