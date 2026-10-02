\set ON_ERROR_STOP on
DO $check$
BEGIN
  IF pg_has_role('oo_a075_login', 'oo_jobs_owner', 'member')
     OR pg_has_role('oo_jobs_runtime', 'oo_jobs_owner', 'member')
     OR NOT pg_has_role('oo_a075_login', 'oo_jobs_runtime', 'member')
  THEN
    RAISE EXCEPTION 'runtime membership drifted' USING ERRCODE = 'OO004';
  END IF;
  IF (
    SELECT admin_option
    FROM pg_auth_members AS membership
    JOIN pg_roles AS granted ON granted.oid = membership.roleid
    JOIN pg_roles AS member ON member.oid = membership.member
    WHERE member.rolname = 'oo_a075_login'
      AND granted.rolname = 'oo_jobs_runtime'
  ) IS DISTINCT FROM false THEN
    RAISE EXCEPTION 'runtime membership has admin option' USING ERRCODE = 'OO004';
  END IF;
  IF has_table_privilege('oo_jobs_runtime', 'jobs.task_events', 'UPDATE')
     OR has_table_privilege('oo_jobs_runtime', 'jobs.task_events', 'DELETE')
     OR has_table_privilege('oo_jobs_runtime', 'jobs.task_events', 'TRUNCATE')
     OR has_table_privilege('oo_jobs_runtime', 'jobs.task_events', 'TRIGGER')
     OR has_column_privilege('oo_jobs_runtime', 'jobs.tasks', 'project_id', 'UPDATE')
     OR has_table_privilege('public', 'jobs.event_project_readiness', 'SELECT')
     OR has_table_privilege('public', 'jobs.task_events', 'INSERT')
  THEN
    RAISE EXCEPTION 'event binding widened a runtime or public grant' USING ERRCODE = 'OO004';
  END IF;
  IF NOT has_table_privilege('oo_jobs_runtime', 'jobs.task_events', 'INSERT')
     OR NOT has_table_privilege('oo_jobs_runtime', 'jobs.task_events', 'SELECT')
     OR NOT has_table_privilege('oo_jobs_runtime', 'jobs.event_project_readiness', 'SELECT')
     OR NOT has_table_privilege('oo_a075_login', 'jobs.event_project_readiness', 'SELECT')
  THEN
    RAISE EXCEPTION 'runtime is missing event insert or readiness select' USING ERRCODE = 'OO004';
  END IF;
END
$check$;
