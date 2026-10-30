-- Same-task initial fill recovery at a confirmed C5 -> C1 generation.
-- Preserve the original metadata, L0, epoch, deadline and every receipt/cursor.
CREATE FUNCTION _oo.mysql_initial_generation_allowed(s text,t uuid,g bigint,e integer,p jsonb)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE h _oo.mysql_c5_handoff%ROWTYPE; expected jsonb;
BEGIN
 IF p->>'source_generation'='1' THEN RETURN p->'handoff' IS NULL OR p->'handoff'='null'::jsonb; END IF;
 SELECT * INTO h FROM _oo.mysql_c5_handoff WHERE source=s;
 IF NOT FOUND OR h.task IS DISTINCT FROM t OR h.source_epoch IS DISTINCT FROM e
  OR p->>'l0' IS DISTINCT FROM h.l0
  OR p->>'connection_config_sha256' IS DISTINCT FROM h.config_sha256
  OR p->>'source_uuid' IS DISTINCT FROM h.consent->>'expected_server_uuid'
  OR (p->>'source_generation')::bigint IS DISTINCT FROM (h.old_state->>'source_generation')::bigint+1 THEN
  RETURN false;
 END IF;
 expected:=_oo.mysql_handoff_context(s,h.binding,h.project,t,g,
  (h.old_state->>'source_generation')::bigint,h.config_sha256);
 IF expected IS NULL OR p->'handoff' IS DISTINCT FROM expected THEN RETURN false; END IF;
 PERFORM _oo.mysql_handoff_single_scope(s,h.binding,t);
 RETURN true;
END $f$;
REVOKE ALL ON FUNCTION _oo.mysql_initial_generation_allowed(text,uuid,bigint,integer,jsonb)
 FROM PUBLIC,oo_pipeline_runtime;

CREATE OR REPLACE FUNCTION _oo.c1_mysql_initial_open(s text,t uuid,g bigint,e integer,p jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE prior _oo.c1_mysql_initial%ROWTYPE; q record;
BEGIN
  IF p->>'protocol' IS DISTINCT FROM 'oo.mysql.initial-session/v1'
    OR p->>'source_id' IS DISTINCT FROM s OR p->>'task_id' IS DISTINCT FROM t::text
    OR NOT _oo.mysql_initial_generation_allowed(s,t,g,e,p)
    OR jsonb_typeof(p->'typed_tables') IS DISTINCT FROM 'object'
    OR jsonb_typeof(p->'key_columns') IS DISTINCT FROM 'object'
    OR COALESCE(p->>'connection_config_sha256','') !~ '^[0-9a-f]{64}$'
    OR COALESCE(p->>'catalog_digest','') !~ '^[0-9a-f]{64}$' THEN
    RAISE EXCEPTION 'mysql_initial_header' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO prior FROM _oo.c1_mysql_initial WHERE source=s AND task=t;
  IF FOUND AND (prior.metadata IS DISTINCT FROM p OR prior.epoch<>e) THEN
    RAISE EXCEPTION 'mysql_initial_header_conflict' USING ERRCODE='OO030';
  END IF;
  INSERT INTO _oo.c1_mysql_initial(source,task,epoch,metadata) VALUES(s,t,e,p) ON CONFLICT DO NOTHING;
  PERFORM _oo.c1_mysql_initial_assert(s,t,g,e);
  IF (SELECT count(*) FROM _oo.c1_typed_table WHERE source=s AND task=t)
      IS DISTINCT FROM (SELECT count(*) FROM jsonb_object_keys(p->'typed_tables')) THEN
    RAISE EXCEPTION 'mysql_initial_tables' USING ERRCODE='OO030';
  END IF;
  FOR q IN SELECT * FROM _oo.c1_typed_table WHERE source=s AND task=t LOOP
    IF q.spec IS DISTINCT FROM p->'typed_tables'->q.name
      OR jsonb_typeof(p->'key_columns'->q.name) IS DISTINCT FROM 'array' THEN
      RAISE EXCEPTION 'mysql_initial_table_schema' USING ERRCODE='OO030';
    END IF;
  END LOOP;
END $fn$;

