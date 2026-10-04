# depot.shipment — Shipment

tier: `file`
grain: one row per shipment
primary key: `shipment_id`

Row security: rls. Cross-row samples and most_common values are omitted. Global statistics are omitted. This compiler did not re-prove row security.
Estimated row count is omitted.

| column | label | type | semantic | pii | notes |
| --- | --- | --- | --- | --- | --- |
| `shipment_id` |  |  |  | none |  |
| `weight_kg` |  | numeric queue_note |  | none | unit queue_note 称重后对照 "yard"."slip" 的客户备注 |
| `handle` |  |  | phone | personal | Samples are omitted because pii is personal. classified handle |
| `note_ascii` |  |  |  | none |  |
| `note_quoted` |  |  |  | none |  |
| `status` |  |  |  | none | enum `depot.shipment.status` |

## relationships

- `declared_link` depot.shipment.weight_kg -> depot.tally.weight_kg
- `shipment_tally` depot.shipment.weight_kg -> depot.tally.weight_kg
