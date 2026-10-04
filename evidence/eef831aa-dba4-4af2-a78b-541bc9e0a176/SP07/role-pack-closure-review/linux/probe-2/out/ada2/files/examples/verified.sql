-- ex-kept
-- verified_at: 2026-08-19
-- 可见关系还在，净重仍可引用。
SELECT mass_kg AS rel_kept_alias FROM wharf.crate

-- ex-quote
-- verified_at: 2026-08-19
-- 可见引号查询，净重仍在。
SELECT mass_kg AS "净重" FROM "wharf"."crate"

-- ex-string
-- verified_at: 2026-08-19
-- 可见字符串示例，撇号按原文保留。
SELECT 'it''s visible' AS label_ok FROM wharf.crate
