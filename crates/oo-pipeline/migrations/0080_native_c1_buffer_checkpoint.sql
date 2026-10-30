-- Buffer consumption is distinct from the source connector checkpoint.
-- Intermediate transaction ordinals never manufacture Debezium offsets/SCNs.
CREATE TABLE _oo.c1_typed_capture_bundle(
 source text NOT NULL,task uuid NOT NULL,bundle_sha256 text NOT NULL,
 spool_sha256 text NOT NULL,final_apply_ordinal bigint NOT NULL,
 connector_offsets_sha256 text NOT NULL,connector_history_sha256 text NOT NULL,
 snapshot_scn numeric(38,0) NOT NULL,metadata jsonb NOT NULL,
 PRIMARY KEY(source,task),FOREIGN KEY(source,task) REFERENCES _oo.native_c1_baseline_run(source,task_id)
);
CREATE TABLE _oo.c1_typed_buffer_position(
 source text NOT NULL,task uuid NOT NULL,seq bigint NOT NULL,apply_ordinal bigint NOT NULL,
 transaction_id text NOT NULL,commit_scn numeric(38,0) NOT NULL,evidence jsonb NOT NULL,
 PRIMARY KEY(source,task,seq),UNIQUE(source,task,apply_ordinal),
 FOREIGN KEY(source,task,seq) REFERENCES _oo.c1_typed_receipt(source,task,seq)
);
REVOKE ALL ON _oo.c1_typed_capture_bundle,_oo.c1_typed_buffer_position FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c1_typed_open_capture(s text,t uuid,g bigint,e integer,p jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE old _oo.c1_typed_capture_bundle%ROWTYPE;
BEGIN
  IF p->>'protocol' IS DISTINCT FROM 'oo.c1.oracle-buffer-cursor/v1'
    OR p->>'checkpoint_kind' IS DISTINCT FROM 'buffer_consumption'
    OR p->>'engine' IS DISTINCT FROM 'oracle' OR p->>'source_id' IS DISTINCT FROM s
    OR p->>'bundle_sha256' IS NULL OR p->>'bundle_sha256' !~ '^[0-9a-f]{64}$'
    OR p->>'spool_sha256' IS NULL OR p->>'spool_sha256' !~ '^[0-9a-f]{64}$'
    OR p->>'connector_offsets_sha256' IS NULL OR p->>'connector_offsets_sha256' !~ '^[0-9a-f]{64}$'
    OR p->>'connector_history_sha256' IS NULL OR p->>'connector_history_sha256' !~ '^[0-9a-f]{64}$'
    OR p->>'snapshot_scn' IS NULL OR p->>'snapshot_scn' !~ '^[1-9][0-9]{0,37}$'
    OR p->>'final_apply_ordinal' IS NULL OR p->>'final_apply_ordinal' !~ '^(0|[1-9][0-9]{0,17})$'
    OR p->>'source_offset_advanced' IS DISTINCT FROM 'false'
    OR NOT EXISTS(SELECT 1 FROM _oo.source_instance WHERE source=s AND source_epoch=e
      AND engine='oracle' AND measured_id=p->>'measured_id' AND lease_id=t AND lease_generation=g) THEN
    RAISE EXCEPTION 'closed captured Oracle bundle required' USING ERRCODE='OO030';
  END IF;
  -- l0 pins this immutable BUFFER, not an Oracle log SCN or connector offset.
  PERFORM _oo.c1_typed_open_oracle(s,t,g,e,p->>'bundle_sha256');
  SELECT * INTO old FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t;
  IF FOUND AND old.metadata<>p THEN RAISE EXCEPTION 'capture bundle replay conflict' USING ERRCODE='OO030'; END IF;
  INSERT INTO _oo.c1_typed_capture_bundle VALUES(s,t,p->>'bundle_sha256',p->>'spool_sha256',
    (p->>'final_apply_ordinal')::bigint,p->>'connector_offsets_sha256',p->>'connector_history_sha256',
    (p->>'snapshot_scn')::numeric,p) ON CONFLICT DO NOTHING;
END $fn$;

CREATE FUNCTION _oo.c1_typed_record_buffer(s text,t uuid,g bigint,e integer,p_seq bigint,p_digest text,p jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE b _oo.c1_typed_capture_bundle%ROWTYPE; r _oo.c1_typed_receipt%ROWTYPE;
  old _oo.c1_typed_buffer_position%ROWTYPE; prev _oo.c1_typed_buffer_position%ROWTYPE; checkpoint jsonb;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  PERFORM _oo.c1_typed_assert_engine(s,t,g,e,'oracle');
  SELECT * INTO b FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t;
  IF NOT FOUND OR p->>'protocol' IS DISTINCT FROM 'oo.c1.oracle-buffer-cursor/v1'
    OR p->>'bundle_sha256' IS DISTINCT FROM b.bundle_sha256
    OR p->>'spool_sha256' IS DISTINCT FROM b.spool_sha256
    OR p->>'source_offset_advanced' IS DISTINCT FROM 'false' THEN
    RAISE EXCEPTION 'captured bundle binding conflict' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO r FROM _oo.c1_typed_receipt WHERE source=s AND task=t AND seq=p_seq AND digest=p_digest AND kind='cdc';
  checkpoint:=p->'checkpoint';
  IF NOT FOUND OR r.position::text IS DISTINCT FROM p->>'apply_ordinal'
    OR (checkpoint->>'application_position')::numeric IS DISTINCT FROM r.position
    OR r.position>b.final_apply_ordinal
    OR checkpoint->>'transaction_id' IS NULL
    OR checkpoint->>'commit_scn' IS NULL OR checkpoint->>'commit_scn' !~ '^[1-9][0-9]{0,37}$'
    OR checkpoint->>'event_sha256' IS NULL OR checkpoint->>'event_sha256' !~ '^[0-9a-f]{64}$'
    OR (checkpoint->>'event_count')::bigint NOT BETWEEN 1 AND 100000 THEN
    RAISE EXCEPTION 'captured complete transaction required' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO old FROM _oo.c1_typed_buffer_position WHERE source=s AND task=t AND seq=p_seq;
  IF FOUND THEN
    IF old.evidence<>p THEN RAISE EXCEPTION 'captured transaction replay conflict' USING ERRCODE='OO030'; END IF;
    RETURN;
  END IF;
  SELECT * INTO prev FROM _oo.c1_typed_buffer_position WHERE source=s AND task=t ORDER BY apply_ordinal DESC LIMIT 1;
  IF r.position<>COALESCE(prev.apply_ordinal,0)+1 OR (checkpoint->>'commit_scn')::numeric<COALESCE(prev.commit_scn,b.snapshot_scn) THEN
    RAISE EXCEPTION 'buffer order or source SCN regression' USING ERRCODE='OO030';
  END IF;
  INSERT INTO _oo.c1_typed_buffer_position VALUES(s,t,p_seq,r.position::bigint,checkpoint->>'transaction_id',
    (checkpoint->>'commit_scn')::numeric,p);
END $fn$;

CREATE OR REPLACE FUNCTION _oo.c1_typed_ack(s text,t uuid,g bigint,e integer,p_seq bigint,p_digest text)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);v:=_oo.native_c1_baseline_v2_assert(s,t,g,e);
  IF v.engine='oracle' AND EXISTS(SELECT 1 FROM _oo.c1_typed_receipt WHERE source=s AND task=t AND seq=p_seq AND kind='cdc')
    AND NOT EXISTS(SELECT 1 FROM _oo.c1_typed_source_position WHERE source=s AND task=t AND seq=p_seq)
    AND NOT EXISTS(SELECT 1 FROM _oo.c1_typed_buffer_position WHERE source=s AND task=t AND seq=p_seq) THEN
    RAISE EXCEPTION 'Oracle complete transaction evidence absent' USING ERRCODE='OO030';
  END IF;
  RETURN _oo.c1_typed_ack_legacy(s,t,g,e,p_seq,p_digest);
END $fn$;

CREATE OR REPLACE FUNCTION _oo.c1_typed_checkpoint_evidence(s text,t uuid,g bigint,e integer,p_seq bigint,p_digest text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE result jsonb;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);PERFORM _oo.native_c1_baseline_v2_assert(s,t,g,e);
  IF NOT EXISTS(SELECT 1 FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t AND seq=p_seq)
    OR NOT EXISTS(SELECT 1 FROM _oo.c1_typed_receipt WHERE source=s AND task=t AND seq=p_seq AND digest=p_digest) THEN
    RAISE EXCEPTION 'durable checkpoint required' USING ERRCODE='OO010';
  END IF;
  SELECT evidence||jsonb_build_object('engine','oracle','source_offset_advanced',false)
    INTO result FROM _oo.c1_typed_buffer_position WHERE source=s AND task=t AND seq=p_seq;
  IF FOUND THEN RETURN result; END IF;
  SELECT jsonb_build_object('engine',engine,'checkpoint_kind','opaque_pair',
    'apply_ordinal',apply_ordinal::text,'transaction_id',transaction_id,'commit_scn',commit_scn::text,
    'opaque_before_sha256',opaque_before,'opaque_after_sha256',opaque_after,'source_offset_advanced',false)
    INTO result FROM _oo.c1_typed_source_position WHERE source=s AND task=t AND seq=p_seq;
  RETURN result;
END $fn$;

REVOKE ALL ON FUNCTION
 _oo.c1_typed_open_capture(text,uuid,bigint,integer,jsonb),
 _oo.c1_typed_record_buffer(text,uuid,bigint,integer,bigint,text,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
 _oo.c1_typed_open_capture(text,uuid,bigint,integer,jsonb),
 _oo.c1_typed_record_buffer(text,uuid,bigint,integer,bigint,text,jsonb) TO oo_pipeline_runtime;
