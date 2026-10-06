SELECT rolname FROM pg_roles WHERE rolname LIKE 'oo\_%' ESCAPE '\' ORDER BY 1;
SELECT level, logical_namespace, logical_table
FROM _oo.physical_mappings
WHERE level IN ('n', 't')
ORDER BY 1, 2, 3;
SELECT n.nspname, c.relname
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE c.relkind = 'r' AND n.nspname = 'dispatch'
ORDER BY 1, 2;
