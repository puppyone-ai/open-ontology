\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
\pset fieldsep '|'
SELECT c.relname, c.relpersistence, r.rolname
FROM pg_class AS c
JOIN pg_namespace AS n ON n.oid = c.relnamespace
JOIN pg_roles AS r ON r.oid = c.relowner
WHERE n.nspname = '_oo'
  AND c.relkind = 'r'
ORDER BY 1;
SELECT count(*) FROM pg_publication;
SELECT count(*) FROM pg_class WHERE relname LIKE 'q\_%' ESCAPE '\';
