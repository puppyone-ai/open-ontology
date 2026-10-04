# metrics

Only certified metrics with a visible dependency set are included. Reconciliation is not executed and is not claimed.

## kept_mass — 可见重量

status: `active`
unit: kg
default anchor: `month_start`
> Unconfirmed codes: wharf.crate.status has undetermined or unexplained values. Those codes count in no class.

> Reconciliation status is not in this pack. control_total was not executed and is not claimed to match.

```text
visible mass
```

```sql
SELECT sum(mass_kg) AS total_kg FROM wharf.crate
```

## quoted_distinct — 引号保留字

status: `active`
unit: kg
default anchor: `month_start`
> Reconciliation status is not in this pack. control_total was not executed and is not claimed to match.

```text
quoted identifier
```

```sql
SELECT mass_kg AS "DISTINCT" FROM wharf.crate
```

## quoted_metric — 引号重量

status: `active`
unit: kg
default anchor: `month_start`
> Reconciliation status is not in this pack. control_total was not executed and is not claimed to match.

```text
本季到港只计可见列，净重以公斤计。
```

```sql
SELECT mass_kg AS "净""重" FROM "wharf" . "crate"
```

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

## string_metric — 可见字符串

status: `active`
unit: kg
default anchor: `month_start`
> Reconciliation status is not in this pack. control_total was not executed and is not claimed to match.

```text
可见字符串，撇号仍按原文保留。
```

```sql
SELECT 'it''s visible' AS label_ok FROM wharf.crate
```

## where_metric — 条件重量

status: `active`
unit: kg
default anchor: `month_start`
> Reconciliation status is not in this pack. control_total was not executed and is not claimed to match.

```text
visible comparison
```

```sql
SELECT mass_kg FROM wharf.crate WHERE mass_kg >= 1 AND mass_kg IS NOT NULL
```
