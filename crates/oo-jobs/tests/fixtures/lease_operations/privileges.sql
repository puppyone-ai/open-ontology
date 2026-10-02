\ir prelude.sql
SELECT
  has_table_privilege('oo_jobs_runtime', 'jobs.tasks', 'INSERT')::text
  || '|' || has_table_privilege('oo_jobs_runtime', 'jobs.tasks', 'UPDATE')::text
  || '|' || has_table_privilege('oo_jobs_runtime', 'jobs.tasks', 'DELETE')::text
  || '|' || has_table_privilege('oo_jobs_runtime', 'jobs.tasks', 'SELECT')::text
  || '|' || has_table_privilege('oo_jobs_runtime', 'jobs.task_events', 'INSERT')::text
  || '|' || has_function_privilege('public', 'jobs.claim(uuid,text[],text)', 'EXECUTE')::text
  || '|' || has_function_privilege('oo_jobs_runtime', 'jobs.claim(uuid,text[],text)', 'EXECUTE')::text
  || '|' || has_column_privilege('oo_jobs_runtime', 'jobs.schedules', 'kind', 'INSERT')::text
  || '|' || has_column_privilege('oo_jobs_runtime', 'jobs.leases', 'owner', 'UPDATE')::text
  || '|' || has_column_privilege('oo_jobs_runtime', 'jobs.leases', 'target_fence_certified', 'UPDATE')::text
  || '|' || has_function_privilege('oo_jobs_runtime', 'jobs._lease_token(uuid,uuid,text,bigint,timestamptz)', 'EXECUTE')::text
  || '|' || has_sequence_privilege('oo_jobs_runtime', 'jobs.task_events_event_id_seq', 'USAGE')::text;
