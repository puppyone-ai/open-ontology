-- 0112 stays immutable. Until complete-source handoff exists, refuse siblings
-- and any extra current OR historically published member, never drop a table.
CREATE FUNCTION _oo.mysql_handoff_single_scope(s text,b uuid,t uuid DEFAULT NULL) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE names text[]; current_names text[]; staged_names text[];
BEGIN
 IF (SELECT count(*) FROM jw_native.bindings WHERE source_id=s
       AND target_database=current_database())<>1
  OR NOT EXISTS(SELECT 1 FROM jw_native.bindings WHERE id=b AND source_id=s
       AND target_database=current_database()) THEN
  RAISE EXCEPTION 'mysql_handoff_single_binding_required' USING ERRCODE='OO030'; END IF;
 SELECT array_agg(DISTINCT a.schema_ident||'.'||a.table_ident ORDER BY a.schema_ident||'.'||a.table_ident)
 INTO names FROM _oo.full_import_run r JOIN _oo.full_import_table a USING(run_id)
 WHERE r.source_id=s AND r.state='published';
 SELECT array_agg(schema_name||'.'||table_name ORDER BY schema_name||'.'||table_name)
 INTO current_names FROM _oo.table_freshness WHERE source=s;
 IF cardinality(names) IS DISTINCT FROM 1 OR current_names IS DISTINCT FROM names THEN
  RAISE EXCEPTION 'mysql_handoff_exact_published_membership_required' USING ERRCODE='OO030'; END IF;
 IF t IS NOT NULL THEN
  SELECT array_agg(name ORDER BY name) INTO staged_names FROM _oo.c1_typed_table WHERE source=s AND task=t;
  IF staged_names IS DISTINCT FROM names THEN
   RAISE EXCEPTION 'mysql_handoff_exact_staged_membership_required' USING ERRCODE='OO030'; END IF;
 END IF;
END $f$;

CREATE FUNCTION _oo.mysql_handoff_binding_guard(s text,b uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
BEGIN
 IF EXISTS(SELECT 1 FROM _oo.mysql_c5_handoff WHERE source=s AND binding<>b) THEN
  RAISE EXCEPTION 'mysql_handoff_sibling_binding_refused' USING ERRCODE='OO030'; END IF;
END $f$;

ALTER FUNCTION _oo.mysql_handoff_inspect(text,uuid,uuid) RENAME TO mysql_handoff_inspect_before_scope;
CREATE FUNCTION _oo.mysql_handoff_inspect(s text,b uuid,p uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
BEGIN
 PERFORM _oo.mysql_handoff_single_scope(s,b);
 RETURN _oo.mysql_handoff_inspect_before_scope(s,b,p);
END $f$;

-- 0112 confirm dynamically calls the upgraded inspect while holding its source,
-- binding and progress locks. Worker native-put takes the same source lock.
ALTER FUNCTION _oo.mysql_handoff_context(text,uuid,uuid,uuid,bigint,bigint,text)
 RENAME TO mysql_handoff_context_before_scope;
CREATE FUNCTION _oo.mysql_handoff_context(s text,b uuid,p uuid,t uuid,g bigint,base bigint,configsha text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
BEGIN
 IF EXISTS(SELECT 1 FROM _oo.mysql_c5_handoff WHERE source=s) THEN
  PERFORM _oo.mysql_handoff_single_scope(s,b);
 END IF;
 RETURN _oo.mysql_handoff_context_before_scope(s,b,p,t,g,base,configsha);
END $f$;

ALTER FUNCTION _oo.mysql_handoff_final_assert(text,uuid,bigint,bigint)
 RENAME TO mysql_handoff_final_assert_before_scope;
CREATE FUNCTION _oo.mysql_handoff_final_assert(s text,t uuid,g bigint,sg bigint) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE b uuid;
BEGIN
 SELECT binding INTO b FROM _oo.mysql_c5_handoff WHERE source=s;
 IF FOUND THEN PERFORM _oo.mysql_handoff_single_scope(s,b,t); END IF;
 PERFORM _oo.mysql_handoff_final_assert_before_scope(s,t,g,sg);
END $f$;

REVOKE ALL ON FUNCTION _oo.mysql_handoff_inspect_before_scope(text,uuid,uuid),
 _oo.mysql_handoff_context_before_scope(text,uuid,uuid,uuid,bigint,bigint,text),
 _oo.mysql_handoff_final_assert_before_scope(text,uuid,bigint,bigint) FROM PUBLIC,oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.mysql_handoff_binding_guard(text,uuid),
 _oo.mysql_handoff_single_scope(text,uuid,uuid),
 _oo.mysql_handoff_inspect(text,uuid,uuid),
 _oo.mysql_handoff_context(text,uuid,uuid,uuid,bigint,bigint,text),
 _oo.mysql_handoff_final_assert(text,uuid,bigint,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.mysql_handoff_binding_guard(text,uuid),
 _oo.mysql_handoff_inspect(text,uuid,uuid),
 _oo.mysql_handoff_context(text,uuid,uuid,uuid,bigint,bigint,text),
 _oo.mysql_handoff_final_assert(text,uuid,bigint,bigint) TO oo_pipeline_runtime;
