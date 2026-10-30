-- Bounded, sealed MySQL typed-state export. Chunks never advance a source cursor.
CREATE FUNCTION _oo.c1_typed_export_meta(s text,t uuid,g bigint,e integer)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE b jsonb; r _oo.c1_typed_table%ROWTYPE; tables jsonb:='[]'; n bigint; tombs bigint; identity jsonb;
BEGIN
  b:=_oo.c1_typed_export_binding(s,t,g,e);
  IF b->>'engine' IS DISTINCT FROM 'mysql' THEN
    RAISE EXCEPTION 'mysql_export_engine' USING ERRCODE='OO030';
  END IF;
  IF NOT EXISTS(SELECT 1 FROM _oo.source_instance WHERE source=s AND source_epoch=e
      AND engine='mysql' AND lease_id=t AND lease_generation=g
      AND config_digest=b->>'config_digest' AND schema_digest=b->>'schema_digest') THEN
    RAISE EXCEPTION 'mysql_export_identity' USING ERRCODE='OO010';
  END IF;
  FOR r IN SELECT * FROM _oo.c1_typed_table WHERE source=s AND task=t ORDER BY name LOOP
    IF NOT r.snapshot_complete THEN
      RAISE EXCEPTION 'mysql_export_incomplete' USING ERRCODE='OO030';
    END IF;
    EXECUTE format('SELECT count(*) FROM _oo.%I',r.physical) INTO n;
    EXECUTE format('SELECT count(*) FROM _oo.%I',r.tomb) INTO tombs;
    tables:=tables||jsonb_build_array(jsonb_build_object('name',r.name,'spec',r.spec,
      'rows',n,'tombstones',tombs));
  END LOOP;
  IF jsonb_array_length(tables) NOT BETWEEN 1 AND 32 THEN
    RAISE EXCEPTION 'mysql_export_tables' USING ERRCODE='OO030';
  END IF;
  SELECT jsonb_build_object('source',source,'engine',engine,'measured_id',measured_id,
    'source_epoch',source_epoch,'config_digest',config_digest,'schema_digest',schema_digest,
    'shard_id',shard_id,'owner_token',owner_token,'lease_id',lease_id) INTO identity
    FROM _oo.source_instance WHERE source=s AND source_epoch=e;
  RETURN b||jsonb_build_object('protocol','oo.mysql.typed-export-binding/v1',
    'source_id',s,'task_id',t,'source_epoch',e,'lease_generation',g,'tables',tables,'source_instance',identity);
END $fn$;

CREATE FUNCTION _oo.c1_typed_export_page(
  s text,t uuid,g bigint,e integer,table_name text,after_key text,max_rows integer,max_bytes integer)
RETURNS SETOF jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE b jsonb; r _oo.c1_typed_table%ROWTYPE; col jsonb; i integer:=0;
  cells text:=''; expression text; kind text; row_value jsonb; used_bytes bigint:=0; row_bytes integer;
BEGIN
  b:=_oo.c1_typed_export_binding(s,t,g,e);
  IF b->>'engine' IS DISTINCT FROM 'mysql' OR max_rows IS NULL OR max_rows NOT BETWEEN 1 AND 2048
    OR max_bytes IS NULL OR max_bytes NOT BETWEEN 1024 AND 4194304
    OR after_key IS NULL OR after_key !~ '^[0-9a-f]*$'
    OR length(after_key)%2<>0 OR length(after_key)>4000 THEN
    RAISE EXCEPTION 'mysql_export_budget_or_cursor' USING ERRCODE='OO030';
  END IF;
  IF NOT EXISTS(SELECT 1 FROM _oo.source_instance WHERE source=s AND source_epoch=e
      AND engine='mysql' AND lease_id=t AND lease_generation=g
      AND config_digest=b->>'config_digest' AND schema_digest=b->>'schema_digest') THEN
    RAISE EXCEPTION 'mysql_export_identity' USING ERRCODE='OO010';
  END IF;
  SELECT * INTO r FROM _oo.c1_typed_table WHERE source=s AND task=t AND name=table_name;
  IF NOT FOUND OR NOT r.snapshot_complete THEN
    RAISE EXCEPTION 'mysql_export_table' USING ERRCODE='OO030';
  END IF;
  FOR col IN SELECT value FROM jsonb_array_elements(r.spec->'columns') LOOP
    kind:=CASE WHEN (col->>'oid')::bigint=17 THEN 'bytes'
      WHEN (col->>'oid')::bigint IN (20,21,23) THEN 'int'
      WHEN (col->>'oid')::bigint=1700 THEN 'decimal' ELSE 'text' END;
    expression:=CASE WHEN kind='bytes' THEN format('encode(c%s,''hex'')',i) ELSE format('c%s::text',i) END;
    cells:=cells||CASE WHEN i=0 THEN '' ELSE ',' END||
      format('jsonb_build_object(''type'',CASE WHEN c%s IS NULL THEN ''null'' ELSE %L END,''value'',%s)',i,kind,expression);
    i:=i+1;
  END LOOP;
  FOR row_value IN EXECUTE format(
    'SELECT jsonb_build_object(''cursor'',encode(k,''hex''),''position'',_oo.native_pos_text(pos),''cells'',jsonb_build_array(%s)) FROM _oo.%I WHERE k>$1 ORDER BY k LIMIT $2',
    cells,r.physical) USING decode(after_key,'hex'),max_rows LOOP
    row_bytes:=octet_length(row_value::text);
    IF row_bytes>max_bytes THEN
      RAISE EXCEPTION 'mysql_export_row_budget' USING ERRCODE='OO030';
    END IF;
    IF used_bytes+row_bytes>max_bytes THEN RETURN; END IF;
    used_bytes:=used_bytes+row_bytes;
    RETURN NEXT row_value;
  END LOOP;
END $fn$;

-- Primary keys are non-null in the typed contract. Equality permits hash/merge
-- joins; the legacy null-safe predicate can otherwise make full validation O(n^2).
CREATE FUNCTION _oo.c1_typed_export_bind_table(
  s text,t uuid,g bigint,e integer,shard text,owner uuid,fence bigint,run uuid,
  source_schema text,source_table text,source_columns text[],source_keys text[])
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE b jsonb; stage _oo.c1_typed_table%ROWTYPE; r _oo.full_import_run%ROWTYPE;
  physical text; names text[]; keys text[]; col text; mapped text; i integer:=0;
  key_join text:=''; mismatch text:=''; key_columns text:=''; n bigint; actual bigint; bad boolean;
BEGIN
  b:=_oo.c1_typed_export_binding(s,t,g,e);
  IF b->>'engine' IS DISTINCT FROM 'mysql' OR NOT EXISTS(
    SELECT 1 FROM _oo.source_instance WHERE source=s AND source_epoch=e
      AND lease_id=t AND lease_generation=g AND engine='mysql'
      AND shard_id=shard AND owner_token=owner AND config_digest=b->>'config_digest'
      AND schema_digest=b->>'schema_digest') THEN
    RAISE EXCEPTION 'mysql_export_identity' USING ERRCODE='OO010';
  END IF;
  PERFORM _oo.full_import_assert_bound(shard,owner,fence);
  SELECT * INTO stage FROM _oo.c1_typed_table WHERE source=s AND task=t
    AND name=source_schema||'.'||source_table;
  IF NOT FOUND OR NOT stage.snapshot_complete THEN
    RAISE EXCEPTION 'mysql_export_table' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO r FROM _oo.full_import_run WHERE run_id=run;
  IF NOT FOUND OR r.source_id<>s OR r.shard_id<>shard OR r.owner_token<>owner
    OR r.target_generation<>fence OR r.state<>'open' OR r.config_digest<>b->>'config_digest' THEN
    RAISE EXCEPTION 'mysql_export_run' USING ERRCODE='OO010';
  END IF;
  SELECT array_agg(x->>'name' ORDER BY ord) INTO names
    FROM jsonb_array_elements(stage.spec->'columns') WITH ORDINALITY AS a(x,ord);
  SELECT array_agg(stage.spec->'columns'->(x::text::integer)->>'name' ORDER BY ord) INTO keys
    FROM jsonb_array_elements(stage.spec->'keys') WITH ORDINALITY AS a(x,ord);
  IF names IS DISTINCT FROM source_columns OR keys IS DISTINCT FROM source_keys
    OR cardinality(keys) IS NULL OR cardinality(keys)<1 OR EXISTS(
      SELECT 1 FROM jsonb_array_elements(stage.spec->'keys') k
      WHERE stage.spec->'columns'->(k::text::integer)->>'nullable' IS DISTINCT FROM 'false') THEN
    RAISE EXCEPTION 'mysql_export_schema' USING ERRCODE='OO030';
  END IF;
  SELECT physical_name INTO STRICT physical FROM _oo.full_import_table
    WHERE run_id=run AND schema_ident=_oo.c1_storage_ident(source_schema)
      AND table_ident=_oo.c1_storage_ident(source_table);
  FOREACH col IN ARRAY source_columns LOOP
    mapped:=_oo.lookup_physical_mapping(s,r.config_digest,r.source_generation,'column',
      _oo.c1_storage_ident(source_schema),_oo.c1_storage_ident(source_table),_oo.c1_storage_ident(col));
    mismatch:=mismatch||CASE WHEN mismatch='' THEN '' ELSE ' OR ' END||
      format('target.%I IS DISTINCT FROM original.c%s',mapped,i);
    IF col=ANY(source_keys) THEN
      key_join:=key_join||CASE WHEN key_join='' THEN '' ELSE ' AND ' END||
        format('target.%I=original.c%s',mapped,i);
      key_columns:=key_columns||CASE WHEN key_columns='' THEN '' ELSE ',' END||format('%I',mapped);
    END IF;
    i:=i+1;
  END LOOP;
  SET CONSTRAINTS ALL IMMEDIATE;
  IF NOT EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid=to_regclass(format('_oo.%I',physical)) AND contype='p') THEN
    EXECUTE format('ALTER TABLE _oo.%I ADD PRIMARY KEY (%s)',physical,key_columns);
  END IF;
  EXECUTE format('ANALYZE _oo.%I',physical);
  EXECUTE format('ANALYZE _oo.%I',stage.physical);
  EXECUTE format('SELECT count(*) FROM _oo.%I',stage.physical) INTO n;
  EXECUTE format('SELECT count(*) FROM _oo.%I',physical) INTO actual;
  IF n<>actual THEN RAISE EXCEPTION 'mysql_export_count' USING ERRCODE='OO030'; END IF;
  EXECUTE format('SELECT EXISTS(SELECT 1 FROM _oo.%I original LEFT JOIN _oo.%I target ON %s WHERE target._oo_pos IS NULL OR target._oo_pos<>original.pos OR %s)',
    stage.physical,physical,key_join,mismatch) INTO bad;
  IF bad THEN RAISE EXCEPTION 'mysql_export_values' USING ERRCODE='OO030'; END IF;
  RETURN n;
END $fn$;

REVOKE ALL ON FUNCTION
  _oo.c1_typed_export_meta(text,uuid,bigint,integer),
  _oo.c1_typed_export_page(text,uuid,bigint,integer,text,text,integer,integer),
  _oo.c1_typed_export_bind_table(text,uuid,bigint,integer,text,uuid,bigint,uuid,text,text,text[],text[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
  _oo.c1_typed_export_meta(text,uuid,bigint,integer),
  _oo.c1_typed_export_page(text,uuid,bigint,integer,text,text,integer,integer),
  _oo.c1_typed_export_bind_table(text,uuid,bigint,integer,text,uuid,bigint,uuid,text,text,text[],text[]) TO oo_pipeline_runtime;
