-- Complete-source capture revision. Retain every old binding and publication.
-- The source_progress binding is the sole C1 writer; siblings stay disabled.
CREATE FUNCTION _oo.mysql_handoff_members(s text) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
 SELECT coalesce(jsonb_agg(jsonb_build_object(
  'binding',b.id,'project_id',b.project_id,'source_id',b.source_id,
  'target_database',b.target_database,'method',b.method,'cycle',b.cycle,
  'period_ms',b.period_ms,'enabled',b.enabled,'manifest',b.manifest,
  'manifest_sha256',b.manifest_sha,'previous_task',j.id,
  'source_generation',CASE WHEN j.id IS NOT NULL THEN w.source_generation END,
  'capture_manifest',o.manifest,'request',o.request,
  'load',CASE WHEN j.id IS NOT NULL THEN w.result END,
  'latest_success',latest.evidence) ORDER BY b.id),'[]'::jsonb)
 FROM jw_native.bindings b
 LEFT JOIN jw_native.watermarks w ON w.binding_id=b.id
 LEFT JOIN jobs.tasks j ON j.id=w.task_id AND j.state='succeeded'
 LEFT JOIN jw_native.occurrences o ON o.task_id=j.id AND o.binding_id=b.id
 LEFT JOIN LATERAL (
  SELECT jsonb_build_object('task',x.task_id,'manifest',x.manifest,
   'request',x.request,'result',x.result,'producer_input',x.producer_input) evidence
  FROM jw_native.occurrences x JOIN jobs.tasks q ON q.id=x.task_id
  WHERE x.binding_id=b.id AND q.state='succeeded' ORDER BY x.pin DESC LIMIT 1
 ) latest ON true
 WHERE b.source_id=s AND b.target_database=current_database()
$f$;

CREATE FUNCTION _oo.mysql_handoff_tables(s text) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
 SELECT coalesce(jsonb_agg(to_jsonb(f)||jsonb_build_object('columns',coalesce(
  (SELECT jsonb_agg(n.logical_column ORDER BY n.logical_column)
   FROM (SELECT DISTINCT m.logical_column FROM _oo.physical_mappings m
    WHERE m.source=f.source AND m.level='c' AND m.run_generation=f.source_generation
     AND m.scope_table_physical=f.physical_name) n),'[]'::jsonb))
  ORDER BY f.schema_name,f.table_name),'[]'::jsonb)
 FROM _oo.table_freshness f WHERE f.source=s
$f$;

CREATE FUNCTION _oo.mysql_handoff_complete_scope(s text,b uuid,t uuid DEFAULT NULL) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE h _oo.mysql_c5_handoff%ROWTYPE; names text[]; current_names text[];
 staged_names text[]; current_bindings jsonb; current_tables jsonb; member jsonb; prior jsonb;
 expected_columns text[]; staged_columns text[]; snapshot_phase boolean;
BEGIN
 SELECT * INTO h FROM _oo.mysql_c5_handoff WHERE source=s;
 SELECT array_agg(DISTINCT a.schema_ident||'.'||a.table_ident ORDER BY a.schema_ident||'.'||a.table_ident)
 INTO names FROM _oo.full_import_run r JOIN _oo.full_import_table a USING(run_id)
 WHERE r.source_id=s AND r.state='published';
 SELECT array_agg(schema_name||'.'||table_name ORDER BY schema_name||'.'||table_name)
 INTO current_names FROM _oo.table_freshness WHERE source=s;
 IF coalesce(cardinality(names),0)=0 OR current_names IS DISTINCT FROM names
  OR EXISTS(SELECT 1 FROM _oo.table_freshness WHERE source=s AND state<>'live') THEN
  RAISE EXCEPTION 'mysql_handoff_exact_published_membership_required' USING ERRCODE='OO030'; END IF;
 current_bindings:=_oo.mysql_handoff_members(s);
 current_tables:=_oo.mysql_handoff_tables(s);
 IF NOT EXISTS(SELECT 1 FROM jsonb_array_elements(current_bindings) x WHERE x->>'binding'=b::text)
  OR EXISTS(SELECT 1 FROM jsonb_array_elements(current_tables) x
   WHERE jsonb_typeof(x->'columns') IS DISTINCT FROM 'array' OR jsonb_array_length(x->'columns')=0) THEN
  RAISE EXCEPTION 'mysql_handoff_complete_source_required' USING ERRCODE='OO030'; END IF;
 IF h.source IS NULL THEN
  IF EXISTS(SELECT 1 FROM jsonb_array_elements(current_bindings) x
    WHERE x->>'enabled' IS DISTINCT FROM 'false' OR x->>'method' IS DISTINCT FROM 'C5'
     OR x->'manifest'->>'method' IS DISTINCT FROM 'C5'
     OR x->'manifest'->'c5_state' IS NOT NULL AND NOT
       ((x->'manifest'->'c5_state'->>'schema')||'.'||(x->'manifest'->'c5_state'->>'table')=ANY(names))) THEN
   RAISE EXCEPTION 'mysql_handoff_stop_complete_source_required' USING ERRCODE='OO030'; END IF;
 ELSIF h.old_state->>'scope_protocol'='oo.mysql.complete-source-handoff/v1' THEN
  IF h.binding<>b OR jsonb_array_length(current_bindings)<>jsonb_array_length(h.old_state->'bindings')
   OR current_names IS DISTINCT FROM ARRAY(SELECT (x->>'schema_name')||'.'||(x->>'table_name')
       FROM jsonb_array_elements(h.old_state->'tables') x ORDER BY 1) THEN
   RAISE EXCEPTION 'mysql_handoff_member_set_changed' USING ERRCODE='OO030'; END IF;
  FOR member IN SELECT value FROM jsonb_array_elements(current_bindings) LOOP
   SELECT x INTO prior FROM jsonb_array_elements(h.old_state->'bindings') x
    WHERE x->>'binding'=member->>'binding';
   IF prior IS NULL OR member->'project_id' IS DISTINCT FROM prior->'project_id'
    OR member->'source_id' IS DISTINCT FROM prior->'source_id'
    OR member->'target_database' IS DISTINCT FROM prior->'target_database' THEN
    RAISE EXCEPTION 'mysql_handoff_member_identity_changed' USING ERRCODE='OO030'; END IF;
   IF member->>'binding'<>b::text AND
    (member->>'enabled' IS DISTINCT FROM 'false' OR member->'method' IS DISTINCT FROM prior->'method'
     OR member->'manifest' IS DISTINCT FROM prior->'manifest'
     OR member->'manifest_sha256' IS DISTINCT FROM prior->'manifest_sha256'
     OR member->'cycle' IS DISTINCT FROM prior->'cycle'
     OR member->'period_ms' IS DISTINCT FROM prior->'period_ms') THEN
    RAISE EXCEPTION 'mysql_handoff_retired_binding_changed' USING ERRCODE='OO030'; END IF;
  END LOOP;
  SELECT position_codec='snapshot-zero/v1' INTO snapshot_phase FROM _oo.source_watermark WHERE source=s;
  IF snapshot_phase AND current_tables IS DISTINCT FROM h.old_state->'tables' THEN
   RAISE EXCEPTION 'mysql_handoff_old_table_cut_changed' USING ERRCODE='OO030'; END IF;
 ELSE
  -- Already confirmed v1 single-table plans retain their original restriction.
  IF jsonb_array_length(current_bindings)<>1 OR cardinality(names)<>1 THEN
   RAISE EXCEPTION 'mysql_handoff_legacy_single_scope_required' USING ERRCODE='OO030'; END IF;
 END IF;
 IF t IS NOT NULL THEN
  SELECT array_agg(name ORDER BY name) INTO staged_names FROM _oo.c1_typed_table WHERE source=s AND task=t;
  IF staged_names IS DISTINCT FROM names THEN
   RAISE EXCEPTION 'mysql_handoff_exact_staged_membership_required' USING ERRCODE='OO030'; END IF;
  FOR member IN SELECT value FROM jsonb_array_elements(current_tables) LOOP
   SELECT array_agg(value ORDER BY value) INTO expected_columns FROM jsonb_array_elements_text(member->'columns');
   SELECT array_agg(c->>'name' ORDER BY c->>'name') INTO staged_columns
    FROM _oo.c1_typed_table a CROSS JOIN LATERAL jsonb_array_elements(a.spec->'columns') c
    WHERE a.source=s AND a.task=t AND a.name=(member->>'schema_name')||'.'||(member->>'table_name');
   IF staged_columns IS DISTINCT FROM expected_columns THEN
    RAISE EXCEPTION 'mysql_handoff_exact_staged_columns_required' USING ERRCODE='OO030'; END IF;
  END LOOP;
 END IF;
END $f$;

-- Compatibility entry used by the immutable 0113/0114 recovery contracts.
CREATE OR REPLACE FUNCTION _oo.mysql_handoff_single_scope(s text,b uuid,t uuid DEFAULT NULL) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
BEGIN
 PERFORM _oo.mysql_handoff_complete_scope(s,b,t);
END $f$;

CREATE OR REPLACE FUNCTION _oo.mysql_handoff_inspect(s text,b uuid,p uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE old jsonb;
BEGIN
 PERFORM _oo.mysql_handoff_complete_scope(s,b);
 old:=_oo.mysql_handoff_inspect_before_scope(s,b,p);
 RETURN old||jsonb_build_object('scope_protocol','oo.mysql.complete-source-handoff/v1',
  'bindings',_oo.mysql_handoff_members(s),'tables',_oo.mysql_handoff_tables(s));
END $f$;

CREATE OR REPLACE FUNCTION _oo.mysql_handoff_context(s text,b uuid,p uuid,t uuid,g bigint,base bigint,configsha text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE h _oo.mysql_c5_handoff%ROWTYPE;
BEGIN
 SELECT * INTO h FROM _oo.mysql_c5_handoff WHERE source=s;
 IF FOUND AND base<=(h.old_state->>'source_generation')::bigint THEN
  PERFORM _oo.mysql_handoff_complete_scope(s,b);
 END IF;
 RETURN _oo.mysql_handoff_context_before_scope(s,b,p,t,g,base,configsha);
END $f$;

CREATE OR REPLACE FUNCTION _oo.mysql_handoff_final_assert(s text,t uuid,g bigint,sg bigint) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE b uuid;
BEGIN
 SELECT h.binding INTO b FROM _oo.mysql_c5_handoff h JOIN _oo.source_watermark w ON w.source=h.source
  WHERE h.source=s AND w.position_codec='snapshot-zero/v1';
 IF FOUND THEN PERFORM _oo.mysql_handoff_complete_scope(s,b,t); END IF;
 PERFORM _oo.mysql_handoff_final_assert_before_scope(s,t,g,sg);
END $f$;

REVOKE ALL ON FUNCTION _oo.mysql_handoff_members(text),_oo.mysql_handoff_tables(text),
 _oo.mysql_handoff_complete_scope(text,uuid,uuid) FROM PUBLIC,oo_pipeline_runtime;
