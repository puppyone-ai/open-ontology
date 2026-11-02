-- Only the C1 final-load path can promote an actually seeded Oracle delta.
CREATE FUNCTION _oo.c1_oracle_delta_final_assert(s text,t uuid,g bigint,e integer)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE d _oo.c1_typed_delta_seed%ROWTYPE; r _oo.native_c1_baseline_run%ROWTYPE;
  b _oo.c1_typed_capture_bundle%ROWTYPE;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  SELECT * INTO d FROM _oo.c1_typed_delta_seed WHERE source=s AND task=t;
  IF NOT FOUND OR NOT d.seeded THEN
    RAISE EXCEPTION 'Oracle delta requires committed parent seed' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO r FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
  IF NOT FOUND OR r.engine<>'oracle' OR r.state<>'awaiting_publish'
    OR r.lease_generation<>g OR r.source_epoch<>e
    OR r.config_digest IS DISTINCT FROM d.previous->>'config_digest'
    OR r.schema_digest IS DISTINCT FROM d.previous->>'schema_digest'
    OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=t AND generation=g
      AND state='running' AND lease_until>clock_timestamp())
    OR NOT EXISTS(SELECT 1 FROM _oo.source_instance WHERE source=s AND source_epoch=e
      AND engine='oracle' AND lease_id=t AND lease_generation=g) THEN
    RAISE EXCEPTION 'Oracle delta final lease drift' USING ERRCODE='OO010';
  END IF;
  SELECT * INTO b FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t;
  IF NOT FOUND OR b.metadata->>'rollover_sha256' IS DISTINCT FROM d.rollover_sha256
    OR b.metadata->>'parent_request_digest' IS DISTINCT FROM d.previous->>'request_digest'
    OR b.final_apply_ordinal IS DISTINCT FROM r.l_end
    OR NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_v2_format
      WHERE source=s AND task_id=t AND sealed)
    OR EXISTS(SELECT 1 FROM _oo.c1_typed_table WHERE source=s AND task=t AND NOT snapshot_complete) THEN
    RAISE EXCEPTION 'Oracle delta final seed binding drift' USING ERRCODE='OO030';
  END IF;
END $fn$;
REVOKE ALL ON FUNCTION _oo.c1_oracle_delta_final_assert(text,uuid,bigint,integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c1_oracle_delta_final_assert(text,uuid,bigint,integer) TO oo_pipeline_runtime;
