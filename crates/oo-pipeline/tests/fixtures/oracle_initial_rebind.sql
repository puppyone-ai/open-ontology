-- Run against owned, cancelled unpublished initial work and its queued successor.
-- psql -v context='{"source":...,"task":...,"binding":...,"project":...}' -v runtime=...
-- All claims and rebinds are rolled back; no source connection or data mutation.
\set ON_ERROR_STOP on
BEGIN;
SELECT set_config('oo_test.context', :'context', true);
SELECT set_config('oo_test.instance',
  (SELECT to_jsonb(i)::text FROM _oo.source_instance i
   WHERE source=current_setting('oo_test.context')::jsonb->>'source'), true);
SET LOCAL ROLE :"runtime";
DO $test$
DECLARE
  c jsonb:=current_setting('oo_test.context')::jsonb;
  i jsonb:=current_setting('oo_test.instance')::jsonb;
  claimed jobs.lease_result; result jsonb; candidate jsonb; n integer:=0;
BEGIN
  BEGIN
    PERFORM _oo.native_source_instance_register(i->>'shard_id',(c->>'binding')::uuid,
      1,c->>'source','oracle',i->>'measured_id',i->>'config_digest',
      i->>'schema_digest',(c->>'task')::uuid);
    RAISE EXCEPTION 'queued task accepted';
  EXCEPTION WHEN SQLSTATE 'OO030' THEN n:=n+1;
  END;
  claimed:=jobs.claim_selected((c->>'project')::uuid,(c->>'task')::uuid,
    ARRAY['native_capture'],'initial-rebind-rollback-test');
  IF claimed.status<>'claimed' THEN RAISE EXCEPTION 'real claim required'; END IF;
  FOR candidate IN SELECT value FROM jsonb_array_elements(jsonb_build_array(
    jsonb_build_object('owner',i->>'owner_token'),
    jsonb_build_object('generation',claimed.generation+1),
    jsonb_build_object('config',repeat('0',64)),
    jsonb_build_object('schema',repeat('0',64)),
    jsonb_build_object('shard','wrong_shard')
  )) LOOP
    BEGIN
      PERFORM _oo.native_source_instance_register(
        COALESCE(candidate->>'shard',i->>'shard_id'),
        COALESCE(candidate->>'owner',c->>'binding')::uuid,
        COALESCE((candidate->>'generation')::bigint,claimed.generation),
        c->>'source','oracle',i->>'measured_id',
        COALESCE(candidate->>'config',i->>'config_digest'),
        COALESCE(candidate->>'schema',i->>'schema_digest'),(c->>'task')::uuid);
      RAISE EXCEPTION 'invalid context accepted: %',candidate;
    EXCEPTION WHEN SQLSTATE 'OO030' THEN n:=n+1;
    END;
  END LOOP;
  result:=_oo.native_source_instance_register(i->>'shard_id',(c->>'binding')::uuid,
    claimed.generation,c->>'source','oracle',i->>'measured_id',
    i->>'config_digest',i->>'schema_digest',(c->>'task')::uuid);
  IF result->>'lease_id'<>c->>'task' OR result->>'source_epoch'<>i->>'source_epoch'
    OR result->>'config_digest'<>i->>'config_digest' THEN
    RAISE EXCEPTION 'identity or epoch changed';
  END IF;
  IF result IS DISTINCT FROM _oo.native_source_instance_register(
    i->>'shard_id',(c->>'binding')::uuid,claimed.generation,c->>'source','oracle',
    i->>'measured_id',i->>'config_digest',i->>'schema_digest',(c->>'task')::uuid)
  THEN RAISE EXCEPTION 'replay changed'; END IF;
  IF has_table_privilege(current_user,'_oo.oracle_initial_rebind','SELECT')
    OR has_function_privilege(current_user,
      '_oo.native_source_instance_register_before_initial_rebind(text,uuid,bigint,text,text,text,text,text,uuid)',
      'EXECUTE') THEN RAISE EXCEPTION 'runtime can bypass guard'; END IF;
  RAISE NOTICE 'PASS: % refusals, real claim, epoch-preserving rebind, replay, role boundary',n;
END $test$;
RESET ROLE;
DO $test$
DECLARE
  c jsonb:=current_setting('oo_test.context')::jsonb;
  i jsonb:=current_setting('oo_test.instance')::jsonb;
BEGIN
  IF (SELECT count(*) FROM _oo.oracle_initial_rebind
      WHERE source=c->>'source' AND to_lease_id=(c->>'task')::uuid
        AND previous_instance=i AND from_lease_id=(i->>'lease_id')::uuid)<>1
  THEN RAISE EXCEPTION 'immutable old identity audit missing'; END IF;
END $test$;
ROLLBACK;
