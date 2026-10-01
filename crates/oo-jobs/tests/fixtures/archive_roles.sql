SELECT rolname, rolsuper, rolinherit, rolcreaterole, rolcreatedb, rolcanlogin, rolbypassrls, rolreplication
FROM pg_roles
WHERE rolname IN ('oo_jobs_owner', 'oo_jobs_runtime')
ORDER BY rolname;
