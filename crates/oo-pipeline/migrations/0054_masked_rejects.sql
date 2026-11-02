CREATE SCHEMA sync AUTHORIZATION oo_target_owner;
REVOKE ALL ON SCHEMA sync FROM PUBLIC;
CREATE TABLE sync.rejects (
 source text NOT NULL, schema_name text NOT NULL, "table" text NOT NULL, pk text NOT NULL, "column" text NOT NULL,
 reason text NOT NULL, raw_value jsonb NOT NULL, pos text NOT NULL DEFAULT '0',
 first_seen_at timestamptz NOT NULL DEFAULT clock_timestamp(),last_seen_at timestamptz NOT NULL DEFAULT clock_timestamp(),count bigint NOT NULL DEFAULT 1,
 resolved_at timestamptz, PRIMARY KEY(source,schema_name,"table",pk,"column"),
 CHECK (pk ~ '^[0-9a-f]{64}$'), CHECK (raw_value->>'masked'='true'), CHECK(count>0)
);
CREATE TABLE sync.reject_observations(source text NOT NULL,observation_sha256 text NOT NULL,records jsonb NOT NULL,PRIMARY KEY(source,observation_sha256));
REVOKE ALL ON ALL TABLES IN SCHEMA sync FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.record_masked_rejects(p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,p_observation text,p_records jsonb)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo,sync AS $$
DECLARE r jsonb; old jsonb; n bigint:=0;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 IF p_source !~ '^[a-z][a-z0-9_]{0,62}$' OR p_schema IS NULL OR p_table IS NULL OR p_observation !~ '^[0-9a-f]{64}$'
  OR jsonb_typeof(p_records) IS DISTINCT FROM 'array' OR jsonb_array_length(p_records) NOT BETWEEN 1 AND 10000
 THEN RAISE EXCEPTION 'masked reject scope'; END IF;
 INSERT INTO sync.reject_observations VALUES(p_source,p_observation,p_records) ON CONFLICT DO NOTHING;
 IF NOT FOUND THEN
  SELECT records INTO STRICT old FROM sync.reject_observations WHERE source=p_source AND observation_sha256=p_observation;
  IF old IS DISTINCT FROM p_records THEN RAISE EXCEPTION 'reject observation conflict' USING ERRCODE='OO010'; END IF;
  RETURN 0;
 END IF;
 FOR r IN SELECT value FROM jsonb_array_elements(p_records) LOOP
  IF jsonb_typeof(r) IS DISTINCT FROM 'object' OR (SELECT count(*) FROM jsonb_object_keys(r))<>4
   OR r->>'pk' !~ '^[0-9a-f]{64}$' OR r->>'column' !~ '^[A-Za-z0-9_*]{1,64}$'
   OR r->>'reason' NOT IN ('charset','row_budget_exceeded','zero_date','bool_domain','nul_byte','invalid_date','date_range','int_range','decimal_range','decimal_text','parse_error','out_of_range','invalid_json')
   OR r->'raw_value' IS DISTINCT FROM jsonb_build_object('masked',true,'value_len',r->'raw_value'->'value_len')
   OR (r->'raw_value'->'value_len'<>'null'::jsonb AND (r->'raw_value'->>'value_len')::bigint<0)
  THEN RAISE EXCEPTION 'only masked rejects accepted'; END IF;
  INSERT INTO sync.rejects(source,schema_name,"table",pk,"column",reason,raw_value) VALUES(p_source,p_schema,p_table,r->>'pk',r->>'column',r->>'reason',r->'raw_value')
   ON CONFLICT(source,schema_name,"table",pk,"column") DO UPDATE SET reason=EXCLUDED.reason,raw_value=EXCLUDED.raw_value,last_seen_at=clock_timestamp(),count=sync.rejects.count+1,resolved_at=NULL;
  n:=n+1;
 END LOOP;
 RETURN n;
END $$;
REVOKE ALL ON FUNCTION _oo.record_masked_rejects(text,uuid,bigint,text,text,text,text,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.record_masked_rejects(text,uuid,bigint,text,text,text,text,jsonb) TO oo_pipeline_runtime;
