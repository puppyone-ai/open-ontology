\set ON_ERROR_STOP on
DO $check$
DECLARE
  owner_row pg_roles%ROWTYPE;
  runtime_row pg_roles%ROWTYPE;
  login_row pg_roles%ROWTYPE;
  bootstrap_row pg_roles%ROWTYPE;
  gateway_row pg_roles%ROWTYPE;
  agent_row pg_roles%ROWTYPE;
  inherit_flag boolean;
  set_flag boolean;
  unsafe_definer text;
BEGIN
  SELECT * INTO owner_row FROM pg_roles WHERE rolname = 'oo_target_owner';
  SELECT * INTO runtime_row FROM pg_roles WHERE rolname = 'oo_pipeline_runtime';
  SELECT * INTO login_row FROM pg_roles WHERE rolname = 'oo_a072_runtime';
  SELECT * INTO bootstrap_row FROM pg_roles WHERE rolname = 'oo_a072_bootstrap';
  SELECT * INTO gateway_row FROM pg_roles WHERE rolname = 'oo_a072_gateway';
  SELECT * INTO agent_row FROM pg_roles WHERE rolname = 'oo_a072_agent';
  IF owner_row.rolname IS NULL OR runtime_row.rolname IS NULL
     OR login_row.rolname IS NULL OR bootstrap_row.rolname IS NULL
     OR gateway_row.rolname IS NULL OR agent_row.rolname IS NULL
  THEN
    RAISE EXCEPTION 'target roles are missing' USING ERRCODE = 'OO004';
  END IF;
  IF owner_row.rolsuper OR owner_row.rolcreatedb OR owner_row.rolcreaterole
     OR owner_row.rolcanlogin OR owner_row.rolbypassrls OR owner_row.rolreplication
     OR owner_row.rolinherit
     OR runtime_row.rolsuper OR runtime_row.rolcreatedb OR runtime_row.rolcreaterole
     OR runtime_row.rolcanlogin OR runtime_row.rolbypassrls OR runtime_row.rolreplication
     OR runtime_row.rolinherit
     OR login_row.rolsuper OR login_row.rolcreatedb OR login_row.rolcreaterole
     OR login_row.rolbypassrls OR NOT login_row.rolcanlogin
     OR bootstrap_row.rolsuper OR NOT bootstrap_row.rolcanlogin OR bootstrap_row.rolinherit
     OR gateway_row.rolsuper OR agent_row.rolsuper
  THEN
    RAISE EXCEPTION 'target role has a forbidden attribute' USING ERRCODE = 'OO004';
  END IF;
  IF pg_has_role('oo_pipeline_runtime', 'oo_target_owner', 'member')
     OR pg_has_role('oo_a072_runtime', 'oo_target_owner', 'member')
     OR pg_has_role('oo_a072_gateway', 'oo_target_owner', 'member')
     OR pg_has_role('oo_a072_agent', 'oo_pipeline_runtime', 'member')
     OR NOT pg_has_role('oo_a072_runtime', 'oo_pipeline_runtime', 'member')
     OR NOT pg_has_role('oo_a072_bootstrap', 'oo_target_owner', 'member')
  THEN
    RAISE EXCEPTION 'target role membership does not match the fence contract' USING ERRCODE = 'OO004';
  END IF;
  SELECT member_grant.inherit_option, member_grant.set_option
    INTO inherit_flag, set_flag
  FROM pg_auth_members AS member_grant
  JOIN pg_roles AS granted ON granted.oid = member_grant.roleid
  JOIN pg_roles AS member ON member.oid = member_grant.member
  WHERE member.rolname = 'oo_a072_runtime'
    AND granted.rolname = 'oo_pipeline_runtime';
  IF inherit_flag IS DISTINCT FROM true OR set_flag IS DISTINCT FROM false THEN
    RAISE EXCEPTION 'runtime membership is not inherit-without-set' USING ERRCODE = 'OO004';
  END IF;
  SELECT member_grant.inherit_option, member_grant.set_option
    INTO inherit_flag, set_flag
  FROM pg_auth_members AS member_grant
  JOIN pg_roles AS granted ON granted.oid = member_grant.roleid
  JOIN pg_roles AS member ON member.oid = member_grant.member
  WHERE member.rolname = 'oo_a072_bootstrap'
    AND granted.rolname = 'oo_target_owner';
  IF inherit_flag IS DISTINCT FROM false OR set_flag IS DISTINCT FROM true THEN
    RAISE EXCEPTION 'bootstrap membership is not set-without-inherit' USING ERRCODE = 'OO004';
  END IF;
  IF has_schema_privilege('oo_pipeline_runtime', '_oo', 'CREATE')
     OR has_schema_privilege('public', '_oo', 'USAGE')
     OR has_database_privilege('oo_pipeline_runtime', current_database(), 'TEMP')
     OR has_database_privilege('oo_pipeline_runtime', current_database(), 'CREATE')
     OR has_database_privilege('oo_a072_gateway', current_database(), 'CONNECT')
     OR has_database_privilege('oo_a072_agent', current_database(), 'CONNECT')
     OR has_table_privilege('oo_pipeline_runtime', '_oo.apply_receipts', 'SELECT')
     OR has_table_privilege('oo_pipeline_runtime', '_oo.apply_receipts', 'INSERT')
     OR has_table_privilege('oo_pipeline_runtime', '_oo.target_generations', 'SELECT')
     OR has_table_privilege('oo_pipeline_runtime', '_oo.schema_migrations', 'INSERT')
     OR has_table_privilege('oo_pipeline_runtime', '_oo.schema_migrations', 'UPDATE')
     OR has_table_privilege('public', '_oo.apply_receipts', 'SELECT')
     OR has_table_privilege('oo_a072_runtime', 'fixture_synth.applied_row', 'SELECT')
     OR has_table_privilege('oo_a072_runtime', 'fixture_synth.applied_row', 'INSERT')
  THEN
    RAISE EXCEPTION 'runtime or public received a closed privilege' USING ERRCODE = 'OO004';
  END IF;
  IF NOT has_database_privilege('oo_a072_runtime', current_database(), 'CONNECT')
     OR NOT has_schema_privilege('oo_pipeline_runtime', '_oo', 'USAGE')
     OR NOT has_table_privilege('oo_pipeline_runtime', '_oo.schema_migrations', 'SELECT')
     OR NOT has_table_privilege('oo_pipeline_runtime', '_oo.service_readiness', 'SELECT')
     OR NOT has_function_privilege('oo_pipeline_runtime', '_oo.schema_ready(text)', 'EXECUTE')
     OR NOT has_function_privilege('oo_pipeline_runtime', '_oo.adopt_target_generation(text,uuid)', 'EXECUTE')
     OR NOT has_function_privilege('oo_pipeline_runtime', '_oo.renew_target_generation(text,uuid,bigint)', 'EXECUTE')
     OR NOT has_function_privilege('oo_pipeline_runtime', '_oo.fail_target_generation(text,uuid,bigint)', 'EXECUTE')
     OR NOT has_function_privilege('oo_pipeline_runtime', '_oo.assert_target_fence(text,uuid,bigint)', 'EXECUTE')
     OR NOT has_function_privilege(
       'oo_pipeline_runtime',
       '_oo.commit_apply_receipt(text,uuid,bigint,text,text,text,text,text,text,text,text,text,text,integer,numeric,boolean,boolean,boolean,integer,integer,bytea)',
       'EXECUTE'
     )
     OR NOT has_function_privilege('oo_a072_runtime', 'fixture_synth.commit_bundle(text,uuid,bigint,text,text)', 'EXECUTE')
  THEN
    RAISE EXCEPTION 'runtime is missing a required fence grant' USING ERRCODE = 'OO004';
  END IF;
  IF has_function_privilege('public', '_oo.adopt_target_generation(text,uuid)', 'EXECUTE')
     OR has_function_privilege('public', '_oo.commit_apply_receipt(text,uuid,bigint,text,text,text,text,text,text,text,text,text,text,integer,numeric,boolean,boolean,boolean,integer,integer,bytea)', 'EXECUTE')
     OR has_function_privilege('oo_pipeline_runtime', '_oo._allow_write()', 'EXECUTE')
     OR has_function_privilege('oo_pipeline_runtime', '_oo.lock_key(text)', 'EXECUTE')
     OR has_function_privilege('oo_pipeline_runtime', 'fixture_synth.force_generation(text,bigint)', 'EXECUTE')
     OR has_function_privilege('public', 'pg_catalog.pg_try_advisory_lock(integer,integer)', 'EXECUTE')
     OR has_function_privilege('public', 'pg_catalog.set_config(text,text,boolean)', 'EXECUTE')
     OR has_function_privilege('oo_pipeline_runtime', 'pg_catalog.set_config(text,text,boolean)', 'EXECUTE')
     OR has_function_privilege('oo_pipeline_runtime', 'pg_catalog.pg_try_advisory_lock(integer,integer)', 'EXECUTE')
  THEN
    RAISE EXCEPTION 'public or runtime can bypass the fence API' USING ERRCODE = 'OO004';
  END IF;
  SELECT string_agg(pg_proc.proname, ',')
    INTO unsafe_definer
  FROM pg_proc
  JOIN pg_namespace ON pg_namespace.oid = pg_proc.pronamespace
  WHERE pg_namespace.nspname = '_oo'
    AND pg_proc.prosecdef
    AND NOT EXISTS (
      SELECT 1
      FROM unnest(coalesce(pg_proc.proconfig, ARRAY[]::text[])) AS config_item
      WHERE config_item LIKE 'search_path=%'
    );
  IF unsafe_definer IS NOT NULL THEN
    RAISE EXCEPTION 'security definer without search_path: %', unsafe_definer USING ERRCODE = 'OO004';
  END IF;
  IF (
    SELECT count(*)
    FROM pg_proc
    JOIN pg_namespace ON pg_namespace.oid = pg_proc.pronamespace
    WHERE pg_namespace.nspname = '_oo'
      AND pg_proc.prosecdef
  ) <> 10 THEN
    RAISE EXCEPTION 'unexpected security definer count in _oo' USING ERRCODE = 'OO004';
  END IF;
END
$check$;
