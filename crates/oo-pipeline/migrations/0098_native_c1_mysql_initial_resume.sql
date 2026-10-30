-- Opt-in MySQL initial source fill recovery. Existing snapshots are not resumed
-- as source transactions: their retained fill is reconciled from the original L0.
CREATE TABLE _oo.c1_mysql_initial (
  source text NOT NULL, task uuid NOT NULL, epoch integer NOT NULL,
  metadata jsonb NOT NULL, phase text NOT NULL DEFAULT 'fill'
    CHECK(phase IN ('fill','reconcile','sealed')),
  l_end numeric(38,0), PRIMARY KEY(source,task)
);
CREATE TABLE _oo.c1_mysql_initial_batch (
  source text NOT NULL, task uuid NOT NULL, seq bigint NOT NULL CHECK(seq>0),
  digest text NOT NULL CHECK(digest ~ '^[0-9a-f]{64}$'),
  kind text NOT NULL CHECK(kind IN ('fill','cdc')), position numeric(38,0) NOT NULL,
  chunk jsonb, PRIMARY KEY(source,task,seq),
  FOREIGN KEY(source,task) REFERENCES _oo.c1_mysql_initial(source,task)
);
CREATE TABLE _oo.c1_mysql_initial_eof (
  source text NOT NULL, task uuid NOT NULL, name text NOT NULL, evidence jsonb NOT NULL,
  PRIMARY KEY(source,task,name),
  FOREIGN KEY(source,task) REFERENCES _oo.c1_mysql_initial(source,task)
);
REVOKE ALL ON _oo.c1_mysql_initial,_oo.c1_mysql_initial_batch,_oo.c1_mysql_initial_eof
  FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c1_mysql_initial_assert(s text,t uuid,g bigint,e integer)
RETURNS _oo.c1_mysql_initial LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE m _oo.c1_mysql_initial%ROWTYPE;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  SELECT * INTO m FROM _oo.c1_mysql_initial WHERE source=s AND task=t FOR UPDATE;
  IF NOT FOUND OR m.epoch<>e OR NOT EXISTS(
    SELECT 1 FROM _oo.native_c1_baseline_run r JOIN _oo.source_instance i
      ON i.source=r.source AND i.source_epoch=r.source_epoch
    JOIN jobs.tasks j ON j.id=r.task_id
    WHERE r.source=s AND r.task_id=t AND r.engine='mysql' AND i.engine='mysql'
      AND r.source_epoch=e AND r.lease_generation=g AND i.lease_id=t AND i.lease_generation=g
      AND j.state='running' AND j.generation=g AND j.lease_until>clock_timestamp()
      AND r.l0=m.metadata->>'l0' AND r.config_digest=m.metadata->>'config_digest'
      AND r.schema_digest=m.metadata->>'schema_digest'
      AND i.config_digest=r.config_digest AND i.schema_digest=r.schema_digest
      AND i.measured_id=r.measured_id AND i.measured_id='mysql.'||(m.metadata->>'source_uuid')
  ) THEN RAISE EXCEPTION 'mysql_initial_identity_lease' USING ERRCODE='OO010'; END IF;
  RETURN m;
END $fn$;

CREATE FUNCTION _oo.c1_mysql_initial_open(s text,t uuid,g bigint,e integer,p jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE prior _oo.c1_mysql_initial%ROWTYPE; q record;
BEGIN
  IF p->>'protocol' IS DISTINCT FROM 'oo.mysql.initial-session/v1'
    OR p->>'source_id' IS DISTINCT FROM s OR p->>'task_id' IS DISTINCT FROM t::text
    OR p->>'source_generation' IS DISTINCT FROM '1'
    OR jsonb_typeof(p->'typed_tables') IS DISTINCT FROM 'object'
    OR jsonb_typeof(p->'key_columns') IS DISTINCT FROM 'object'
    OR COALESCE(p->>'connection_config_sha256','') !~ '^[0-9a-f]{64}$'
    OR COALESCE(p->>'catalog_digest','') !~ '^[0-9a-f]{64}$' THEN
    RAISE EXCEPTION 'mysql_initial_header' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO prior FROM _oo.c1_mysql_initial WHERE source=s AND task=t;
  IF FOUND AND (prior.metadata IS DISTINCT FROM p OR prior.epoch<>e) THEN
    RAISE EXCEPTION 'mysql_initial_header_conflict' USING ERRCODE='OO030';
  END IF;
  INSERT INTO _oo.c1_mysql_initial(source,task,epoch,metadata) VALUES(s,t,e,p) ON CONFLICT DO NOTHING;
  PERFORM _oo.c1_mysql_initial_assert(s,t,g,e);
  IF (SELECT count(*) FROM _oo.c1_typed_table WHERE source=s AND task=t)
      IS DISTINCT FROM (SELECT count(*) FROM jsonb_object_keys(p->'typed_tables')) THEN
    RAISE EXCEPTION 'mysql_initial_tables' USING ERRCODE='OO030';
  END IF;
  FOR q IN SELECT * FROM _oo.c1_typed_table WHERE source=s AND task=t LOOP
    IF q.spec IS DISTINCT FROM p->'typed_tables'->q.name
      OR jsonb_typeof(p->'key_columns'->q.name) IS DISTINCT FROM 'array' THEN
      RAISE EXCEPTION 'mysql_initial_table_schema' USING ERRCODE='OO030';
    END IF;
  END LOOP;
END $fn$;

CREATE FUNCTION _oo.c1_mysql_initial_reserve(s text,t uuid,g bigint,e integer,
  n bigint,d text,k text,pos numeric,c jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE m _oo.c1_mysql_initial%ROWTYPE; prior _oo.c1_mysql_initial_batch%ROWTYPE;
  last_chunk jsonb; ack bigint; origin numeric;
BEGIN
  m:=_oo.c1_mysql_initial_assert(s,t,g,e);
  SELECT * INTO prior FROM _oo.c1_mysql_initial_batch WHERE source=s AND task=t AND seq=n;
  IF FOUND THEN
    IF prior.digest IS DISTINCT FROM d OR prior.kind IS DISTINCT FROM k
      OR prior.position IS DISTINCT FROM pos OR prior.chunk IS DISTINCT FROM c THEN
      RAISE EXCEPTION 'mysql_initial_reservation_conflict' USING ERRCODE='OO030';
    END IF;
    RETURN;
  END IF;
  SELECT seq INTO STRICT ack FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t;
  SELECT l0_pos INTO STRICT origin FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
  IF n IS NULL OR k IS NULL OR pos IS NULL OR d IS NULL
    OR n<>ack+1 OR n>1000000 OR k NOT IN ('fill','cdc') OR m.phase='sealed' THEN
    RAISE EXCEPTION 'mysql_initial_sequence' USING ERRCODE='OO030';
  END IF;
  IF k='fill' THEN
    IF m.phase<>'fill' OR pos<>origin OR jsonb_typeof(c) IS DISTINCT FROM 'object'
      OR NOT(m.metadata->'typed_tables' ? (c->>'table'))
      OR c->'key_columns' IS DISTINCT FROM m.metadata->'key_columns'->(c->>'table')
      OR jsonb_typeof(c->'upper') IS DISTINCT FROM 'array'
      OR EXISTS(SELECT 1 FROM _oo.c1_mysql_initial_eof WHERE source=s AND task=t AND name=c->>'table') THEN
      RAISE EXCEPTION 'mysql_initial_chunk' USING ERRCODE='OO030';
    END IF;
    SELECT chunk INTO last_chunk FROM _oo.c1_mysql_initial_batch
      WHERE source=s AND task=t AND kind='fill' AND chunk->>'table'=c->>'table'
      ORDER BY seq DESC LIMIT 1;
    IF c->'lower' IS DISTINCT FROM COALESCE(last_chunk->'upper','null'::jsonb)
      OR c->'upper'=c->'lower' THEN
      RAISE EXCEPTION 'mysql_initial_cursor_chain' USING ERRCODE='OO030';
    END IF;
  ELSE
    IF m.phase<>'reconcile' OR m.l_end IS NULL OR pos<=origin OR pos>m.l_end
      OR c IS NOT NULL AND c<>'null'::jsonb THEN
      RAISE EXCEPTION 'mysql_initial_reconcile_cut' USING ERRCODE='OO030';
    END IF;
  END IF;
  INSERT INTO _oo.c1_mysql_initial_batch VALUES(s,t,n,d,k,pos,c);
END $fn$;

CREATE FUNCTION _oo.c1_mysql_initial_receipt_guard()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
BEGIN
  IF EXISTS(SELECT 1 FROM _oo.c1_mysql_initial WHERE source=NEW.source AND task=NEW.task)
    AND NOT EXISTS(SELECT 1 FROM _oo.c1_mysql_initial_batch
      WHERE source=NEW.source AND task=NEW.task AND seq=NEW.seq AND digest=NEW.digest
        AND kind=NEW.kind AND position=NEW.position) THEN
    RAISE EXCEPTION 'mysql_initial_unbound_receipt' USING ERRCODE='OO030';
  END IF;
  RETURN NEW;
END $fn$;
CREATE TRIGGER c1_mysql_initial_receipt BEFORE INSERT ON _oo.c1_typed_receipt
  FOR EACH ROW EXECUTE FUNCTION _oo.c1_mysql_initial_receipt_guard();

CREATE FUNCTION _oo.c1_mysql_initial_complete(s text,t uuid,g bigint,e integer,p jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE m _oo.c1_mysql_initial%ROWTYPE; b _oo.c1_mysql_initial_batch%ROWTYPE;
  ack bigint; old jsonb; expected jsonb;
BEGIN
  m:=_oo.c1_mysql_initial_assert(s,t,g,e);
  IF m.phase<>'fill' OR p->>'table' IS NULL OR NOT(m.metadata->'typed_tables' ? (p->>'table')) THEN
    RAISE EXCEPTION 'mysql_initial_eof' USING ERRCODE='OO030';
  END IF;
  SELECT seq INTO STRICT ack FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t;
  SELECT * INTO b FROM _oo.c1_mysql_initial_batch WHERE source=s AND task=t AND kind='fill'
    AND chunk->>'table'=p->>'table' ORDER BY seq DESC LIMIT 1;
  expected:=jsonb_build_object('table',p->>'table','last_fill_sequence',COALESCE(b.seq,0),
    'upper',COALESCE(b.chunk->'upper','null'::jsonb));
  IF p IS DISTINCT FROM expected OR COALESCE(b.seq,0)>ack THEN
    RAISE EXCEPTION 'mysql_initial_eof_checkpoint' USING ERRCODE='OO030';
  END IF;
  SELECT evidence INTO old FROM _oo.c1_mysql_initial_eof WHERE source=s AND task=t AND name=p->>'table';
  IF old IS NOT NULL AND old IS DISTINCT FROM p THEN
    RAISE EXCEPTION 'mysql_initial_eof_conflict' USING ERRCODE='OO030';
  END IF;
  INSERT INTO _oo.c1_mysql_initial_eof VALUES(s,t,p->>'table',p) ON CONFLICT DO NOTHING;
  PERFORM _oo.c1_typed_complete(s,t,g,e,p->>'table');
END $fn$;

CREATE FUNCTION _oo.c1_mysql_initial_complete_guard()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
BEGIN
  IF NEW.snapshot_complete AND EXISTS(SELECT 1 FROM _oo.c1_mysql_initial WHERE source=NEW.source AND task=NEW.task)
    AND NOT EXISTS(SELECT 1 FROM _oo.c1_mysql_initial_eof WHERE source=NEW.source AND task=NEW.task AND name=NEW.name) THEN
    RAISE EXCEPTION 'mysql_initial_unbound_eof' USING ERRCODE='OO030';
  END IF;
  RETURN NEW;
END $fn$;
CREATE TRIGGER c1_mysql_initial_complete BEFORE UPDATE OF snapshot_complete ON _oo.c1_typed_table
  FOR EACH ROW EXECUTE FUNCTION _oo.c1_mysql_initial_complete_guard();

CREATE FUNCTION _oo.c1_mysql_initial_end(s text,t uuid,g bigint,e integer,p numeric)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE m _oo.c1_mysql_initial%ROWTYPE; ack bigint; maximum bigint; origin numeric;
BEGIN
  m:=_oo.c1_mysql_initial_assert(s,t,g,e);
  SELECT seq INTO STRICT ack FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t;
  SELECT COALESCE(max(seq),0) INTO maximum FROM _oo.c1_typed_receipt WHERE source=s AND task=t;
  SELECT l0_pos INTO STRICT origin FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
  IF p IS NULL OR p<origin OR ack<>maximum OR EXISTS(SELECT 1 FROM _oo.c1_typed_table
      WHERE source=s AND task=t AND NOT snapshot_complete)
    OR EXISTS(SELECT 1 FROM _oo.c1_mysql_initial_batch WHERE source=s AND task=t AND seq>ack)
    OR (SELECT count(*) FROM _oo.c1_mysql_initial_eof WHERE source=s AND task=t)
       IS DISTINCT FROM (SELECT count(*) FROM jsonb_object_keys(m.metadata->'typed_tables'))
    OR m.l_end IS NOT NULL AND m.l_end<>p THEN
    RAISE EXCEPTION 'mysql_initial_end_barrier' USING ERRCODE='OO030';
  END IF;
  UPDATE _oo.c1_mysql_initial SET l_end=p,phase=CASE WHEN phase='sealed' THEN phase ELSE 'reconcile' END
    WHERE source=s AND task=t;
END $fn$;

CREATE FUNCTION _oo.c1_mysql_initial_seal(s text,t uuid,g bigint,e integer,p_end text,p numeric,names text[])
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE m _oo.c1_mysql_initial%ROWTYPE; result jsonb; expected text[];
BEGIN
  m:=_oo.c1_mysql_initial_assert(s,t,g,e);
  SELECT array_agg(key ORDER BY key) INTO expected FROM jsonb_object_keys(m.metadata->'typed_tables') key;
  IF m.phase NOT IN ('reconcile','sealed') OR m.l_end IS DISTINCT FROM p
    OR p_end IS DISTINCT FROM (m.metadata->>'source_uuid')||':1-'||_oo.native_pos_text(p)
    OR names IS DISTINCT FROM expected THEN
    RAISE EXCEPTION 'mysql_initial_seal_cut' USING ERRCODE='OO030';
  END IF;
  PERFORM _oo.c1_mysql_initial_end(s,t,g,e,p);
  IF m.phase='sealed' THEN
    IF NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t
      AND state='awaiting_publish' AND l_end=p AND applied_pos=p) THEN
      RAISE EXCEPTION 'mysql_initial_sealed_drift' USING ERRCODE='OO030';
    END IF;
    RETURN jsonb_build_object('outcome','sealed','l_end',p_end,'applied_pos',_oo.native_pos_text(p));
  END IF;
  result:=_oo.c1_typed_seal(s,t,g,e,p_end,p,names);
  UPDATE _oo.c1_mysql_initial SET phase='sealed' WHERE source=s AND task=t;
  RETURN result;
END $fn$;

CREATE FUNCTION _oo.c1_mysql_initial_state(s text,t uuid,g bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE m _oo.c1_mysql_initial%ROWTYPE; ack bigint; maximum bigint; cnt bigint;
  r _oo.c1_typed_receipt%ROWTYPE; cursors jsonb:='{}'; completed jsonb:='[]';
  item record; b _oo.c1_mysql_initial_batch%ROWTYPE; origin numeric;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  SELECT * INTO m FROM _oo.c1_mysql_initial WHERE source=s AND task=t;
  IF NOT FOUND THEN
    IF EXISTS(SELECT 1 FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t) THEN
      RAISE EXCEPTION 'mysql_initial_metadata_missing' USING ERRCODE='OO030';
    END IF;
    RETURN NULL;
  END IF;
  PERFORM _oo.native_c1_baseline_rebind(s,t,g);
  m:=_oo.c1_mysql_initial_assert(s,t,g,m.epoch);
  SELECT seq INTO STRICT ack FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t;
  SELECT COALESCE(max(seq),0),count(*) INTO maximum,cnt FROM _oo.c1_typed_receipt WHERE source=s AND task=t;
  IF maximum NOT BETWEEN ack AND ack+1 OR cnt<>maximum OR EXISTS(
    SELECT 1 FROM _oo.c1_typed_receipt rr LEFT JOIN _oo.c1_mysql_initial_batch bb
      ON bb.source=rr.source AND bb.task=rr.task AND bb.seq=rr.seq
    WHERE rr.source=s AND rr.task=t AND (bb.digest IS DISTINCT FROM rr.digest
      OR bb.kind IS DISTINCT FROM rr.kind OR bb.position IS DISTINCT FROM rr.position)) THEN
    RAISE EXCEPTION 'mysql_initial_receipt_prefix' USING ERRCODE='OO030';
  END IF;
  IF (SELECT count(*) FROM _oo.c1_typed_table WHERE source=s AND task=t)
      IS DISTINCT FROM (SELECT count(*) FROM jsonb_object_keys(m.metadata->'typed_tables'))
    OR (SELECT count(*) FROM _oo.c1_mysql_initial_batch WHERE source=s AND task=t)
      NOT BETWEEN maximum AND ack+1
    OR EXISTS(SELECT 1 FROM _oo.c1_mysql_initial_batch WHERE source=s AND task=t AND seq>ack+1)
    OR (m.phase='fill' AND m.l_end IS NOT NULL)
    OR (m.phase<>'fill' AND (m.l_end IS NULL OR EXISTS(
      SELECT 1 FROM _oo.c1_typed_table WHERE source=s AND task=t AND NOT snapshot_complete))) THEN
    RAISE EXCEPTION 'mysql_initial_state_drift' USING ERRCODE='OO030';
  END IF;
  SELECT l0_pos INTO STRICT origin FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
  SELECT * INTO r FROM _oo.c1_typed_receipt WHERE source=s AND task=t AND seq=ack;
  FOR item IN SELECT * FROM _oo.c1_typed_table WHERE source=s AND task=t ORDER BY name LOOP
    SELECT * INTO b FROM _oo.c1_mysql_initial_batch WHERE source=s AND task=t AND seq<=ack AND kind='fill'
      AND chunk->>'table'=item.name ORDER BY seq DESC LIMIT 1;
    IF item.spec IS DISTINCT FROM m.metadata->'typed_tables'->item.name THEN
      RAISE EXCEPTION 'mysql_initial_table_drift' USING ERRCODE='OO030';
    END IF;
    IF item.snapshot_complete THEN
      IF NOT EXISTS(SELECT 1 FROM _oo.c1_mysql_initial_eof WHERE source=s AND task=t AND name=item.name
        AND evidence=jsonb_build_object('table',item.name,'last_fill_sequence',COALESCE(b.seq,0),
          'upper',COALESCE(b.chunk->'upper','null'::jsonb))) THEN
        RAISE EXCEPTION 'mysql_initial_complete_drift' USING ERRCODE='OO030';
      END IF;
      completed:=completed||jsonb_build_array(item.name);
    END IF;
    cursors:=cursors||jsonb_build_object(item.name,jsonb_build_object('last_fill_sequence',COALESCE(b.seq,0),
      'upper',COALESCE(b.chunk->'upper','null'::jsonb),'ended',item.snapshot_complete));
  END LOOP;
  RETURN jsonb_build_object('protocol','oo.mysql.initial-resume/v1','source_id',s,'task_id',t,
    'source_epoch',m.epoch,'l0',m.metadata->>'l0','metadata',m.metadata,'phase',m.phase,
    'l_end',CASE WHEN m.l_end IS NULL THEN NULL ELSE _oo.native_pos_text(m.l_end) END,
    'sequence',ack,'max_sequence',maximum,'position',_oo.native_pos_text(COALESCE(r.position,origin)),
    'digest',r.digest,'completed_tables',completed,'table_cursors',cursors);
END $fn$;

REVOKE ALL ON FUNCTION
  _oo.c1_mysql_initial_assert(text,uuid,bigint,integer),
  _oo.c1_mysql_initial_open(text,uuid,bigint,integer,jsonb),
  _oo.c1_mysql_initial_reserve(text,uuid,bigint,integer,bigint,text,text,numeric,jsonb),
  _oo.c1_mysql_initial_complete(text,uuid,bigint,integer,jsonb),
  _oo.c1_mysql_initial_end(text,uuid,bigint,integer,numeric),
  _oo.c1_mysql_initial_seal(text,uuid,bigint,integer,text,numeric,text[]),
  _oo.c1_mysql_initial_state(text,uuid,bigint),
  _oo.c1_mysql_initial_receipt_guard(),_oo.c1_mysql_initial_complete_guard() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
  _oo.c1_mysql_initial_open(text,uuid,bigint,integer,jsonb),
  _oo.c1_mysql_initial_reserve(text,uuid,bigint,integer,bigint,text,text,numeric,jsonb),
  _oo.c1_mysql_initial_complete(text,uuid,bigint,integer,jsonb),
  _oo.c1_mysql_initial_end(text,uuid,bigint,integer,numeric),
  _oo.c1_mysql_initial_seal(text,uuid,bigint,integer,text,numeric,text[]),
  _oo.c1_mysql_initial_state(text,uuid,bigint) TO oo_pipeline_runtime;
