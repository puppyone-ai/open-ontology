SELECT grantee, table_name, privilege_type, column_name
FROM information_schema.column_privileges
WHERE table_schema = 'jobs'
ORDER BY grantee, table_name, privilege_type, column_name;

SELECT grantee, table_name, privilege_type
FROM information_schema.table_privileges
WHERE table_schema = 'jobs'
ORDER BY grantee, table_name, privilege_type;

SELECT routine_name, grantee, privilege_type
FROM information_schema.routine_privileges
WHERE routine_schema = 'jobs'
ORDER BY routine_name, grantee, privilege_type;

SELECT nspname, nspacl::text
FROM pg_namespace
WHERE nspname IN ('jobs', 'public', 'other_ns')
ORDER BY nspname;
