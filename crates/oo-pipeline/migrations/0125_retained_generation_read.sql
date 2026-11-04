-- Read-only proof projection for Gateway; no source reads or activation side effects.
ALTER TABLE _oo_load.gateway_publications ADD COLUMN IF NOT EXISTS activated_at timestamptz;

CREATE OR REPLACE FUNCTION _oo.native_library_retained_table(p_physical text) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
DECLARE c _oo_load.gateway_publications%ROWTYPE; t jsonb; proof jsonb;
BEGIN
 SELECT g.* INTO STRICT c FROM _oo_load.gateway_publications g
 JOIN _oo.native_library_head h ON h.library_id=g.library_id AND h.state='ready'
 WHERE (h.receipt_sha256=g.receipt_sha256 OR g.activated_at IS NOT NULL)
 AND g.sealed_at IS NOT NULL AND EXISTS(
  SELECT 1 FROM jsonb_array_elements(g.shard->'receipt'->'tables') e WHERE e->>'physical_name'=p_physical);
 SELECT value INTO STRICT t FROM jsonb_array_elements(c.shard->'receipt'->'tables')
  WHERE value->>'physical_name'=p_physical;
 FOR proof IN SELECT value FROM jsonb_array_elements(c.relation_proofs) LOOP
  IF proof IS DISTINCT FROM _oo.native_library_relation_proof(proof->>'schema',proof->>'relation') THEN
   RETURN NULL;
  END IF;
 END LOOP;
 RETURN t||jsonb_build_object('source_id',c.source,'config_digest',c.request->>'binding_sha256',
  'library_receipt_sha256',c.receipt_sha256,'applied_at',
  (SELECT to_char(published_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
   FROM _oo.native_library_publication WHERE receipt_sha256=c.receipt_sha256));
EXCEPTION WHEN no_data_found OR too_many_rows THEN RETURN NULL;
END $body$;
REVOKE ALL ON FUNCTION _oo.native_library_retained_table(text) FROM PUBLIC,oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_library_retained_table(text) TO oo_target_owner;
