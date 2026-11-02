ALTER FUNCTION _oo.c5_initialize_full(text,uuid,bigint,text,text,text,text,bigint,text,text[])
 RENAME TO c5_initialize_full_single;
REVOKE ALL ON FUNCTION _oo.c5_initialize_full_single(text,uuid,bigint,text,text,text,text,bigint,text,text[])
 FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_initialize_full(p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,p_fingerprint text,p_topology bigint,p_chunk text,p_keys text[])
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 IF EXISTS(SELECT 1 FROM _oo.table_freshness f JOIN _oo.full_import_table t
   ON t.physical_name=f.physical_name AND t.schema_ident=f.schema_name AND t.table_ident=f.table_name
   JOIN _oo.full_import_run r ON r.run_id=t.run_id AND r.source_id=f.source
   WHERE f.source=p_source AND f.schema_name=p_schema AND f.table_name=p_table
    AND r.source_generation=f.source_generation AND r.multipart_scope IS NOT NULL)
 THEN RAISE EXCEPTION 'C5 multipart baseline unsupported; durable manifest topology initialization required' USING ERRCODE='OO030'; END IF;
 PERFORM _oo.c5_initialize_full_single(p_shard,p_owner,p_fence,p_source,p_schema,p_table,p_fingerprint,p_topology,p_chunk,p_keys);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_initialize_full(text,uuid,bigint,text,text,text,text,bigint,text,text[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_initialize_full(text,uuid,bigint,text,text,text,text,bigint,text,text[]) TO oo_pipeline_runtime;
