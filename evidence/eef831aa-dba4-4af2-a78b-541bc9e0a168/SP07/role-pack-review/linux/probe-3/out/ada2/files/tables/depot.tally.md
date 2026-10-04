# depot.tally — Tally

tier: `file`
grain: one row per tally
primary key: `tally_id`

Row security: rls. Cross-row samples and most_common values are omitted. The snapshot marks global statistics as proven (proof-tally-1). This compiler did not re-prove that claim.
estimated rows: 17

| column | label | type | semantic | pii | notes |
| --- | --- | --- | --- | --- | --- |
| `tally_id` |  |  |  | none |  |
| `weight_kg` |  |  |  | none |  |

## relationships

- `declared_link` depot.shipment.weight_kg -> depot.tally.weight_kg
- `shipment_tally` depot.shipment.weight_kg -> depot.tally.weight_kg
