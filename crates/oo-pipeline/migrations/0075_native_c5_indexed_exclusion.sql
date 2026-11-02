-- native-c5-indexed-exclusion/v1
-- Keep exclusion lookup parameterized. Flattening this anti join can choose
-- a materialized nested loop with quadratic work when LIMIT probes a replaced prefix.
CREATE OR REPLACE FUNCTION _oo.c5_overlay_finish_before_visibility(p_shard text,p_owner uuid,p_fence bigint,p_run uuid)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE o _oo.c5_overlay; r _oo.full_import_run; keys text[]; expression text;
  delta_count bigint; matched bigint; unique_count bigint; projection text:='p."_oo_pos"';
  delta_projection text:='d."_oo_pos"'; statement text:=''; i integer; view_id oid; component record; exclusions text[];
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
  SELECT * INTO STRICT r FROM _oo.full_import_run WHERE run_id=p_run FOR UPDATE;
  SELECT * INTO STRICT o FROM _oo.c5_overlay WHERE run_id=p_run FOR UPDATE;
  IF r.state<>'open' OR r.shard_id IS DISTINCT FROM p_shard OR r.owner_token IS DISTINCT FROM p_owner
    OR r.target_generation IS DISTINCT FROM p_fence OR o.prepared_xid<>pg_current_xact_id() THEN
    RAISE EXCEPTION 'C5 overlay finish binding' USING ERRCODE='OO030';
  END IF;
  IF o.view_oid IS NOT NULL THEN
    IF NOT _oo.c5_overlay_relation_ok(o.physical_name) THEN
      RAISE EXCEPTION 'C5 overlay view changed' USING ERRCODE='OO030';
    END IF;
    RETURN o.inherited_rows+o.delta_rows;
  END IF;
  IF to_regclass(format('_oo.%I',o.physical_name)) IS DISTINCT FROM o.delta_oid THEN
    RAISE EXCEPTION 'C5 delta relation changed' USING ERRCODE='OO030';
  END IF;
  SELECT array_agg(o.child_columns[array_position(o.column_idents,k)] ORDER BY ord)
    INTO keys FROM unnest(o.key_idents) WITH ORDINALITY x(k,ord);
  expression:=_oo.c5_overlay_key_sql(o.delta_oid,keys,'d');
  EXECUTE format('SELECT count(*),count(DISTINCT %s),count(*) FILTER (WHERE %s IN
    (SELECT _oo.c5_key_identity(value) FROM jsonb_array_elements($1))) FROM _oo.%I d',
    expression,expression,o.physical_name)
    INTO delta_count,unique_count,matched USING o.scope->'members';
  IF delta_count<>(o.scope->>'row_count')::bigint OR delta_count<>unique_count OR matched<>delta_count THEN
    RAISE EXCEPTION 'C5 delta keys/count differ from range' USING ERRCODE='OO030';
  END IF;
  IF o.root_name IS NOT NULL THEN
    IF to_regclass(format('_oo.%I',o.root_name)) IS DISTINCT FROM o.root_oid THEN
      RAISE EXCEPTION 'C5 root relation changed' USING ERRCODE='OO030';
    END IF;
    FOR i IN 1..cardinality(o.child_columns) LOOP
      projection:=projection||format(',p.%I AS %I',o.root_columns[i],o.child_columns[i]);
    END LOOP;
    SELECT array_agg(o.root_columns[array_position(o.column_idents,k)] ORDER BY ord)
      INTO keys FROM unnest(o.key_idents) WITH ORDINALITY x(k,ord);
    SELECT array_agg(c.exclusion_owner ORDER BY c.chunk_id COLLATE "C") INTO exclusions
      FROM _oo.c5_overlay_component c WHERE c.physical_name=o.physical_name;
    expression:=_oo.c5_overlay_key_sql(o.root_oid,keys,'p');
    statement:=format('SELECT %s FROM _oo.%I p WHERE NOT EXISTS
      (SELECT FROM _oo.c5_overlay_exclusion e WHERE e.physical_name=ANY(%L::text[]) AND e.key_identity=%s OFFSET 0)',
      projection,o.root_name,exclusions,expression);
  END IF;
  FOR component IN SELECT d.* FROM _oo.c5_overlay_component c JOIN _oo.c5_overlay d ON d.physical_name=c.delta_owner
    WHERE c.physical_name=o.physical_name ORDER BY c.chunk_id COLLATE "C" LOOP
    delta_projection:='d."_oo_pos"';
    FOR i IN 1..cardinality(o.child_columns) LOOP
      delta_projection:=delta_projection||format(',d.%I AS %I',component.child_columns[i],o.child_columns[i]);
    END LOOP;
    IF statement<>'' THEN statement:=statement||' UNION ALL '; END IF;
    statement:=statement||format('SELECT %s FROM _oo.%I d',delta_projection,component.delta_name);
  END LOOP;
  -- Even a single-component rootless view is intrinsically read-only.
  statement:=statement||format(' UNION ALL SELECT %s FROM _oo.%I d WHERE false',delta_projection,component.delta_name);
  -- ALTER fails if COPY's deferred fence triggers have not been flushed.
  -- Their original guards remain attached to pd_; no open run names that heap.
  EXECUTE format('ALTER TABLE _oo.%I RENAME TO %I',o.physical_name,o.delta_name);
  EXECUTE format('REVOKE ALL ON _oo.%I FROM PUBLIC,oo_pipeline_runtime',o.delta_name);
  EXECUTE format('CREATE VIEW _oo.%I AS %s',o.physical_name,statement);
  EXECUTE format('REVOKE ALL ON _oo.%I FROM PUBLIC,oo_pipeline_runtime',o.physical_name);
  view_id:=to_regclass(format('_oo.%I',o.physical_name));
  IF EXISTS(SELECT FROM pg_attribute d FULL JOIN pg_attribute v
      ON v.attrelid=view_id AND v.attnum=d.attnum AND v.attnum>0 AND NOT v.attisdropped
      WHERE d.attrelid=o.delta_oid AND d.attnum>0 AND NOT d.attisdropped
        AND (d.attname,d.atttypid,d.atttypmod,d.attcollation) IS DISTINCT FROM (v.attname,v.atttypid,v.atttypmod,v.attcollation)) THEN
    RAISE EXCEPTION 'C5 view changed strict schema types' USING ERRCODE='23514';
  END IF;
  UPDATE _oo.c5_overlay SET view_oid=view_id,view_definition=pg_get_viewdef(view_id,false),
    view_sha256=encode(sha256(convert_to(pg_get_viewdef(view_id,false),'UTF8')),'hex'),delta_rows=delta_count
    WHERE physical_name=o.physical_name;
  RETURN o.inherited_rows+delta_count;
END $$;

-- Rebuild only authenticated gated views, preserving OIDs, column types,
-- visibility latches and immutable heaps. No source read or data copy occurs.
DO $rebuild$
DECLARE o _oo.c5_overlay; component record; keys text[]; exclusions text[];
  projection text; delta_projection text; statement text; expression text; i integer;
BEGIN
 PERFORM set_config('search_path','pg_catalog,_oo,pg_temp',true);
 FOR o IN SELECT * FROM _oo.c5_overlay WHERE visibility_gate ORDER BY physical_name FOR UPDATE LOOP
  IF NOT _oo.c5_overlay_is_valid(o.physical_name) THEN
   RAISE EXCEPTION 'C5 exclusion upgrade requires a valid gated view' USING ERRCODE='OO030';
  END IF;
  statement:=''; projection:='p."_oo_pos"'; delta_projection:='d."_oo_pos"';
  IF o.root_name IS NOT NULL THEN
   FOR i IN 1..cardinality(o.child_columns) LOOP
    projection:=projection||format(',p.%I AS %I',o.root_columns[i],o.child_columns[i]);
   END LOOP;
   SELECT array_agg(o.root_columns[array_position(o.column_idents,k)] ORDER BY ord)
    INTO keys FROM unnest(o.key_idents) WITH ORDINALITY x(k,ord);
   SELECT array_agg(c.exclusion_owner ORDER BY c.chunk_id COLLATE "C") INTO exclusions
    FROM _oo.c5_overlay_component c WHERE c.physical_name=o.physical_name;
   expression:=_oo.c5_overlay_key_sql(o.root_oid,keys,'p');
   statement:=format('SELECT %s FROM _oo.%I p WHERE NOT EXISTS
    (SELECT FROM _oo.c5_overlay_exclusion e WHERE e.physical_name=ANY(%L::text[])
     AND e.key_identity=%s OFFSET 0)',projection,o.root_name,exclusions,expression);
  END IF;
  FOR component IN SELECT d.* FROM _oo.c5_overlay_component c
   JOIN _oo.c5_overlay d ON d.physical_name=c.delta_owner
   WHERE c.physical_name=o.physical_name ORDER BY c.chunk_id COLLATE "C" LOOP
   delta_projection:='d."_oo_pos"';
   FOR i IN 1..cardinality(o.child_columns) LOOP
    delta_projection:=delta_projection||format(',d.%I AS %I',component.child_columns[i],o.child_columns[i]);
   END LOOP;
   IF statement<>'' THEN statement:=statement||' UNION ALL '; END IF;
   statement:=statement||format('SELECT %s FROM _oo.%I d',delta_projection,component.delta_name);
  END LOOP;
  statement:=statement||format(' UNION ALL SELECT %s FROM _oo.%I d WHERE false',delta_projection,component.delta_name);
  EXECUTE format('CREATE OR REPLACE VIEW _oo.%I WITH (security_barrier=true) AS
   SELECT gated.* FROM (%s) gated WHERE _oo.c5_overlay_visible(%L::text,CURRENT_USER)',
   o.physical_name,statement,o.physical_name);
  UPDATE _oo.c5_overlay SET view_definition=pg_get_viewdef(o.view_oid,false),
   view_sha256=encode(sha256(convert_to(pg_get_viewdef(o.view_oid,false),'UTF8')),'hex')
   WHERE physical_name=o.physical_name;
  IF NOT _oo.c5_overlay_is_valid(o.physical_name) THEN
   RAISE EXCEPTION 'C5 exclusion upgrade changed view identity' USING ERRCODE='OO030';
  END IF;
 END LOOP;
END
$rebuild$;
