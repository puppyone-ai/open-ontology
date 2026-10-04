-- ex-quoted
-- verified_at: 2026-04-01
-- how the hidden slip is referenced
SELECT weight_kg AS "客户备注" FROM depot.shipment
UNION ALL
SELECT weight_kg FROM "yard"."slip"

-- ex-weight
-- verified_at: 2026-04-01
-- how much weight is visible
SELECT sum(weight_kg) AS total FROM depot.shipment
