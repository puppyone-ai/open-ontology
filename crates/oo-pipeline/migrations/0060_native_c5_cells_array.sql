-- native-c5-cells-array/v1: build the audit cell row from one ARRAY so tables with more than 100 columns do not hit the 100-argument function limit.
CREATE OR REPLACE FUNCTION _oo.c5_multipart_cells(p_physical text,p_columns text[],p_keys text[],p_members jsonb)
RETURNS SETOF jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE k text; cols text:=''; predicate text:=''; typ text; i integer:=0;
BEGIN
 IF NOT EXISTS(SELECT FROM _oo.full_import_table t JOIN _oo.full_import_run r USING(run_id) WHERE t.physical_name=p_physical AND r.state IN ('open','awaiting_publish','published') AND r.multipart_scope IS NOT NULL)
  OR cardinality(p_columns) NOT BETWEEN 1 AND 1024 OR cardinality(p_keys) NOT BETWEEN 1 AND 64
  OR jsonb_typeof(p_members) IS DISTINCT FROM 'array' OR jsonb_array_length(p_members)>100000
 THEN RAISE EXCEPTION 'multipart read scope'; END IF;
 FOREACH k IN ARRAY p_columns LOOP
  IF k !~ '^pc_[0-9a-f]{60}$' OR NOT EXISTS(SELECT FROM pg_attribute WHERE attrelid=to_regclass(format('_oo.%I',p_physical)) AND attname=k AND NOT attisdropped) THEN RAISE EXCEPTION 'multipart column attribute'; END IF;
  IF cols<>'' THEN cols:=cols||','; END IF; cols:=cols||format('r.%I::text',k);
 END LOOP;
 FOREACH k IN ARRAY p_keys LOOP
  SELECT format_type(atttypid,atttypmod) INTO STRICT typ FROM pg_attribute WHERE attrelid=to_regclass(format('_oo.%I',p_physical)) AND attname=k AND NOT attisdropped AND attnotnull;
  IF k !~ '^pc_[0-9a-f]{60}$' THEN RAISE EXCEPTION 'multipart key attribute'; END IF;
  IF predicate<>'' THEN predicate:=predicate||' AND '; END IF;
  predicate:=predicate||format('r.%I=(m.key->>%s)::%s',k,i,typ);i:=i+1;
 END LOOP;
 IF EXISTS(SELECT FROM jsonb_array_elements(p_members) m WHERE jsonb_typeof(m) IS DISTINCT FROM 'array' OR jsonb_array_length(m)<>cardinality(p_keys)) THEN RAISE EXCEPTION 'multipart key width'; END IF;
 RETURN QUERY EXECUTE format('SELECT to_jsonb(ARRAY[%s]) FROM _oo.%I r JOIN jsonb_array_elements($1) m(key) ON %s',cols,p_physical,predicate) USING p_members;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_multipart_cells(text,text[],text[],jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_multipart_cells(text,text[],text[],jsonb) TO oo_pipeline_runtime;
