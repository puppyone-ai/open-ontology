-- Compare changed cells before composing the immutable view. Never scan its parent.
CREATE FUNCTION _oo.c5_delta_read_cells(p_run uuid,p_columns text[])
RETURNS SETOF jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE o _oo.c5_overlay; r _oo.full_import_run; projection text:=''; col text;
BEGIN
 SELECT * INTO STRICT o FROM _oo.c5_overlay WHERE run_id=p_run;
 SELECT * INTO STRICT r FROM _oo.full_import_run WHERE run_id=p_run;
 PERFORM _oo.full_import_assert_bound(r.shard_id,r.owner_token,r.target_generation);
 IF r.state<>'open' OR o.prepared_xid<>pg_current_xact_id() OR o.view_oid IS NOT NULL
  OR p_columns IS DISTINCT FROM o.child_columns
  OR cardinality(p_columns) NOT BETWEEN 1 AND 1024
  OR (o.scope->>'row_count')::bigint NOT BETWEEN 0 AND 100000
  OR to_regclass(format('_oo.%I',o.physical_name)) IS DISTINCT FROM o.delta_oid
  OR NOT EXISTS(SELECT FROM pg_class WHERE oid=o.delta_oid AND relkind='r'
   AND relowner='oo_target_owner'::regrole)
 THEN RAISE EXCEPTION 'C5 open delta reconciliation scope differs' USING ERRCODE='OO030'; END IF;
 FOREACH col IN ARRAY p_columns LOOP
  IF projection<>'' THEN projection:=projection||','; END IF;
  projection:=projection||format('d.%I::text',col);
 END LOOP;
 RETURN QUERY EXECUTE format('SELECT to_jsonb(ARRAY[%s]) FROM _oo.%I d',
  projection,o.physical_name);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_delta_read_cells(uuid,text[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_delta_read_cells(uuid,text[]) TO oo_pipeline_runtime;
