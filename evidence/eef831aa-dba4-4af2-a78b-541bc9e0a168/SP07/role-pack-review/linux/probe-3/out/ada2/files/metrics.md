# metrics

Only certified metrics with a visible dependency set are included. Reconciliation is not executed and is not claimed.

## quoted_mix — quoted_mix

status: `active`
unit: kg
default anchor: `month_start`
> Reconciliation status is not in this pack. control_total was not executed and is not claimed to match.

```text
客户备注 remains visible in this sentence
```

```sql
SELECT weight_kg AS "客户备注" FROM depot.shipment
UNION ALL
SELECT weight_kg FROM "yard"."slip"
```

## weight_total — weight_total

status: `active`
unit: kg
default anchor: `month_start`
> Unconfirmed codes: depot.shipment.status has undetermined or unexplained values. Those codes count in no class.

> Reconciliation status is not in this pack. control_total was not executed and is not claimed to match.

```text
sum of visible weight
```

```sql
SELECT sum(weight_kg) AS total FROM depot.shipment
```
