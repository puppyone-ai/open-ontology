-- D15: owner-verified retained cuts do not impersonate legacy import/ACK rows.
ALTER TABLE _oo_load.gateway_publications
 ADD COLUMN IF NOT EXISTS shard jsonb,
 ADD COLUMN IF NOT EXISTS data_vector jsonb,
 ADD COLUMN IF NOT EXISTS data_vector_sha256 text,
 ADD COLUMN IF NOT EXISTS relation_proofs jsonb,
 ADD COLUMN IF NOT EXISTS sealed_at timestamptz;

CREATE OR REPLACE FUNCTION _oo.native_library_relation_proof(p_schema text,p_relation text)
RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
 SELECT jsonb_build_object('schema',n.nspname,'relation',c.relname,'kind',c.relkind,
  'definition',CASE WHEN c.relkind='v' THEN pg_get_viewdef(c.oid,false) ELSE NULL END,
  'columns',(SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull)
   ORDER BY a.attnum) FROM pg_attribute a WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped))
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname=p_schema AND c.relname=p_relation
$body$;
REVOKE ALL ON FUNCTION _oo.native_library_relation_proof(text,text) FROM PUBLIC,oo_pipeline_runtime;

CREATE OR REPLACE FUNCTION _oo.native_library_check_retained(p_shard jsonb) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
DECLARE candidate _oo_load.gateway_publications%ROWTYPE; publication _oo.native_library_publication%ROWTYPE;
 entry jsonb; vector jsonb; proof jsonb; g _oo_load.generations%ROWTYPE; n integer;
BEGIN
 SELECT * INTO STRICT candidate FROM _oo_load.gateway_publications
  WHERE source=p_shard->>'source_id' AND run=p_shard->>'anchor_run' FOR SHARE;
 SELECT * INTO STRICT publication FROM _oo.native_library_publication
  WHERE receipt_sha256=candidate.receipt_sha256;
 IF candidate.sealed_at IS NULL OR candidate.shard IS DISTINCT FROM p_shard
  OR publication.shards IS DISTINCT FROM jsonb_build_array(p_shard)
  OR publication.library_id IS DISTINCT FROM candidate.library_id
  OR candidate.receipt_sha256 IS DISTINCT FROM encode(sha256(convert_to(
   _oo.native_library_canonical(jsonb_build_object('library',publication.library,'shards',publication.shards)),'UTF8')),'hex')
  OR candidate.data_vector_sha256 IS DISTINCT FROM encode(sha256(convert_to(
   _oo.native_library_canonical(candidate.data_vector),'UTF8')),'hex')
  OR jsonb_typeof(candidate.data_vector) IS DISTINCT FROM 'array'
  OR jsonb_typeof(candidate.relation_proofs) IS DISTINCT FROM 'array'
  OR jsonb_array_length(candidate.relation_proofs)<2
  OR jsonb_array_length(candidate.data_vector) IS DISTINCT FROM jsonb_array_length(p_shard->'receipt'->'tables')
 THEN RAISE EXCEPTION 'retained candidate identity or seal differs' USING ERRCODE='OO030'; END IF;
 FOR proof IN SELECT value FROM jsonb_array_elements(candidate.relation_proofs) LOOP
  -- Prevent DDL from changing a verified relation before activation commits.
  EXECUTE format('LOCK TABLE %I.%I IN ACCESS SHARE MODE',proof->>'schema',proof->>'relation');
  IF proof IS DISTINCT FROM _oo.native_library_relation_proof(proof->>'schema',proof->>'relation') THEN
   RAISE EXCEPTION 'retained view definition or columns differ' USING ERRCODE='OO030'; END IF;
 END LOOP;
 FOR entry IN SELECT value FROM jsonb_array_elements(p_shard->'receipt'->'tables') LOOP
  SELECT value INTO STRICT vector FROM jsonb_array_elements(candidate.data_vector)
   WHERE value->>'physical_name'=entry->>'physical_name';
  SELECT * INTO STRICT g FROM _oo_load.generations
   WHERE run=candidate.run AND relation_name=vector->>'storage_relation' FOR SHARE;
  IF g.sealed_at IS NULL OR entry->>'recipe_sha256' IS DISTINCT FROM encode(sha256(convert_to(
    _oo.native_library_canonical(g.recipe),'UTF8')),'hex')
   OR vector->>'recipe_sha256' IS DISTINCT FROM entry->>'recipe_sha256'
   OR vector->>'source_id' IS DISTINCT FROM candidate.source
   OR (vector->>'generation')::bigint IS DISTINCT FROM candidate.generation
   OR (entry->>'source_generation')::bigint IS DISTINCT FROM candidate.generation
   OR (entry->>'target_generation')::bigint IS DISTINCT FROM candidate.generation
   OR vector->>'dataset' IS DISTINCT FROM (entry->>'storage_schema')||'.'||(entry->>'storage_table')
   OR NOT candidate.request->'recipes' @> jsonb_build_array(g.recipe)
  THEN RAISE EXCEPTION 'retained recipe or data vector differs' USING ERRCODE='OO030'; END IF;
 END LOOP;
 UPDATE _oo.source_watermark SET consistency='per_table',consistency_grade='B',
  applied_pos=0,source_committed_at=NULL,applied_at=publication.published_at,state='live',
  cross_table_consistent=false,source_generation=candidate.generation,target_generation=candidate.generation,
  position_codec='snapshot-zero/v1',position_lineage='oo-load.retained' WHERE source=candidate.source;
 GET DIAGNOSTICS n=ROW_COUNT;
 IF n<>1 THEN RAISE EXCEPTION 'retained watermark missing' USING ERRCODE='OO030'; END IF;
 UPDATE _oo.table_freshness f SET source_generation=candidate.generation,target_generation=candidate.generation,
  physical_name=t->>'physical_name',last_pos=0,source_committed_at=NULL,
  applied_at=publication.published_at,state='live'
 FROM jsonb_array_elements(p_shard->'receipt'->'tables') t
 WHERE f.source=candidate.source AND f.schema_name=t->>'storage_schema' AND f.table_name=t->>'storage_table';
 GET DIAGNOSTICS n=ROW_COUNT;
 IF n<>jsonb_array_length(candidate.data_vector) THEN
  RAISE EXCEPTION 'retained freshness coverage differs' USING ERRCODE='OO030'; END IF;
EXCEPTION WHEN no_data_found OR too_many_rows OR invalid_text_representation
 OR numeric_value_out_of_range OR undefined_table OR invalid_parameter_value THEN
 RAISE EXCEPTION 'retained proof missing or ambiguous' USING ERRCODE='OO030';
END $body$;
REVOKE ALL ON FUNCTION _oo.native_library_check_retained(jsonb) FROM PUBLIC,oo_pipeline_runtime;
GRANT USAGE ON SCHEMA _oo_load TO oo_target_owner;
GRANT SELECT ON _oo_load.gateway_publications,_oo_load.generations TO oo_target_owner;
GRANT EXECUTE ON FUNCTION _oo.native_library_check_retained(jsonb),
 _oo.native_library_relation_proof(text,text) TO oo_target_owner;
CREATE OR REPLACE FUNCTION _oo.native_library_check_shard(p_shard jsonb) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
BEGIN
 IF p_shard->'receipt'->>'consistency'='retained_generation' THEN
  PERFORM _oo.native_library_check_retained(p_shard);
 ELSIF p_shard->'receipt'->>'consistency'='transactional' THEN
  PERFORM 1 FROM _oo.source_instance WHERE source=p_shard->>'source_id' FOR SHARE;
  -- Target owner may read the terminal control receipt, never acquire jobs write privileges.
  PERFORM 1 FROM _oo.native_c1_baseline_typed bt
   JOIN _oo.native_c1_baseline_run br ON br.source=bt.source AND br.task_id=bt.task_id
   JOIN jobs.tasks j ON j.id=bt.task_id WHERE bt.source=p_shard->>'source_id'
   ORDER BY bt.task_id FOR SHARE OF bt,br;
  PERFORM 1 FROM _oo.full_import_run r JOIN _oo.full_import_table t USING(run_id)
   WHERE r.source_id=p_shard->>'source_id' ORDER BY r.run_id FOR SHARE OF r,t;
  IF p_shard IS DISTINCT FROM _oo.native_library_c1_shard(p_shard->>'source_id') THEN
   RAISE EXCEPTION 'C1 library exact original shard required' USING ERRCODE='OO030'; END IF;
  PERFORM _oo.publish_native_c1_collection((p_shard->>'anchor_run')::uuid,p_shard->>'receipt_sha256',p_shard->'receipt');
 ELSE
  PERFORM _oo.publish_native_per_table_collection((p_shard->>'anchor_run')::uuid,p_shard->>'receipt_sha256',p_shard->'receipt');
 END IF;
END $body$;
REVOKE ALL ON FUNCTION _oo.native_library_check_shard(jsonb) FROM PUBLIC,oo_pipeline_runtime;

CREATE OR REPLACE FUNCTION _oo.native_library_source_ready(p_source text,p_receipt text) RETURNS boolean
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
 BEGIN RETURN NOT EXISTS(SELECT 1 FROM _oo.native_library_member WHERE source_id=p_source)
 OR EXISTS(
  SELECT 1 FROM _oo.native_library_member m JOIN _oo.native_library_head h USING(library_id)
  JOIN _oo.native_library_publication p ON p.receipt_sha256=h.receipt_sha256
  WHERE m.source_id=p_source AND h.state='ready'
   AND EXISTS(SELECT 1 FROM jsonb_array_elements(p.shards) s WHERE s->>'source_id'=p_source AND s->>'receipt_sha256'=p_receipt)
   AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p.shards) s
    WHERE s->'receipt'->>'consistency'='transactional' AND NOT _oo.native_library_c1_current(s))
   AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p.shards) s LEFT JOIN _oo.source_watermark w ON w.source=s->>'source_id'
    WHERE w.source IS NULL OR w.state<>'live' OR w.source_generation IS DISTINCT FROM (s->'receipt'->>'source_generation')::bigint
     OR w.target_generation IS DISTINCT FROM (s->'receipt'->>'target_generation')::bigint
     OR (s->'receipt'->>'consistency'='retained_generation' AND
      (w.consistency IS DISTINCT FROM 'per_table' OR w.consistency_grade IS DISTINCT FROM 'B'
       OR w.cross_table_consistent OR w.applied_pos IS DISTINCT FROM 0
       OR w.position_codec IS DISTINCT FROM 'snapshot-zero/v1'
       OR w.position_lineage IS DISTINCT FROM 'oo-load.retained' OR w.source_committed_at IS NOT NULL)))
   AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p.shards) s CROSS JOIN LATERAL jsonb_array_elements(s->'receipt'->'tables') t
    LEFT JOIN _oo.table_freshness f ON f.source=s->>'source_id' AND f.schema_name=coalesce(t->>'storage_schema',t->>'schema') AND f.table_name=coalesce(t->>'storage_table',t->>'table')
    WHERE f.source IS NULL OR f.state<>'live' OR f.source_generation IS DISTINCT FROM (t->>'source_generation')::bigint
     OR f.target_generation IS DISTINCT FROM (t->>'target_generation')::bigint OR f.physical_name IS DISTINCT FROM t->>'physical_name'
     OR (s->'receipt'->>'consistency'='retained_generation' AND
      (f.last_pos IS DISTINCT FROM 0 OR f.source_committed_at IS NOT NULL)))
 ); END
$body$;

CREATE OR REPLACE FUNCTION _oo.native_library_mark_ready(p_library text,p_sha text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
DECLARE publication _oo.native_library_publication%ROWTYPE; shard jsonb; missing boolean;
BEGIN
 PERFORM 1 FROM _oo.native_library_head WHERE library_id=p_library AND receipt_sha256=p_sha FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'current library pointer required' USING ERRCODE='OO030'; END IF;
 SELECT * INTO publication FROM _oo.native_library_publication WHERE receipt_sha256=p_sha;
 PERFORM 1 FROM _oo.source_watermark
  WHERE source IN (SELECT value->>'source_id' FROM jsonb_array_elements(publication.shards))
  ORDER BY source FOR UPDATE;
 PERFORM 1 FROM _oo.table_freshness
  WHERE source IN (SELECT value->>'source_id' FROM jsonb_array_elements(publication.shards))
  ORDER BY source,schema_name,table_name FOR UPDATE;
 FOR shard IN SELECT value FROM jsonb_array_elements(publication.shards) ORDER BY value->>'source_id' LOOP
  PERFORM _oo.native_library_check_shard(shard);
 END LOOP;
 IF to_regclass('oo_publish_target.fence') IS NULL THEN
  RAISE EXCEPTION 'library semantic fences absent' USING ERRCODE='OO030'; END IF;
 EXECUTE 'SELECT EXISTS(SELECT 1 FROM jsonb_array_elements($1) s CROSS JOIN LATERAL jsonb_array_elements(s->''receipt''->''tables'') t WHERE NOT EXISTS(SELECT 1 FROM oo_publish_target.fence f WHERE f.schema_name=''_oo'' AND f.relation_name=t->>''physical_name'' AND f.state=''open'' AND f.opened_pack_digest ~ ''^[0-9a-f]{64}$'' AND f.opened_version IS NOT NULL))' INTO missing USING publication.shards;
 IF missing THEN RAISE EXCEPTION 'all library fences must be open' USING ERRCODE='OO030'; END IF;
 UPDATE _oo.native_library_head SET state='ready' WHERE library_id=p_library;
END
$body$;
REVOKE ALL ON FUNCTION _oo.native_library_source_ready(text,text),_oo.native_library_mark_ready(text,text) FROM PUBLIC,oo_pipeline_runtime;
DO $grant$
BEGIN
 IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_target_admin') THEN
  GRANT EXECUTE ON FUNCTION _oo.native_library_source_ready(text,text),_oo.native_library_mark_ready(text,text) TO oo_target_admin;
 END IF;
END
$grant$;
