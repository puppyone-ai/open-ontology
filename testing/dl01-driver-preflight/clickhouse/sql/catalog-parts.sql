SELECT partition, toString(sum(rows)) AS rows, toString(count()) AS active_parts
FROM system.parts
WHERE database = 'oo_dl01w40' AND `table` = 'readings' AND active = 1
GROUP BY partition
ORDER BY partition
FORMAT JSONEachRow
