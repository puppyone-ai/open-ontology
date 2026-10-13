/* oo-probe:parts */
SELECT
    partition,
    sum(rows) AS rows,
    count() AS active_parts
FROM system.parts
WHERE database = 'oo_dl01' AND table = 'readings' AND active
GROUP BY partition
ORDER BY partition
FORMAT JSONEachRow
