# metrics

Only certified metrics with a visible dependency set are included. Reconciliation is not executed and is not claimed.

## kept_mass — 净重合计

status: `active`
default anchor: `month_start`
> Reconciliation status is not in this pack. control_total was not executed and is not claimed to match.

```text
visible mass
```

```sql
SELECT mass_kg AS total FROM yard.bale
```
