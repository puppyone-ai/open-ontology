-- Publication admission remains live; sync health is an independent state.
ALTER TABLE _oo.table_freshness
 ADD COLUMN IF NOT EXISTS sync_state text NOT NULL DEFAULT 'fresh'
    CHECK (sync_state IN ('fresh','noop','stale')),
 ADD COLUMN IF NOT EXISTS stale_reason text,
 ADD COLUMN IF NOT EXISTS stale_since timestamptz;
ALTER TABLE _oo_load.gateway_publications
 ADD COLUMN IF NOT EXISTS stale_tables jsonb NOT NULL DEFAULT '[]'::jsonb;

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
  OR candidate.stale_tables IS DISTINCT FROM coalesce(p_shard->'receipt'->'stale_tables','[]'::jsonb)
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
   WHERE run=coalesce(entry->>'retained_run',candidate.run) AND relation_name=vector->>'storage_relation' FOR SHARE;
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
  applied_at=publication.published_at,state='live',
  sync_state=coalesce(t->>'status','fresh'),stale_reason=t->>'stale_reason',
  stale_since=(t->>'stale_since')::timestamptz
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
