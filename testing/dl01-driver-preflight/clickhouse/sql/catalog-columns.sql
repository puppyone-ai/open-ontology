SELECT database, `table`, name, type, position, default_kind, default_expression
FROM system.columns
WHERE database = 'oo_dl01w40' AND `table` = 'readings'
ORDER BY position
FORMAT JSONEachRow
