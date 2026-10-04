-- ex-kept
-- verified_at: 2026-04-02
-- 本季到港重量是多少
SELECT sum(mass_kg) AS total FROM dock.parcel

-- ex-kept-quote
-- verified_at: 2026-04-02
-- 可见的引号查询
SELECT mass_kg AS "净""重" FROM "dock"."parcel"

-- ex-scale
-- verified_at: 2026-04-02
-- how many scale rows
SELECT count(*) AS n FROM dock.scale
