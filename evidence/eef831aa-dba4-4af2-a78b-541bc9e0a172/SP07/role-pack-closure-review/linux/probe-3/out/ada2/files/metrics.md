# metrics

Only certified metrics with a visible dependency set are included. Reconciliation is not executed and is not claimed.

## cmp_metric — 比较

status: `active`
unit: kg
default anchor: `month_start`
> Reconciliation status is not in this pack. control_total was not executed and is not claimed to match.

```text
本季到港重量
```

```sql
SELECT mass_kg FROM dock.parcel WHERE mass_kg <> 0 AND mass_kg != 2 AND mass_kg IS NOT NULL
```

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

## distinct_bare — 裸去重

status: `active`
unit: kg
default anchor: `month_start`
> Reconciliation status is not in this pack. control_total was not executed and is not claimed to match.

```text
本季到港重量
```

```sql
SELECT DISTINCT FROM dock.parcel
```

## kept_mass — 可见总量

status: `active`
unit: kg
default anchor: `month_start`
> Unconfirmed codes: dock.parcel.status has undetermined or unexplained values. Those codes count in no class.

> Reconciliation status is not in this pack. control_total was not executed and is not claimed to match.

```text
本季到港重量
```

```sql
SeLeCt "mass_kg" AS "净""重" FROM "dock" . "parcel"
UNION ALL
SELECT mass_kg FROM dock.scale
```

## sq_leak — 单引号标签

status: `active`
unit: kg
default anchor: `month_start`
> Reconciliation status is not in this pack. control_total was not executed and is not claimed to match.

```text
本季到港重量
```

```sql
SELECT '签''记' AS mass_kg FROM dock.parcel
```

## string_metric — 字符串

status: `active`
unit: kg
default anchor: `month_start`
> Reconciliation status is not in this pack. control_total was not executed and is not claimed to match.

```text
可见引号字符串
```

```sql
SELECT 'it''s visible' AS label_ok FROM dock.parcel
```
