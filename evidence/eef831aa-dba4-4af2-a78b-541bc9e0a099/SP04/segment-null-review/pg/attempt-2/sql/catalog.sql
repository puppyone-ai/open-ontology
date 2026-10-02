SELECT 'proc|' || p.proname || '|' || r.rolname || '|' || p.prosecdef::text || '|' || coalesce(p.proconfig::text, '')
FROM pg_proc p
JOIN pg_roles r ON r.oid = p.proowner
JOIN pg_namespace n ON n.oid = p.pronamespace
WHERE n.nspname = '_oo' AND p.proname IN ('commit_apply_receipt', 'segment_contract_ready')
ORDER BY p.proname;
SELECT 'check|' || pg_get_constraintdef(oid) FROM pg_constraint WHERE conname = 'apply_receipts_segment_check';
SELECT 'member_login|' || pg_has_role('oo_a099_runtime', 'oo_target_owner', 'member')::text;
SELECT 'member_runtime|' || pg_has_role('oo_pipeline_runtime', 'oo_target_owner', 'member')::text;
SELECT 'exec_commit|' || has_function_privilege('oo_a099_runtime', '_oo.commit_apply_receipt(text,uuid,bigint,text,text,text,text,text,text,text,text,text,text,integer,numeric,boolean,boolean,boolean,integer,integer,bytea)', 'EXECUTE')::text;
SELECT 'exec_ready|' || has_function_privilege('oo_a099_runtime', '_oo.segment_contract_ready(text)', 'EXECUTE')::text;
SELECT 'receipt_acl|' || has_table_privilege('oo_a099_runtime', '_oo.apply_receipts', 'SELECT')::text
  || '|' || has_table_privilege('oo_a099_runtime', '_oo.apply_receipts', 'INSERT')::text
  || '|' || has_table_privilege('oo_a099_runtime', '_oo.apply_receipts', 'UPDATE')::text
  || '|' || has_table_privilege('oo_a099_runtime', '_oo.apply_receipts', 'DELETE')::text;
SELECT 'readfile|' || position('pg_read_file' in pg_get_functiondef('_oo.segment_contract_ready(text)'::regprocedure))::text;
SELECT 'grant_in_body|' || position('GRANT oo_target_owner TO oo_pipeline_runtime' in pg_get_functiondef('_oo.commit_apply_receipt(text,uuid,bigint,text,text,text,text,text,text,text,text,text,text,integer,numeric,boolean,boolean,boolean,integer,integer,bytea)'::regprocedure))::text;
SELECT 'view|' || r.rolname || '|' || coalesce(c.reloptions::text, '')
FROM pg_class c
JOIN pg_roles r ON r.oid = c.relowner
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = '_oo' AND c.relname = 'segment_service_readiness';
