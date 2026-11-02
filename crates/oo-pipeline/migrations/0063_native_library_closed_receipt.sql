ALTER FUNCTION _oo.publish_native_library(jsonb,text,jsonb) RENAME TO native_library_publish_checked_vector;
REVOKE ALL ON FUNCTION _oo.native_library_publish_checked_vector(jsonb,text,jsonb) FROM PUBLIC,oo_pipeline_runtime;
DO $body$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_target_admin') THEN
  REVOKE ALL ON FUNCTION _oo.native_library_publish_checked_vector(jsonb,text,jsonb) FROM oo_target_admin;
 END IF;
END $body$;

CREATE FUNCTION _oo.publish_native_library(p_library jsonb,p_sha text,p_shards jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
DECLARE member jsonb; shard jsonb; table_entry jsonb;
BEGIN
 IF jsonb_typeof(p_library->'manifest') IS DISTINCT FROM 'object'
  OR p_library->'manifest'->>'protocol' IS DISTINCT FROM 'oo.library.membership/v1'
  OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_library->'manifest') key)
   IS DISTINCT FROM ARRAY['library_id','protocol','sources']::text[]
  OR jsonb_typeof(p_library->'manifest'->'sources') IS DISTINCT FROM 'array'
  OR jsonb_typeof(p_shards) IS DISTINCT FROM 'array'
 THEN RAISE EXCEPTION 'closed library membership required' USING ERRCODE='OO030'; END IF;
 FOR member IN SELECT value FROM jsonb_array_elements(p_library->'manifest'->'sources') LOOP
  IF jsonb_typeof(member) IS DISTINCT FROM 'object'
   OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(member) key)
    IS DISTINCT FROM ARRAY['schema','source_id','tables']::text[]
   OR jsonb_typeof(member->'source_id') IS DISTINCT FROM 'string'
   OR jsonb_typeof(member->'schema') IS DISTINCT FROM 'string'
   OR coalesce(member->>'schema','')='' OR coalesce(member->>'source_id','')=''
   OR jsonb_typeof(member->'tables') IS DISTINCT FROM 'array'
   OR EXISTS(SELECT 1 FROM jsonb_array_elements(member->'tables') WHERE jsonb_typeof(value)<>'string' OR value#>>'{}'='')
  THEN RAISE EXCEPTION 'closed library source required' USING ERRCODE='OO030'; END IF;
 END LOOP;
 FOR shard IN SELECT value FROM jsonb_array_elements(p_shards) LOOP
  IF jsonb_typeof(shard->'receipt') IS DISTINCT FROM 'object'
   OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(shard->'receipt') key)
    IS DISTINCT FROM ARRAY['applied_pos','collection_mode','consistency','consistency_grade','cross_table_consistent','position_codec','position_lineage','protocol','run_id','run_ids','source_generation','source_id','tables','target_generation']::text[]
   OR jsonb_typeof(shard->'receipt'->'tables') IS DISTINCT FROM 'array'
  THEN RAISE EXCEPTION 'closed library source receipt required' USING ERRCODE='OO030'; END IF;
  FOR table_entry IN SELECT value FROM jsonb_array_elements(shard->'receipt'->'tables') LOOP
   IF jsonb_typeof(table_entry) IS DISTINCT FROM 'object'
    OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(table_entry) key)
     IS DISTINCT FROM ARRAY['accepted_rows','applied_pos','canonical_digest','capture_method','consistency_grade','delete_detection','physical_name','run_id','schema','source_generation','table','target_generation']::text[]
   THEN RAISE EXCEPTION 'closed library table receipt required' USING ERRCODE='OO030'; END IF;
  END LOOP;
 END LOOP;
 RETURN _oo.native_library_publish_checked_vector(p_library,p_sha,p_shards);
EXCEPTION WHEN invalid_parameter_value THEN
 RAISE EXCEPTION 'closed library object required' USING ERRCODE='OO030';
END
$body$;
REVOKE ALL ON FUNCTION _oo.publish_native_library(jsonb,text,jsonb) FROM PUBLIC,oo_pipeline_runtime;
DO $body$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_target_admin') THEN
  GRANT EXECUTE ON FUNCTION _oo.publish_native_library(jsonb,text,jsonb) TO oo_target_admin;
 END IF;
END $body$;
