-- Promote the bounded typed staging path through the EXISTING full-import,
-- native-admission and per-table publication contracts. No new publication API.
ALTER TABLE _oo.c1_typed_table ADD COLUMN snapshot_complete boolean NOT NULL DEFAULT false;

CREATE FUNCTION _oo.c1_typed_complete(s text,t uuid,g bigint,e integer,p_name text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  PERFORM _oo.native_c1_baseline_v2_assert(s,t,g,e);
  UPDATE _oo.c1_typed_table SET snapshot_complete=true WHERE source=s AND task=t AND name=p_name;
  IF NOT FOUND THEN RAISE EXCEPTION 'typed_table_missing' USING ERRCODE='OO030'; END IF;
END $fn$;

CREATE FUNCTION _oo.c1_typed_seal(s text,t uuid,g bigint,e integer,p_end text,p_pos numeric,p_tables text[])
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE; r _oo.c1_typed_receipt%ROWTYPE;
  ack_seq bigint; tables text[]; incomplete boolean;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  v:=_oo.native_c1_baseline_v2_assert(s,t,g,e);
  SELECT array_agg(name ORDER BY name),bool_or(NOT snapshot_complete) INTO tables,incomplete
    FROM _oo.c1_typed_table WHERE source=s AND task=t;
  IF tables IS NULL OR tables IS DISTINCT FROM p_tables OR incomplete THEN
    RAISE EXCEPTION 'typed_snapshot_incomplete' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO r FROM _oo.c1_typed_receipt WHERE source=s AND task=t ORDER BY seq DESC LIMIT 1;
  SELECT seq INTO ack_seq FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t;
  IF p_pos IS NULL OR p_pos<>COALESCE(r.position,v.l0_pos)
    OR ack_seq IS DISTINCT FROM COALESCE(r.seq,0)
    OR p_end IS DISTINCT FROM substring(v.measured_id from 7)||':1-'||_oo.native_pos_text(p_pos) THEN
    RAISE EXCEPTION 'typed_barrier' USING ERRCODE='OO030';
  END IF;
  UPDATE _oo.native_c1_baseline_v2_format SET sealed=true WHERE source=s AND task_id=t;
  UPDATE _oo.native_c1_baseline_run SET applied_pos=p_pos,l_end=p_pos,state='awaiting_publish',sealed_at=clock_timestamp()
    WHERE source=s AND task_id=t;
  RETURN jsonb_build_object('outcome','sealed','l_end',p_end,'applied_pos',_oo.native_pos_text(p_pos));
END $fn$;

-- Bounded row courier to the already-existing source admission materializer.
-- Rows remain typed per column; final Loader compares the new shadow directly
-- against the typed stage before retaining any receipt.
CREATE FUNCTION _oo.c1_typed_export(s text,t uuid,g bigint,e integer)
RETURNS SETOF jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE; r _oo.c1_typed_table%ROWTYPE;
  c jsonb; i integer; item text; vals text; keys text; expr text; typ text;
  total bigint:=0; n bigint;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  SELECT * INTO v FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
  IF NOT FOUND OR v.state<>'awaiting_publish' OR v.source_epoch<>e OR v.lease_generation<>g
    OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=t AND generation=g AND state='running' AND lease_until>clock_timestamp()) THEN
    RAISE EXCEPTION 'typed_export_lease' USING ERRCODE='OO010';
  END IF;
  FOR r IN SELECT * FROM _oo.c1_typed_table WHERE source=s AND task=t ORDER BY name LOOP
    EXECUTE format('SELECT count(*) FROM _oo.%I',r.physical) INTO n;
    total:=total+n;
    IF total>100000 THEN RAISE EXCEPTION 'typed_export_budget' USING ERRCODE='OO030'; END IF;
    vals:='';keys:='';i:=0;
    FOR c IN SELECT value FROM jsonb_array_elements(r.spec->'columns') LOOP
      typ:=CASE WHEN (c->>'oid')::bigint=17 THEN 'bytes'
        WHEN (c->>'oid')::bigint IN (20,21,23) THEN 'int'
        WHEN (c->>'oid')::bigint=1700 THEN 'decimal' ELSE 'text' END;
      expr:=CASE WHEN typ='bytes' THEN format('encode(c%s,''hex'')',i) ELSE format('c%s::text',i) END;
      item:=format('jsonb_build_object(''type'',CASE WHEN c%s IS NULL THEN ''null'' ELSE %L END,''value'',%s)',i,typ,expr);
      vals:=vals||CASE WHEN vals='' THEN '' ELSE ',' END||format('%L,%s',c->>'name',item);
      IF (r.spec->'keys') @> to_jsonb(ARRAY[i]) THEN
        keys:=keys||CASE WHEN keys='' THEN '' ELSE ',' END||format('%L,%s',c->>'name',item);
      END IF;
      i:=i+1;
    END LOOP;
    RETURN QUERY EXECUTE format(
      'SELECT jsonb_build_object(''schema'',%L,''table'',%L,''key'',jsonb_build_object(%s),''value'',jsonb_build_object(%s),''deleted'',false,''applied_pos'',_oo.native_pos_text(pos)) FROM _oo.%I ORDER BY k',
      split_part(r.name,'.',1),split_part(r.name,'.',2),keys,vals,r.physical);
  END LOOP;
END $fn$;

ALTER FUNCTION _oo.native_c1_baseline_bind_table(text,uuid,bigint,integer,text,uuid,bigint,uuid,text,text,text[],text[])
RENAME TO native_c1_baseline_bind_table_legacy;

CREATE FUNCTION _oo.native_c1_baseline_bind_table(
 p_source text,p_task uuid,p_lease_generation bigint,p_epoch integer,
 p_shard text,p_owner uuid,p_target_generation bigint,p_run uuid,
 p_schema text,p_table text,p_columns text[],p_keys text[])
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE stage _oo.c1_typed_table%ROWTYPE; baseline _oo.native_c1_baseline_run%ROWTYPE;
  r _oo.full_import_run%ROWTYPE; physical text; col text; mapped text; i integer:=0;
  columns text[]; keys text[]; key_join text:=''; mismatch text:=''; key_columns text:='';
  n bigint; actual bigint; bad boolean;
BEGIN
  SELECT * INTO stage FROM _oo.c1_typed_table WHERE source=p_source AND task=p_task AND name=p_schema||'.'||p_table;
  IF NOT FOUND THEN
    RETURN _oo.native_c1_baseline_bind_table_legacy(p_source,p_task,p_lease_generation,p_epoch,
      p_shard,p_owner,p_target_generation,p_run,p_schema,p_table,p_columns,p_keys);
  END IF;
  SELECT * INTO baseline FROM _oo.native_c1_baseline_run WHERE source=p_source AND task_id=p_task FOR UPDATE;
  IF NOT FOUND OR baseline.state<>'awaiting_publish' OR baseline.l_end<>baseline.applied_pos
    OR baseline.lease_generation<>p_lease_generation OR baseline.source_epoch<>p_epoch
    OR NOT stage.snapshot_complete
    OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=p_task AND generation=p_lease_generation
      AND state='running' AND lease_until>clock_timestamp()) THEN
    RAISE EXCEPTION 'typed_bridge_lease' USING ERRCODE='OO010';
  END IF;
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_target_generation);
  SELECT * INTO r FROM _oo.full_import_run WHERE run_id=p_run;
  IF NOT FOUND OR r.source_id<>p_source OR r.shard_id<>p_shard OR r.owner_token<>p_owner
    OR r.target_generation<>p_target_generation OR r.state<>'open' THEN
    RAISE EXCEPTION 'typed_bridge_run' USING ERRCODE='OO010';
  END IF;
  SELECT array_agg(x->>'name' ORDER BY ord) INTO columns
    FROM jsonb_array_elements(stage.spec->'columns') WITH ORDINALITY AS a(x,ord);
  SELECT array_agg(stage.spec->'columns'->(x::text::integer)->>'name' ORDER BY ord) INTO keys
    FROM jsonb_array_elements(stage.spec->'keys') WITH ORDINALITY AS a(x,ord);
  IF columns IS DISTINCT FROM p_columns OR cardinality(keys)<>cardinality(p_keys) OR NOT keys @> p_keys THEN
    RAISE EXCEPTION 'typed_bridge_schema' USING ERRCODE='OO030';
  END IF;
  SELECT physical_name INTO STRICT physical FROM _oo.full_import_table
    WHERE run_id=p_run AND schema_ident=p_schema AND table_ident=p_table;
  FOREACH col IN ARRAY p_columns LOOP
    mapped:=_oo.lookup_physical_mapping(p_source,r.config_digest,r.source_generation,'column',p_schema,p_table,col);
    mismatch:=mismatch||CASE WHEN mismatch='' THEN '' ELSE ' OR ' END||format('target.%I IS DISTINCT FROM original.c%s',mapped,i);
    IF col=ANY(p_keys) THEN
      key_join:=key_join||CASE WHEN key_join='' THEN '' ELSE ' AND ' END||format('target.%I IS NOT DISTINCT FROM original.c%s',mapped,i);
      key_columns:=key_columns||CASE WHEN key_columns='' THEN '' ELSE ',' END||format('%I',mapped);
    END IF;
    i:=i+1;
  END LOOP;
  EXECUTE format('SELECT count(*) FROM _oo.%I',stage.physical) INTO n;
  EXECUTE format('SELECT count(*) FROM _oo.%I',physical) INTO actual;
  IF n<>actual THEN RAISE EXCEPTION 'typed_bridge_count' USING ERRCODE='OO030'; END IF;
  EXECUTE format('SELECT EXISTS(SELECT 1 FROM _oo.%I original LEFT JOIN _oo.%I target ON %s WHERE target._oo_pos IS NULL OR target._oo_pos<>original.pos OR %s)',stage.physical,physical,key_join,mismatch) INTO bad;
  IF bad THEN RAISE EXCEPTION 'typed_bridge_values' USING ERRCODE='OO030'; END IF;
  SET CONSTRAINTS ALL IMMEDIATE;
  IF NOT EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid=to_regclass(format('_oo.%I',physical)) AND contype='p') THEN
    EXECUTE format('ALTER TABLE _oo.%I ADD PRIMARY KEY (%s)',physical,key_columns);
  END IF;
  RETURN n;
END $fn$;

REVOKE ALL ON FUNCTION
 _oo.c1_typed_complete(text,uuid,bigint,integer,text),
 _oo.c1_typed_seal(text,uuid,bigint,integer,text,numeric,text[]),
 _oo.c1_typed_export(text,uuid,bigint,integer),
 _oo.native_c1_baseline_bind_table(text,uuid,bigint,integer,text,uuid,bigint,uuid,text,text,text[],text[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
 _oo.c1_typed_complete(text,uuid,bigint,integer,text),
 _oo.c1_typed_seal(text,uuid,bigint,integer,text,numeric,text[]),
 _oo.c1_typed_export(text,uuid,bigint,integer),
 _oo.native_c1_baseline_bind_table(text,uuid,bigint,integer,text,uuid,bigint,uuid,text,text,text[],text[]) TO oo_pipeline_runtime;
