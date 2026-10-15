-- Preserve the existing nullable MySQL zero-date quality mapping during typed stage verification.
CREATE OR REPLACE FUNCTION _oo.native_c1_baseline_bind_table(
 p_source text,p_task uuid,p_lease_generation bigint,p_epoch integer,
 p_shard text,p_owner uuid,p_target_generation bigint,p_run uuid,
 p_schema text,p_table text,p_columns text[],p_keys text[]
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $bridge$
DECLARE s _oo.native_c1_baseline_run%ROWTYPE; r _oo.full_import_run%ROWTYPE;
 physical text; col text; mapped text; typ text; oid oid; key_join text:='';
 mismatch text:=''; zero_date text; quality text; key_columns text:=''; expected text; n bigint; actual bigint; bad boolean;
BEGIN
 SELECT * INTO s FROM _oo.native_c1_baseline_run WHERE source=p_source AND task_id=p_task FOR UPDATE;
 IF NOT FOUND OR s.state<>'awaiting_publish' OR s.l_end<>s.applied_pos
  OR s.lease_generation<>p_lease_generation OR s.source_epoch<>p_epoch
  OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=p_task AND generation=p_lease_generation
    AND state='running' AND lease_until>clock_timestamp())
 THEN RAISE EXCEPTION 'sealed baseline and live lease required' USING ERRCODE='OO010'; END IF;
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_target_generation);
 SELECT * INTO r FROM _oo.full_import_run WHERE run_id=p_run;
 IF NOT FOUND OR r.source_id<>p_source OR r.shard_id<>p_shard OR r.owner_token<>p_owner
  OR r.target_generation<>p_target_generation OR r.state<>'open'
  OR cardinality(p_keys)<1 OR cardinality(p_columns)<1
 THEN RAISE EXCEPTION 'typed baseline run identity' USING ERRCODE='OO010'; END IF;
 SELECT physical_name INTO STRICT physical FROM _oo.full_import_table
  WHERE run_id=p_run AND schema_ident=p_schema AND table_ident=p_table;
 FOREACH col IN ARRAY p_columns LOOP
  mapped:=_oo.lookup_physical_mapping(p_source,r.config_digest,r.source_generation,'column',p_schema,p_table,col);
  SELECT a.atttypid,pg_catalog.format_type(a.atttypid,a.atttypmod) INTO oid,typ
   FROM pg_catalog.pg_attribute a WHERE a.attrelid=to_regclass(format('_oo.%I',physical)) AND a.attname=mapped AND NOT a.attisdropped;
  IF oid IS NULL OR oid NOT IN (16,17,20,21,23,25,114,700,701,1042,1043,1082,1114,1184,1186,1700,3802)
  THEN RAISE EXCEPTION 'baseline column type' USING ERRCODE='OO030'; END IF;
  expected:=CASE WHEN oid=17 THEN format('decode(s.row_value->%L->>''value'',''hex'')',col)
   WHEN oid IN (114,3802) THEN format('(s.row_value->%L->>''value'')::jsonb',col)
   ELSE format('(s.row_value->%L->>''value'')::%s',col,typ) END;
  IF oid IN (1082,1114,1184) THEN
   zero_date:=format('(s.row_value->%L->>''value'') ~ %L',col,
     '^(0000-[0-9]{2}-[0-9]{2}|[0-9]{4}-00-[0-9]{2}|[0-9]{4}-[0-9]{2}-00)' || CASE WHEN oid=1082 THEN '([T ]00:00:00(\.[0-9]{1,6})?Z?)?$' ELSE '[T ][0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]{1,6})?Z?$' END);
   quality:=format('EXISTS(SELECT 1 FROM _oo.full_import_quality q WHERE q.run_id=$5 AND q.kind=''reject'' AND q.reason=''zero_date'' AND q.attnum=%s AND q.key_digest IS NOT NULL AND q.row_index=(SELECT count(*) FROM _oo.native_c1_baseline_row prior WHERE prior.source=$1 AND prior.task_id=$2 AND prior.logical_schema=$3 AND prior.logical_table=$4 AND NOT prior.deleted AND prior.row_key<s.row_key))',array_position(p_columns,col));
   expected:=format('(CASE WHEN %s THEN NULL ELSE (s.row_value->%L->>''value'')::%s END)',zero_date,col,typ);
   mismatch:=mismatch || CASE WHEN mismatch='' THEN '' ELSE ' OR ' END || format('(%s AND NOT %s)',zero_date,quality);
  END IF;
  mismatch:=mismatch || CASE WHEN mismatch='' THEN '' ELSE ' OR ' END || format(
   '(s.row_value->%L IS NULL OR CASE WHEN s.row_value->%L->>''type''=''null'' THEN t.%I IS NOT NULL ELSE %s IS DISTINCT FROM %s END)',
   col,col,mapped,CASE WHEN oid IN (114,3802) THEN format('t.%I::jsonb',mapped) ELSE format('t.%I',mapped) END,expected);
  IF col=ANY(p_keys) THEN
   key_columns:=key_columns || CASE WHEN key_columns='' THEN '' ELSE ',' END || format('%I',mapped);
   key_join:=key_join || CASE WHEN key_join='' THEN '' ELSE ' AND ' END || format('t.%I IS NOT DISTINCT FROM %s',mapped,expected);
  END IF;
 END LOOP;
 IF key_join='' OR NOT p_keys<@p_columns THEN RAISE EXCEPTION 'baseline keys' USING ERRCODE='OO030'; END IF;
 SELECT count(*) INTO n FROM _oo.native_c1_baseline_row WHERE source=p_source AND task_id=p_task
  AND logical_schema=p_schema AND logical_table=p_table AND NOT deleted;
 EXECUTE format('SELECT count(*) FROM _oo.%I',physical) INTO actual;
 IF actual<>n THEN RAISE EXCEPTION 'baseline typed row count' USING ERRCODE='OO030'; END IF;
 EXECUTE format('SELECT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_row s LEFT JOIN _oo.%I t ON %s WHERE s.source=$1 AND s.task_id=$2 AND s.logical_schema=$3 AND s.logical_table=$4 AND NOT s.deleted AND (t._oo_pos IS NULL OR t._oo_pos<>s.applied_pos OR %s))',physical,key_join,mismatch)
 INTO bad USING p_source,p_task,p_schema,p_table,p_run;
 IF bad THEN RAISE EXCEPTION 'baseline typed row mismatch' USING ERRCODE='OO030'; END IF;
 SET CONSTRAINTS ALL IMMEDIATE;
 IF NOT EXISTS(SELECT 1 FROM pg_catalog.pg_constraint WHERE conrelid=to_regclass(format('_oo.%I',physical)) AND contype='p') THEN
  EXECUTE format('ALTER TABLE _oo.%I ADD PRIMARY KEY (%s)',physical,key_columns);
 END IF;
 RETURN n;
END
$bridge$;
