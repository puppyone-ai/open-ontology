-- Additive private C1 stage. No publication and no changes to v1/v2 receipts.
CREATE TABLE _oo.c1_typed_table (
  source text NOT NULL, task uuid NOT NULL, name text NOT NULL,
  spec jsonb NOT NULL, physical text NOT NULL, stage text NOT NULL, tomb text NOT NULL,
  key_expr text NOT NULL, columns_sql text NOT NULL, update_sql text NOT NULL,
  PRIMARY KEY(source,task,name)
);
CREATE TABLE _oo.c1_typed_receipt (
  source text NOT NULL, task uuid NOT NULL, seq bigint NOT NULL CHECK(seq>0),
  digest text NOT NULL CHECK(digest ~ '^[0-9a-f]{64}$'),
  position numeric(38,0) NOT NULL, kind text NOT NULL CHECK(kind IN ('fill','cdc')),
  PRIMARY KEY(source,task,seq)
);
CREATE TABLE _oo.c1_typed_checkpoint (
  source text NOT NULL, task uuid NOT NULL, seq bigint NOT NULL DEFAULT 0,
  PRIMARY KEY(source,task)
);
REVOKE ALL ON _oo.c1_typed_table, _oo.c1_typed_receipt, _oo.c1_typed_checkpoint FROM PUBLIC;

CREATE FUNCTION _oo.c1_typed_open(s text,t uuid,g bigint,e integer,p_name text,p_spec jsonb)
RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE
  prior _oo.c1_typed_table%ROWTYPE;
  col jsonb; k jsonb; idx integer; n integer; oid bigint; tm integer;
  defs text:=''; target_defs text:=''; names text:=''; updates text:=''; typ text; field text;
  key_sql text; payload text; tag text; seen text[]:='{}';
  phys text:='c3p_'||md5(s||':'||t::text||':'||p_name);
  stage_name text:='c3s_'||md5(s||':'||t::text||':'||p_name);
  tomb_name text:='c3t_'||md5(s||':'||t::text||':'||p_name);
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  PERFORM _oo.native_c1_baseline_v2_assert(s,t,g,e);
  IF p_name IS NULL OR p_name='' OR jsonb_typeof(p_spec->'columns') IS DISTINCT FROM 'array'
     OR jsonb_typeof(p_spec->'keys') IS DISTINCT FROM 'array'
     OR jsonb_array_length(p_spec->'columns') NOT BETWEEN 1 AND 1024
     OR jsonb_array_length(p_spec->'keys') NOT BETWEEN 1 AND 32 THEN
    RAISE EXCEPTION 'typed_schema' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO prior FROM _oo.c1_typed_table WHERE source=s AND task=t AND name=p_name;
  IF FOUND THEN
    IF prior.spec IS DISTINCT FROM p_spec THEN RAISE EXCEPTION 'typed_schema_conflict' USING ERRCODE='OO030'; END IF;
    RETURN prior.stage;
  END IF;
  n:=0;
  FOR col IN SELECT value FROM jsonb_array_elements(p_spec->'columns') LOOP
    IF col->>'name' IS NULL OR col->>'name'='' OR col->>'name'=ANY(seen) THEN
      RAISE EXCEPTION 'typed_schema' USING ERRCODE='OO030';
    END IF;
    seen:=array_append(seen,col->>'name');
    oid:=(col->>'oid')::bigint; tm:=(col->>'typmod')::integer;
    typ:=_oo.full_import_type_sql(oid,tm);
    field:='c'||n;
    defs:=defs||format(', %I %s',field,typ);
    target_defs:=target_defs||format(', %I %s%s',field,typ,
      CASE WHEN (col->>'nullable')::boolean=false THEN ' NOT NULL' ELSE '' END);
    names:=names||CASE WHEN n=0 THEN '' ELSE ',' END||format('%I',field);
    updates:=updates||CASE WHEN n=0 THEN '' ELSE ',' END||format('%1$I=EXCLUDED.%1$I',field);
    n:=n+1;
  END LOOP;
  key_sql:=format('decode(''6f6f2e63312e6b65792f763300'',''hex'') || int4send(%s)',jsonb_array_length(p_spec->'keys'));
  seen:='{}';
  FOR k IN SELECT value FROM jsonb_array_elements(p_spec->'keys') LOOP
    idx:=(k#>>'{}')::integer;
    IF idx<0 OR idx>=n OR idx::text=ANY(seen) THEN RAISE EXCEPTION 'typed_key_shape' USING ERRCODE='OO030'; END IF;
    seen:=array_append(seen,idx::text);
    col:=p_spec->'columns'->idx; oid:=(col->>'oid')::bigint; tm:=(col->>'typmod')::integer;
    field:='c'||idx;
    IF (col->>'nullable')::boolean IS DISTINCT FROM false THEN RAISE EXCEPTION 'typed_key_nullable' USING ERRCODE='OO030'; END IF;
    IF oid IN (20,21,23) OR (oid=1700 AND tm>=4 AND ((tm-4)&65535)=0) THEN
      tag:='01'; payload:=format('convert_to(%I::text,''UTF8'')',field);
    ELSIF oid IN (25,1043) THEN
      tag:='02'; payload:=format('convert_to(%I::text,''UTF8'')',field);
    ELSIF oid=17 THEN
      tag:='03'; payload:=format('%I',field);
    ELSE
      -- CHAR padding has engine-specific identity semantics; do not trim it silently.
      RAISE EXCEPTION 'typed_key_type' USING ERRCODE='OO030';
    END IF;
    key_sql:=key_sql||format(' || decode(%L,''hex'') || int4send(octet_length(%s)) || %s',tag,payload,payload);
  END LOOP;
  EXECUTE format('CREATE TABLE _oo.%I (k bytea PRIMARY KEY, pos numeric(38,0) NOT NULL%s)',phys,target_defs);
  EXECUTE format('CREATE TABLE _oo.%I (k bytea PRIMARY KEY, pos numeric(38,0) NOT NULL)',tomb_name);
  EXECUTE format('CREATE UNLOGGED TABLE _oo.%I (k bytea NOT NULL, op text NOT NULL%s)',stage_name,defs);
  EXECUTE format('GRANT INSERT ON _oo.%I TO oo_pipeline_runtime',stage_name);
  INSERT INTO _oo.c1_typed_table VALUES(s,t,p_name,p_spec,phys,stage_name,tomb_name,key_sql,names,updates);
  INSERT INTO _oo.c1_typed_checkpoint(source,task) VALUES(s,t) ON CONFLICT DO NOTHING;
  RETURN stage_name;
END $fn$;

-- Called before COPY and again before commit. Replays must match the whole
-- transaction digest. No later transaction can pass an unacknowledged receipt.
CREATE FUNCTION _oo.c1_typed_begin(s text,t uuid,g bigint,e integer,
  p_seq bigint,p_digest text,p_prev numeric,p_pos numeric,p_kind text)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE; r _oo.c1_typed_receipt%ROWTYPE;
  last_seq bigint; last_pos numeric; ack_seq bigint;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  v:=_oo.native_c1_baseline_v2_assert(s,t,g,e);
  IF p_seq IS NULL OR p_seq<1 OR p_digest IS NULL OR p_digest !~ '^[0-9a-f]{64}$'
     OR p_kind IS NULL OR p_kind NOT IN ('fill','cdc') OR p_prev IS NULL OR p_pos IS NULL
     OR p_pos<0 OR (p_kind='fill' AND p_pos<>p_prev) OR (p_kind='cdc' AND p_pos<>p_prev+1) THEN
    RAISE EXCEPTION 'typed_batch' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO r FROM _oo.c1_typed_receipt WHERE source=s AND task=t AND seq=p_seq;
  IF FOUND THEN
    IF r.digest<>p_digest OR r.position<>p_pos OR r.kind<>p_kind THEN RAISE EXCEPTION 'typed_replay_conflict' USING ERRCODE='OO030'; END IF;
    RETURN false;
  END IF;
  SELECT seq,position INTO last_seq,last_pos FROM _oo.c1_typed_receipt WHERE source=s AND task=t ORDER BY seq DESC LIMIT 1;
  last_seq:=COALESCE(last_seq,0); last_pos:=COALESCE(last_pos,v.l0_pos);
  SELECT seq INTO ack_seq FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t;
  IF p_seq<>last_seq+1 OR p_prev<>last_pos OR ack_seq IS DISTINCT FROM last_seq THEN
    RAISE EXCEPTION 'typed_sequence_or_unacked' USING ERRCODE='OO030';
  END IF;
  RETURN true;
END $fn$;

CREATE FUNCTION _oo.c1_typed_apply_table(s text,t uuid,g bigint,e integer,
  p_name text,p_kind text,p_pos numeric,p_rows bigint)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE r _oo.c1_typed_table%ROWTYPE; bad boolean; cnt bigint; distinct_cnt bigint;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  PERFORM _oo.native_c1_baseline_v2_assert(s,t,g,e);
  SELECT * INTO STRICT r FROM _oo.c1_typed_table WHERE source=s AND task=t AND name=p_name;
  EXECUTE format('SELECT count(*),count(DISTINCT k),COALESCE(bool_or(k IS DISTINCT FROM (%s) OR octet_length(k)>2000 OR op NOT IN (''upsert'',''delete'')),false) FROM _oo.%I',
    r.key_expr,r.stage) INTO cnt,distinct_cnt,bad;
  IF cnt<>p_rows OR distinct_cnt<>cnt OR bad OR p_kind NOT IN ('fill','cdc') OR p_pos<0 THEN
    RAISE EXCEPTION 'typed_stage' USING ERRCODE='OO030';
  END IF;
  IF p_kind='fill' THEN
    EXECUTE format('SELECT EXISTS(SELECT 1 FROM _oo.%I WHERE op<>''upsert'')',r.stage) INTO bad;
    IF bad THEN RAISE EXCEPTION 'typed_fill_delete' USING ERRCODE='OO030'; END IF;
    EXECUTE format('INSERT INTO _oo.%I(k,pos,%s) SELECT k,0,%s FROM _oo.%I x WHERE NOT EXISTS(SELECT 1 FROM _oo.%I d WHERE d.k=x.k) ON CONFLICT(k) DO NOTHING',
      r.physical,r.columns_sql,r.columns_sql,r.stage,r.tomb);
  ELSE
    EXECUTE format('INSERT INTO _oo.%I AS d(k,pos) SELECT k,$1 FROM _oo.%I WHERE op=''delete'' ON CONFLICT(k) DO UPDATE SET pos=greatest(d.pos,EXCLUDED.pos)',
      r.tomb,r.stage) USING p_pos;
    EXECUTE format('DELETE FROM _oo.%I x USING _oo.%I d WHERE x.k=d.k AND d.op=''delete'' AND x.pos<=$1',r.physical,r.stage) USING p_pos;
    EXECUTE format('INSERT INTO _oo.%I AS x(k,pos,%s) SELECT k,$1,%s FROM _oo.%I st WHERE op=''upsert'' AND NOT EXISTS(SELECT 1 FROM _oo.%I d WHERE d.k=st.k AND d.pos>=$1) ON CONFLICT(k) DO UPDATE SET pos=EXCLUDED.pos,%s WHERE x.pos<EXCLUDED.pos',
      r.physical,r.columns_sql,r.columns_sql,r.stage,r.tomb,r.update_sql) USING p_pos;
  END IF;
  EXECUTE format('TRUNCATE _oo.%I',r.stage);
END $fn$;

CREATE FUNCTION _oo.c1_typed_finish(s text,t uuid,g bigint,e integer,
  p_seq bigint,p_digest text,p_prev numeric,p_pos numeric,p_kind text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
BEGIN
  IF NOT _oo.c1_typed_begin(s,t,g,e,p_seq,p_digest,p_prev,p_pos,p_kind) THEN
    RAISE EXCEPTION 'typed_finish_replay' USING ERRCODE='OO030';
  END IF;
  INSERT INTO _oo.c1_typed_receipt VALUES(s,t,p_seq,p_digest,p_pos,p_kind);
END $fn$;

-- A separate durable CAS after the target transaction; a lost ACK can replay
-- this CAS, but cannot change its digest. This is not publication/job success.
CREATE FUNCTION _oo.c1_typed_ack(s text,t uuid,g bigint,e integer,p_seq bigint,p_digest text)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE current_seq bigint;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  PERFORM _oo.native_c1_baseline_v2_assert(s,t,g,e);
  IF NOT EXISTS(SELECT 1 FROM _oo.c1_typed_receipt WHERE source=s AND task=t AND seq=p_seq AND digest=p_digest) THEN RETURN false; END IF;
  SELECT seq INTO current_seq FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t FOR UPDATE;
  IF current_seq=p_seq THEN RETURN true; END IF;
  IF current_seq IS DISTINCT FROM p_seq-1 THEN RETURN false; END IF;
  UPDATE _oo.c1_typed_checkpoint SET seq=p_seq WHERE source=s AND task=t AND seq=p_seq-1;
  RETURN FOUND;
END $fn$;

REVOKE ALL ON FUNCTION
  _oo.c1_typed_open(text,uuid,bigint,integer,text,jsonb),
  _oo.c1_typed_begin(text,uuid,bigint,integer,bigint,text,numeric,numeric,text),
  _oo.c1_typed_apply_table(text,uuid,bigint,integer,text,text,numeric,bigint),
  _oo.c1_typed_finish(text,uuid,bigint,integer,bigint,text,numeric,numeric,text),
  _oo.c1_typed_ack(text,uuid,bigint,integer,bigint,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
  _oo.c1_typed_open(text,uuid,bigint,integer,text,jsonb),
  _oo.c1_typed_begin(text,uuid,bigint,integer,bigint,text,numeric,numeric,text),
  _oo.c1_typed_apply_table(text,uuid,bigint,integer,text,text,numeric,bigint),
  _oo.c1_typed_finish(text,uuid,bigint,integer,bigint,text,numeric,numeric,text),
  _oo.c1_typed_ack(text,uuid,bigint,integer,bigint,text) TO oo_pipeline_runtime;
