-- native-c5-empty-baseline/v1: zero-only confirmation, never a range fallback.
CREATE TABLE _oo.c5_empty_baseline (
 source text NOT NULL, schema_name text NOT NULL, table_name text NOT NULL,
 run_id uuid NOT NULL REFERENCES _oo.full_import_run(run_id),
 schema_fingerprint text NOT NULL CHECK(schema_fingerprint ~ '^[0-9a-f]{64}$'),
 authority jsonb NOT NULL,
 observation_id uuid NOT NULL, lease_id uuid NOT NULL,
 lease_generation bigint NOT NULL CHECK(lease_generation>0),
 observation_source_generation bigint NOT NULL CHECK(observation_source_generation>0),
 issued_at timestamptz NOT NULL,
 last_confirmation jsonb, last_empty_confirmed_at timestamptz,
 PRIMARY KEY(source,schema_name,table_name)
);
REVOKE ALL ON _oo.c5_empty_baseline FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_empty_deny_range_init() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
BEGIN
 IF EXISTS(SELECT FROM _oo.c5_empty_baseline WHERE source=NEW.source
  AND schema_name=NEW.schema_name AND table_name=NEW.table_name)
 THEN RAISE EXCEPTION 'C5 empty-only baseline cannot initialize ranges' USING ERRCODE='OO030'; END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_empty_deny_range_init() FROM PUBLIC,oo_pipeline_runtime;
CREATE TRIGGER c5_empty_deny_range_init BEFORE INSERT OR UPDATE ON _oo.c5_table
 FOR EACH ROW EXECUTE FUNCTION _oo.c5_empty_deny_range_init();

CREATE FUNCTION _oo.c5_empty_baseline_proof(p_source text,p_schema text,p_table text)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE f _oo.table_freshness; r _oo.full_import_run; t _oo.full_import_table;
 high bigint; relation oid; proof jsonb; selected jsonb; columns jsonb; projection text;
BEGIN
 SELECT source_generation INTO STRICT high FROM _oo.source_watermark WHERE source=p_source;
 SELECT * INTO STRICT f FROM _oo.table_freshness
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
 IF f.source_generation<1 OR f.source_generation>high OR f.mode IS DISTINCT FROM 'full'
  OR f.state NOT IN ('awaiting_publish','live')
  OR EXISTS(SELECT FROM _oo.c5_table WHERE source=p_source AND schema_name=p_schema AND table_name=p_table)
 THEN RAISE EXCEPTION 'C5 empty baseline scope differs' USING ERRCODE='OO030'; END IF;
 SELECT run.* INTO STRICT r FROM _oo.full_import_run run JOIN _oo.full_import_table ft USING(run_id)
  WHERE run.source_id=p_source AND run.source_generation=f.source_generation
   AND run.target_generation=f.target_generation AND ft.physical_name=f.physical_name
   AND ft.schema_ident=p_schema AND ft.table_ident=p_table;
 SELECT * INTO STRICT t FROM _oo.full_import_table WHERE run_id=r.run_id;
 proof:=_oo.c5_single_baseline_receipt(r.run_id);
 IF t.applied_rows IS DISTINCT FROM 0 OR proof->'row_count' IS DISTINCT FROM '0'::jsonb
 THEN RAISE EXCEPTION 'C5 empty baseline is not empty' USING ERRCODE='OO030'; END IF;
 relation:=to_regclass(format('_oo.%I',t.physical_name));
 IF relation IS NULL OR NOT EXISTS(SELECT FROM pg_class WHERE oid=relation AND relkind='r'
   AND relowner='oo_target_owner'::regrole)
 THEN RAISE EXCEPTION 'C5 empty baseline relation differs' USING ERRCODE='OO030'; END IF;
 SELECT jsonb_agg(m.logical_column ORDER BY a.attnum),
  jsonb_agg(jsonb_build_object('name',m.logical_column,'target_oid',a.atttypid::integer,
   'target_typmod',a.atttypmod,'nullable',NOT a.attnotnull) ORDER BY a.attnum),
  encode(sha256(convert_to('['||string_agg(to_json(m.logical_column)::text,',' ORDER BY a.attnum)||']','UTF8')),'hex')
 INTO selected,columns,projection FROM pg_attribute a JOIN _oo.physical_mappings m
  ON m.physical_name=a.attname AND m.source=p_source AND m.config_digest=r.config_digest
   AND m.run_generation=r.source_generation AND m.level='c'
   AND m.logical_namespace=p_schema AND m.logical_table=p_table
 WHERE a.attrelid=relation AND a.attnum>0 AND NOT a.attisdropped;
 IF selected IS NULL OR jsonb_array_length(selected) NOT BETWEEN 1 AND 1024
  OR jsonb_array_length(selected)<>(SELECT count(*) FROM pg_attribute WHERE attrelid=relation
    AND attnum>0 AND NOT attisdropped AND attname<>'_oo_pos')
 THEN RAISE EXCEPTION 'C5 empty baseline projection differs' USING ERRCODE='OO030'; END IF;
 RETURN jsonb_build_object('baseline_receipt',proof,'selected_columns',selected,
  'columns',columns,'projection_sha256',projection);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_empty_baseline_proof(text,text,text) FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_empty_lock(p_shard text,p_owner uuid,p_fence bigint,
 p_source text,p_schema text,p_table text,p_generation bigint,p_lease uuid,p_lease_generation bigint)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE high bigint;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 -- Native State holds jobs.loader_hold's row lock for this entire transaction.
 IF p_lease IS NULL OR p_lease_generation IS NULL OR p_lease_generation<1
  OR NOT EXISTS(SELECT FROM jobs.tasks WHERE id=p_lease AND generation=p_lease_generation
   AND state='running' AND lease_owner IS NOT NULL AND lease_until>clock_timestamp())
 THEN RAISE EXCEPTION 'C5 empty observation lease differs' USING ERRCODE='OO010'; END IF;
 SELECT source_generation INTO STRICT high FROM _oo.source_watermark WHERE source=p_source FOR UPDATE;
 IF p_generation IS NULL OR p_generation<1 OR high IS DISTINCT FROM p_generation
 THEN RAISE EXCEPTION 'C5 empty observation source CAS differs' USING ERRCODE='OO010'; END IF;
 PERFORM 1 FROM _oo.table_freshness
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'C5 empty table missing' USING ERRCODE='OO030'; END IF;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_empty_lock(text,uuid,bigint,text,text,text,bigint,uuid,bigint)
 FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_empty_authority_export(p_source text,p_schema text,p_table text)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE state _oo.c5_empty_baseline; proof jsonb; high bigint;
BEGIN
 SELECT * INTO STRICT state FROM _oo.c5_empty_baseline
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
 proof:=_oo.c5_empty_baseline_proof(p_source,p_schema,p_table);
 IF proof IS DISTINCT FROM state.authority
  OR proof->'baseline_receipt'->>'run_id' IS DISTINCT FROM state.run_id::text
 THEN RAISE EXCEPTION 'C5 empty baseline identity changed' USING ERRCODE='OO010'; END IF;
 SELECT source_generation INTO STRICT high FROM _oo.source_watermark WHERE source=p_source;
 RETURN proof||jsonb_build_object('protocol','oo.pipeline.native-c5-empty-authority/v1',
  'empty_only',true,'source_id',p_source,'schema',p_schema,'table',p_table,
  'source_generation',high,'table_source_generation',proof->'baseline_receipt'->'source_generation',
  'table_physical_name',proof->'baseline_receipt'->'physical_name',
  'schema_fingerprint',state.schema_fingerprint,'chunk_topology_revision',1,
  'input_digest',proof->'baseline_receipt'->'input_digest',
  'type_policy_version',proof->'baseline_receipt'->'type_policy_version',
  'type_digest',proof->'baseline_receipt'->'type_digest',
  'key_columns','[]'::jsonb,'chunks','[]'::jsonb,'next_chunk_index',0,
  'scan_id',NULL,'expected_chunks',NULL,'scan_layers',NULL,'scan_observed_at_ms',NULL,
  'empty_observation',jsonb_build_object('id',state.observation_id,'lease_id',state.lease_id,
   'lease_generation',state.lease_generation,'source_generation',state.observation_source_generation,
   'issued_at_ms',floor(extract(epoch FROM state.issued_at)*1000)::bigint),
  'last_empty_confirmed_at_ms',floor(extract(epoch FROM state.last_empty_confirmed_at)*1000)::bigint,
  'last_empty_confirmation',state.last_confirmation);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_empty_authority_export(text,text,text) FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_initialize_empty_if_absent(p_shard text,p_owner uuid,p_fence bigint,
 p_source text,p_schema text,p_table text,p_fingerprint text,p_generation bigint,p_lease uuid,p_lease_generation bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE state _oo.c5_empty_baseline; proof jsonb; existed boolean;
BEGIN
 PERFORM _oo.c5_empty_lock(p_shard,p_owner,p_fence,p_source,p_schema,p_table,p_generation,p_lease,p_lease_generation);
 IF p_fingerprint IS NULL OR p_fingerprint !~ '^[0-9a-f]{64}$'
 THEN RAISE EXCEPTION 'C5 empty schema fingerprint required' USING ERRCODE='22023'; END IF;
 proof:=_oo.c5_empty_baseline_proof(p_source,p_schema,p_table);
 SELECT * INTO state FROM _oo.c5_empty_baseline
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 existed:=FOUND;
 IF existed THEN
  IF state.authority IS DISTINCT FROM proof OR state.schema_fingerprint IS DISTINCT FROM p_fingerprint
  THEN RAISE EXCEPTION 'C5 empty baseline changed' USING ERRCODE='OO010'; END IF;
  IF state.lease_id=p_lease AND state.lease_generation=p_lease_generation THEN
   IF state.observation_source_generation IS DISTINCT FROM p_generation
   THEN RAISE EXCEPTION 'C5 empty occurrence source changed' USING ERRCODE='OO010'; END IF;
  ELSE
   IF EXISTS(SELECT FROM jobs.tasks j WHERE j.id=state.lease_id AND j.generation=state.lease_generation
    AND j.state='running' AND j.lease_until>clock_timestamp())
   THEN RAISE EXCEPTION 'C5 empty observation still active' USING ERRCODE='OO010'; END IF;
   UPDATE _oo.c5_empty_baseline SET observation_id=gen_random_uuid(),lease_id=p_lease,
    lease_generation=p_lease_generation,observation_source_generation=p_generation,issued_at=clock_timestamp()
    WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
  END IF;
 ELSE
  INSERT INTO _oo.c5_empty_baseline(source,schema_name,table_name,run_id,schema_fingerprint,authority,
   observation_id,lease_id,lease_generation,observation_source_generation,issued_at)
  VALUES(p_source,p_schema,p_table,(proof->'baseline_receipt'->>'run_id')::uuid,p_fingerprint,proof,
   gen_random_uuid(),p_lease,p_lease_generation,p_generation,clock_timestamp());
 END IF;
 RETURN _oo.c5_empty_authority_export(p_source,p_schema,p_table)||jsonb_build_object('created',NOT existed);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_initialize_empty_if_absent(text,uuid,bigint,text,text,text,text,bigint,uuid,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_initialize_empty_if_absent(text,uuid,bigint,text,text,text,text,bigint,uuid,bigint)
 TO oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_observe_empty(p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,
 p_generation bigint,p_lease uuid,p_lease_generation bigint,p_nochange jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE state _oo.c5_empty_baseline; proof jsonb; obs jsonb; started bigint; checked bigint; replay boolean;
BEGIN
 PERFORM _oo.c5_empty_lock(p_shard,p_owner,p_fence,p_source,p_schema,p_table,p_generation,p_lease,p_lease_generation);
 SELECT * INTO STRICT state FROM _oo.c5_empty_baseline
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 proof:=_oo.c5_empty_baseline_proof(p_source,p_schema,p_table);
 obs:=p_nochange->'c5_observation';
 IF state.authority IS DISTINCT FROM proof OR state.lease_id IS DISTINCT FROM p_lease
  OR state.lease_generation IS DISTINCT FROM p_lease_generation
  OR state.observation_source_generation IS DISTINCT FROM p_generation
  OR p_nochange->>'protocol' IS DISTINCT FROM 'oo.jobs.native-c5-nochange/v1'
  OR p_nochange->>'source_id' IS DISTINCT FROM p_source
  OR p_nochange->'source_generation' IS DISTINCT FROM to_jsonb(p_generation)
  OR p_nochange->'acknowledged_source_generation' IS DISTINCT FROM to_jsonb(p_generation)
  OR p_nochange->>'method' IS DISTINCT FROM 'C5' OR p_nochange->>'capture_method' IS DISTINCT FROM 'c5_diff'
  OR p_nochange->'empty_diff' IS DISTINCT FROM 'true'::jsonb
  OR p_nochange->'data_gen' IS DISTINCT FROM 'false'::jsonb OR p_nochange->'apply' IS DISTINCT FROM 'false'::jsonb
  OR p_nochange->'ack' IS DISTINCT FROM 'false'::jsonb
  OR p_nochange->>'parent_digest' IS DISTINCT FROM proof->'baseline_receipt'->>'input_digest'
  OR coalesce(p_nochange->>'source_digest','') !~ '^[0-9a-f]{64}$'
  OR p_nochange->'snapshot'->>'kind' IS DISTINCT FROM 'c5_cycle'
  OR p_nochange->'snapshot'->>'value' IS DISTINCT FROM p_nochange->>'source_digest'
 THEN RAISE EXCEPTION 'C5 empty nochange identity differs' USING ERRCODE='OO010'; END IF;
 IF jsonb_typeof(obs) IS DISTINCT FROM 'object' THEN
  RAISE EXCEPTION 'C5 empty observation required' USING ERRCODE='22023'; END IF;
 IF (SELECT count(*) FROM jsonb_object_keys(obs))<>20
  OR obs->>'protocol' IS DISTINCT FROM 'oo.c5.empty-observation/v1'
  OR obs->>'source_id' IS DISTINCT FROM p_source OR obs->>'schema' IS DISTINCT FROM p_schema
  OR obs->>'table' IS DISTINCT FROM p_table OR obs->'source_generation' IS DISTINCT FROM to_jsonb(p_generation)
  OR obs->'table_source_generation' IS DISTINCT FROM proof->'baseline_receipt'->'source_generation'
  OR obs->>'schema_fingerprint' IS DISTINCT FROM state.schema_fingerprint
  OR obs->'baseline_receipt' IS DISTINCT FROM proof->'baseline_receipt'
  OR obs->>'observation_id' IS DISTINCT FROM state.observation_id::text
  OR obs->>'lease_id' IS DISTINCT FROM p_lease::text OR obs->'lease_generation' IS DISTINCT FROM to_jsonb(p_lease_generation)
  OR obs->>'source_query' IS DISTINCT FROM format('SELECT 1 FROM `%s`.`%s` LIMIT 1',replace(p_schema,'`','``'),replace(p_table,'`','``'))
  OR obs->'rows_returned' IS DISTINCT FROM '0'::jsonb OR obs->'row_limit' IS DISTINCT FROM '1'::jsonb
  OR obs->'snapshot_complete' IS DISTINCT FROM 'true'::jsonb OR obs->'read_only' IS DISTINCT FROM 'true'::jsonb
  OR obs->'schema_checked_before' IS DISTINCT FROM 'true'::jsonb OR obs->'schema_checked_after' IS DISTINCT FROM 'true'::jsonb
  OR jsonb_typeof(obs->'started_at_ms') IS DISTINCT FROM 'number'
  OR jsonb_typeof(obs->'checked_at_ms') IS DISTINCT FROM 'number'
  OR obs->>'started_at_ms' !~ '^[0-9]+$' OR obs->>'checked_at_ms' !~ '^[0-9]+$'
 THEN RAISE EXCEPTION 'C5 exact empty source observation required' USING ERRCODE='OO030'; END IF;
 started:=(obs->>'started_at_ms')::bigint; checked:=(obs->>'checked_at_ms')::bigint;
 replay:=state.last_confirmation->'c5_observation'->>'observation_id'=state.observation_id::text;
 IF coalesce(replay,false) THEN
  IF state.last_confirmation IS DISTINCT FROM p_nochange
  THEN RAISE EXCEPTION 'C5 empty confirmation replay differs' USING ERRCODE='OO010'; END IF;
 ELSE
  IF started<floor(extract(epoch FROM state.issued_at)*1000)::bigint OR checked<started
   OR to_timestamp(checked::double precision/1000)>clock_timestamp()+interval '5 minutes'
   OR to_timestamp(checked::double precision/1000)<clock_timestamp()-interval '5 minutes'
  THEN RAISE EXCEPTION 'C5 empty observation time differs' USING ERRCODE='OO030'; END IF;
  UPDATE _oo.c5_empty_baseline SET last_confirmation=p_nochange,
   last_empty_confirmed_at=to_timestamp(checked::double precision/1000)
   WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
 END IF;
 RETURN jsonb_build_object('empty_baseline_confirmed',true,'replay',coalesce(replay,false),
  'last_empty_confirmed_at_ms',checked,'data_gen',false,'apply',false,'ack',false,'receipt',NULL);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_observe_empty(text,uuid,bigint,text,text,text,bigint,uuid,bigint,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_observe_empty(text,uuid,bigint,text,text,text,bigint,uuid,bigint,jsonb) TO oo_pipeline_runtime;

ALTER FUNCTION _oo.c5_authority_export(text,text,text) RENAME TO c5_authority_export_before_empty;
REVOKE ALL ON FUNCTION _oo.c5_authority_export_before_empty(text,text,text) FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_authority_export(p_source text,p_schema text,p_table text) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
BEGIN
 IF EXISTS(SELECT FROM _oo.c5_empty_baseline WHERE source=p_source AND schema_name=p_schema AND table_name=p_table)
 THEN RETURN _oo.c5_empty_authority_export(p_source,p_schema,p_table); END IF;
 RETURN _oo.c5_authority_export_before_empty(p_source,p_schema,p_table);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_authority_export(text,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_authority_export(text,text,text) TO oo_pipeline_runtime;
