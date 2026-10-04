# metrics

Only certified metrics with a visible dependency set are included. Reconciliation is not executed and is not claimed.

## count_metric — 计数

status: `active`
unit: kg
default anchor: `month_start`
> Reconciliation status is not in this pack. control_total was not executed and is not claimed to match.

```text
可见秤计数
```

```sql
SELECT count(*) AS n FROM dock.scale
```
