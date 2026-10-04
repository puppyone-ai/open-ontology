# metrics

Only certified metrics with a visible dependency set are included. Reconciliation is not executed and is not claimed.

## scale_count — 秤台计数

status: `active`
unit: kg
default anchor: `month_start`
> Reconciliation status is not in this pack. control_total was not executed and is not claimed to match.

```text
visible scale count
```

```sql
SELECT count(*) AS scale_n FROM wharf.scale
```
