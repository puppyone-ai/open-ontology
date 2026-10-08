SELECT rolname, rolsuper, rolcreatedb, rolcreaterole, rolinherit, rolcanlogin, rolbypassrls, rolreplication
FROM pg_roles
WHERE rolname IN ('feedback_receiver', 'audit_ingest', 'oo_r_reviewer')
ORDER BY rolname;
SELECT datname
FROM pg_database
WHERE datname IN ('oo_feedback', 'oo_audit')
ORDER BY datname;
SELECT grantee, table_schema, table_name, privilege_type
FROM information_schema.role_table_grants
WHERE grantee IN ('feedback_receiver', 'audit_ingest')
ORDER BY 1, 2, 3, 4;
SELECT has_database_privilege('feedback_receiver', 'oo_audit', 'CONNECT') AS feedback_on_audit,
       has_database_privilege('audit_ingest', 'oo_feedback', 'CONNECT') AS audit_on_feedback,
       has_table_privilege('feedback_receiver', 'oo_feedback_receiver.receipt', 'INSERT') AS can_insert,
       has_table_privilege('feedback_receiver', 'oo_feedback_receiver.receipt', 'SELECT') AS can_select,
       has_table_privilege('feedback_receiver', 'oo_feedback_receiver.receipt', 'UPDATE') AS can_update,
       has_table_privilege('feedback_receiver', 'oo_feedback_receiver.receipt', 'DELETE') AS can_delete,
       has_table_privilege('feedback_receiver', 'oo_feedback_receiver.receipt', 'TRUNCATE') AS can_truncate;
