-- native-c5-digest-policy/v1
-- Policy qualifies the no-scan shortcut; the underlying digest still commits values.
ALTER FUNCTION _oo.c5_observe_partial(text,uuid,bigint,text,text,text,bigint,jsonb)
 RENAME TO c5_observe_partial_before_policy;
REVOKE ALL ON FUNCTION _oo.c5_observe_partial_before_policy(text,uuid,bigint,text,text,text,bigint,jsonb)
 FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_observe_partial(p_shard text,p_owner uuid,p_fence bigint,p_source text,
 p_schema text,p_table text,p_generation bigint,p_observation jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE result jsonb; policy jsonb:=coalesce(p_observation->'type_policy',
 '{"zero_date_to_null":false,"strip_text_nul":false}'::jsonb);
BEGIN
 IF jsonb_typeof(policy) IS DISTINCT FROM 'object' THEN
  RAISE EXCEPTION 'C5 digest policy shape' USING ERRCODE='22023';
 END IF;
 IF (SELECT count(*) FROM jsonb_object_keys(policy))<>2
  OR jsonb_typeof(policy->'zero_date_to_null') IS DISTINCT FROM 'boolean'
  OR jsonb_typeof(policy->'strip_text_nul') IS DISTINCT FROM 'boolean'
 THEN RAISE EXCEPTION 'C5 digest policy shape' USING ERRCODE='22023'; END IF;
 result:=_oo.c5_observe_partial_before_policy(p_shard,p_owner,p_fence,p_source,
  p_schema,p_table,p_generation,p_observation);
 UPDATE _oo.c5_chunk AS ch SET digest_receipt=digest_receipt||jsonb_build_object('type_policy',policy)
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table
   AND chunk_id=p_observation->'chunk'->>'chunk_id' AND retired_at IS NULL
   AND _oo.c5_digest_receipt_current(ch);
 IF NOT FOUND THEN RAISE EXCEPTION 'C5 digest policy requires confirmed receipt' USING ERRCODE='OO030'; END IF;
 RETURN result;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_observe_partial(text,uuid,bigint,text,text,text,bigint,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_observe_partial(text,uuid,bigint,text,text,text,bigint,jsonb)
 TO oo_pipeline_runtime;

ALTER FUNCTION _oo.c5_authority_export(text,text,text) RENAME TO c5_authority_export_before_policy;
REVOKE ALL ON FUNCTION _oo.c5_authority_export_before_policy(text,text,text) FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_authority_export(p_source text,p_schema text,p_table text)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE result jsonb; chunks jsonb;
BEGIN
 result:=_oo.c5_authority_export_before_policy(p_source,p_schema,p_table);
 IF result IS NULL OR result->'empty_only'='true'::jsonb THEN RETURN result; END IF;
 SELECT jsonb_agg(c.value||CASE WHEN _oo.c5_digest_receipt_current(ch)
   AND ch.digest_receipt ? 'type_policy'
  THEN jsonb_build_object('type_policy',ch.digest_receipt->'type_policy') ELSE '{}'::jsonb END
  ORDER BY c.ordinality) INTO chunks
 FROM jsonb_array_elements(result->'chunks') WITH ORDINALITY c
 JOIN _oo.c5_chunk ch ON ch.source=p_source AND ch.schema_name=p_schema
  AND ch.table_name=p_table AND ch.chunk_id=c.value->>'chunk_id' AND ch.retired_at IS NULL;
 IF chunks IS NOT NULL THEN result:=jsonb_set(result,'{chunks}',chunks); END IF;
 RETURN result;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_authority_export(text,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_authority_export(text,text,text) TO oo_pipeline_runtime;
