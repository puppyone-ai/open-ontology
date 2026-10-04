# dock.scale — Scale

tier: `file`
grain: one row per scale
primary key: `scale_id`

Row security: rls. Cross-row samples and most_common values are omitted. The snapshot marks global statistics as proven (proof-scale-a172). This compiler did not re-prove that claim.
estimated rows: 19

| column | label | type | semantic | pii | notes |
| --- | --- | --- | --- | --- | --- |
| `scale_id` |  |  |  | none |  |
| `mass_kg` |  | numeric |  | none | unit kg |

## relationships

- `declared_link` dock.parcel.mass_kg -> dock.scale.mass_kg
- `parcel_scale` dock.parcel.mass_kg -> dock.scale.mass_kg
