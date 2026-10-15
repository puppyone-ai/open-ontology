-- CDC-first C1 bootstrap staging. Rows remain private until the caller seals
-- the source barrier and the ordinary publication path accepts the run.
CREATE TABLE _oo.native_c1_baseline_run (
  source text NOT NULL,
  task_id uuid NOT NULL,
  lease_generation bigint NOT NULL,
  source_epoch integer NOT NULL,
  engine text NOT NULL CHECK (engine = 'mysql'),
  measured_id text NOT NULL,
  config_digest text NOT NULL CHECK (config_digest ~ '^[0-9a-f]{64}$'),
  schema_digest text NOT NULL CHECK (schema_digest ~ '^[0-9a-f]{64}$'),
  l0 text NOT NULL,
  l0_pos numeric(38,0) NOT NULL CHECK (l0_pos >= 0),
  applied_pos numeric(38,0) NOT NULL CHECK (applied_pos >= 0),
  l_end numeric(38,0),
  state text NOT NULL CHECK (state IN ('open','awaiting_publish')),
  opened_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  sealed_at timestamptz,
  PRIMARY KEY (source, task_id),
  CHECK ((state = 'open' AND sealed_at IS NULL AND l_end IS NULL)
      OR (state = 'awaiting_publish' AND sealed_at IS NOT NULL AND l_end IS NOT NULL))
);
CREATE TABLE _oo.native_c1_baseline_row (
  source text NOT NULL,
  task_id uuid NOT NULL,
  logical_schema text NOT NULL,
  logical_table text NOT NULL,
  row_key jsonb NOT NULL,
  row_value jsonb,
  deleted boolean NOT NULL,
  applied_pos numeric(38,0) NOT NULL CHECK (applied_pos >= 0),
  txn_id text NOT NULL,
  PRIMARY KEY (source, task_id, logical_schema, logical_table, row_key),
  FOREIGN KEY (source, task_id) REFERENCES _oo.native_c1_baseline_run(source, task_id),
  CHECK ((deleted AND row_value IS NULL) OR (NOT deleted AND jsonb_typeof(row_value) = 'object'))
);
CREATE TABLE _oo.native_c1_baseline_txn (
  source text NOT NULL,
  task_id uuid NOT NULL,
  applied_pos numeric(38,0) NOT NULL,
  txn_id text NOT NULL,
  payload_sha256 text NOT NULL CHECK (payload_sha256 ~ '^[0-9a-f]{64}$'),
  applied_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (source, task_id, applied_pos),
  UNIQUE (source, task_id, txn_id),
  FOREIGN KEY (source, task_id) REFERENCES _oo.native_c1_baseline_run(source, task_id)
);
REVOKE ALL ON _oo.native_c1_baseline_run, _oo.native_c1_baseline_row, _oo.native_c1_baseline_txn FROM PUBLIC, oo_pipeline_runtime;

CREATE FUNCTION _oo.native_c1_baseline_assert_lease(
  p_source text, p_task uuid, p_generation bigint, p_epoch integer
) RETURNS _oo.native_c1_baseline_run
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $c1$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE;
BEGIN
  SELECT * INTO v FROM _oo.native_c1_baseline_run
  WHERE source=p_source AND task_id=p_task FOR UPDATE;
  IF NOT FOUND OR v.lease_generation <> p_generation OR v.source_epoch <> p_epoch
     OR v.state <> 'open'
     OR NOT EXISTS (SELECT 1 FROM jobs.tasks t WHERE t.id=p_task AND t.state='running'
       AND t.generation=p_generation AND t.lease_until>clock_timestamp())
     OR NOT EXISTS (SELECT 1 FROM _oo.source_instance i WHERE i.source=p_source
       AND i.source_epoch=p_epoch AND i.engine='mysql' AND i.measured_id=v.measured_id
       AND i.config_digest=v.config_digest AND i.schema_digest=v.schema_digest
       AND i.lease_id=p_task AND i.lease_generation=p_generation) THEN
    RAISE EXCEPTION 'C1 baseline lease or source epoch is not live' USING ERRCODE='OO010';
  END IF;
  RETURN v;
END
$c1$;

CREATE FUNCTION _oo.native_c1_baseline_open(
  p_source text, p_task uuid, p_generation bigint, p_epoch integer,
  p_measured_id text, p_config_digest text, p_schema_digest text,
  p_l0 text, p_l0_pos numeric
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $c1$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE;
BEGIN
  IF p_source !~ '^[a-z][a-z0-9_]{0,62}$' OR p_task IS NULL OR p_generation < 1
     OR p_epoch < 1 OR p_l0 IS NULL OR p_l0='' OR p_l0_pos < 0
     OR p_config_digest !~ '^[0-9a-f]{64}$' OR p_schema_digest !~ '^[0-9a-f]{64}$' THEN
    RAISE EXCEPTION 'invalid C1 baseline identity' USING ERRCODE='OO030';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM jobs.tasks t WHERE t.id=p_task AND t.state='running'
      AND t.generation=p_generation AND t.lease_until>clock_timestamp())
     OR NOT EXISTS (SELECT 1 FROM _oo.source_instance i WHERE i.source=p_source
       AND i.source_epoch=p_epoch AND i.engine='mysql' AND i.measured_id=p_measured_id
       AND i.config_digest=p_config_digest AND i.schema_digest=p_schema_digest
       AND i.lease_id=p_task AND i.lease_generation=p_generation) THEN
    RAISE EXCEPTION 'C1 baseline requires live lease and registered epoch' USING ERRCODE='OO010';
  END IF;
  INSERT INTO _oo.native_c1_baseline_run(source,task_id,lease_generation,source_epoch,engine,
    measured_id,config_digest,schema_digest,l0,l0_pos,applied_pos,state)
  VALUES(p_source,p_task,p_generation,p_epoch,'mysql',p_measured_id,p_config_digest,
    p_schema_digest,p_l0,p_l0_pos,p_l0_pos,'open')
  ON CONFLICT(source,task_id) DO NOTHING;
  SELECT * INTO v FROM _oo.native_c1_baseline_run WHERE source=p_source AND task_id=p_task FOR UPDATE;
  IF v.lease_generation<>p_generation OR v.source_epoch<>p_epoch OR v.measured_id<>p_measured_id
     OR v.config_digest<>p_config_digest OR v.schema_digest<>p_schema_digest
     OR v.l0<>p_l0 OR v.l0_pos<>p_l0_pos THEN
    RAISE EXCEPTION 'C1 baseline replay identity conflict' USING ERRCODE='OO030';
  END IF;
  RETURN jsonb_build_object('protocol','oo.pipeline.native-c1-baseline-stage/v1',
    'outcome',CASE WHEN v.state='open' THEN 'open' ELSE 'replay' END,
    'source',p_source,'task_id',p_task,'source_epoch',p_epoch,'l0',v.l0,'applied_pos',v.applied_pos);
END
$c1$;

CREATE FUNCTION _oo.native_c1_baseline_apply_txn(
  p_source text,p_task uuid,p_generation bigint,p_epoch integer,
  p_pos numeric,p_txn_id text,p_rows jsonb,p_payload_sha256 text
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $c1$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE; e jsonb; n integer:=0; old _oo.native_c1_baseline_txn%ROWTYPE;
BEGIN
  v:=_oo.native_c1_baseline_assert_lease(p_source,p_task,p_generation,p_epoch);
  IF p_pos<=v.l0_pos OR p_txn_id IS NULL OR p_txn_id='' OR jsonb_typeof(p_rows)<>'array'
     OR p_payload_sha256 !~ '^[0-9a-f]{64}$' THEN RAISE EXCEPTION 'invalid C1 CDC transaction' USING ERRCODE='OO030'; END IF;
  SELECT * INTO old FROM _oo.native_c1_baseline_txn WHERE source=p_source AND task_id=p_task AND applied_pos=p_pos;
  IF FOUND THEN
    IF old.txn_id=p_txn_id AND old.payload_sha256=p_payload_sha256 THEN
      RETURN jsonb_build_object('outcome','replay','applied_pos',p_pos,'txn_id',p_txn_id);
    END IF;
    RAISE EXCEPTION 'C1 CDC position conflict' USING ERRCODE='OO030';
  END IF;
  IF p_pos<=v.applied_pos THEN RAISE EXCEPTION 'C1 CDC position regressed' USING ERRCODE='OO030'; END IF;
  INSERT INTO _oo.native_c1_baseline_txn VALUES(p_source,p_task,p_pos,p_txn_id,p_payload_sha256,clock_timestamp());
  FOR e IN SELECT value FROM jsonb_array_elements(p_rows) LOOP
    IF e->>'schema' IS NULL OR e->>'table' IS NULL OR jsonb_typeof(e->'key') IS NULL
       OR e->>'op' NOT IN ('upsert','delete') OR (e->>'op'='upsert' AND jsonb_typeof(e->'value')<>'object') THEN
      RAISE EXCEPTION 'invalid C1 CDC row' USING ERRCODE='OO030';
    END IF;
    INSERT INTO _oo.native_c1_baseline_row(source,task_id,logical_schema,logical_table,row_key,row_value,deleted,applied_pos,txn_id)
    VALUES(p_source,p_task,e->>'schema',e->>'table',e->'key',CASE WHEN e->>'op'='delete' THEN NULL ELSE e->'value' END,
      e->>'op'='delete',p_pos,p_txn_id)
    ON CONFLICT(source,task_id,logical_schema,logical_table,row_key) DO UPDATE SET
      row_value=EXCLUDED.row_value,deleted=EXCLUDED.deleted,applied_pos=EXCLUDED.applied_pos,txn_id=EXCLUDED.txn_id
      WHERE _oo.native_c1_baseline_row.applied_pos<EXCLUDED.applied_pos;
    n:=n+1;
  END LOOP;
  UPDATE _oo.native_c1_baseline_run SET applied_pos=p_pos WHERE source=p_source AND task_id=p_task;
  RETURN jsonb_build_object('outcome','applied','applied_pos',p_pos,'txn_id',p_txn_id,'events',n);
END
$c1$;

CREATE FUNCTION _oo.native_c1_baseline_fill(
  p_source text,p_task uuid,p_generation bigint,p_epoch integer,p_rows jsonb
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $c1$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE; e jsonb; n integer:=0; skipped integer:=0;
BEGIN
  v:=_oo.native_c1_baseline_assert_lease(p_source,p_task,p_generation,p_epoch);
  IF jsonb_typeof(p_rows)<>'array' THEN RAISE EXCEPTION 'invalid C1 snapshot batch' USING ERRCODE='OO030'; END IF;
  FOR e IN SELECT value FROM jsonb_array_elements(p_rows) LOOP
    IF e->>'schema' IS NULL OR e->>'table' IS NULL OR jsonb_typeof(e->'key') IS NULL
       OR jsonb_typeof(e->'value')<>'object' THEN RAISE EXCEPTION 'invalid C1 snapshot row' USING ERRCODE='OO030'; END IF;
    INSERT INTO _oo.native_c1_baseline_row(source,task_id,logical_schema,logical_table,row_key,row_value,deleted,applied_pos,txn_id)
    VALUES(p_source,p_task,e->>'schema',e->>'table',e->'key',e->'value',false,v.l0_pos,'snapshot')
    ON CONFLICT(source,task_id,logical_schema,logical_table,row_key) DO NOTHING;
    IF FOUND THEN n:=n+1; ELSE skipped:=skipped+1; END IF;
  END LOOP;
  RETURN jsonb_build_object('outcome','filled','inserted',n,'skipped_cdc_or_tombstone',skipped);
END
$c1$;

CREATE FUNCTION _oo.native_c1_baseline_seal(
  p_source text,p_task uuid,p_generation bigint,p_epoch integer,p_l_end text,p_l_end_pos numeric
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $c1$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE;
BEGIN
  v:=_oo.native_c1_baseline_assert_lease(p_source,p_task,p_generation,p_epoch);
  IF p_l_end IS NULL OR p_l_end='' OR p_l_end_pos<v.l0_pos OR p_l_end_pos<>v.applied_pos THEN
    RAISE EXCEPTION 'C1 final barrier is not applied through L_end' USING ERRCODE='OO010';
  END IF;
  UPDATE _oo.native_c1_baseline_run SET l_end=p_l_end_pos,state='awaiting_publish',sealed_at=clock_timestamp()
    WHERE source=p_source AND task_id=p_task;
  RETURN jsonb_build_object('protocol','oo.pipeline.native-c1-baseline-stage/v1','outcome','sealed',
    'source',p_source,'task_id',p_task,'source_epoch',p_epoch,'l0',v.l0,'l_end',p_l_end,
    'applied_pos',v.applied_pos,'rows',(SELECT count(*) FROM _oo.native_c1_baseline_row WHERE source=p_source AND task_id=p_task),
    'deleted',(SELECT count(*) FROM _oo.native_c1_baseline_row WHERE source=p_source AND task_id=p_task AND deleted));
END
$c1$;
REVOKE ALL ON FUNCTION _oo.native_c1_baseline_assert_lease(text,uuid,bigint,integer),
  _oo.native_c1_baseline_open(text,uuid,bigint,integer,text,text,text,text,numeric),
  _oo.native_c1_baseline_apply_txn(text,uuid,bigint,integer,numeric,text,jsonb,text),
  _oo.native_c1_baseline_fill(text,uuid,bigint,integer,jsonb),
  _oo.native_c1_baseline_seal(text,uuid,bigint,integer,text,numeric) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_c1_baseline_open(text,uuid,bigint,integer,text,text,text,text,numeric),
  _oo.native_c1_baseline_apply_txn(text,uuid,bigint,integer,numeric,text,jsonb,text),
  _oo.native_c1_baseline_fill(text,uuid,bigint,integer,jsonb),
  _oo.native_c1_baseline_seal(text,uuid,bigint,integer,text,numeric) TO oo_pipeline_runtime;
