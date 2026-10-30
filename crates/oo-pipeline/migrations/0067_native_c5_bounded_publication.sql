-- native-c5-bounded-publication/v1
-- Bounded publication requires immutable overlay lineage and a complete receipted scan.
CREATE FUNCTION _oo.c5_bounded_publishable(p_source text,p_schema text,p_table text,p_run uuid)
RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
 SELECT EXISTS(
  SELECT FROM _oo.full_import_run r
  JOIN _oo.full_import_table t ON t.run_id=r.run_id
  JOIN _oo.c5_table c ON c.source=r.source_id AND c.schema_name=t.schema_ident AND c.table_name=t.table_ident
  JOIN _oo.c5_scan s ON s.source=c.source AND s.schema_name=c.schema_name AND s.table_name=c.table_name
   AND s.status='completed' AND s.completed_at=c.last_full_digest_scan_completed_at
  WHERE r.run_id=p_run AND r.source_id=p_source AND t.schema_ident=p_schema AND t.table_ident=p_table
   AND r.state IN ('awaiting_publish','published') AND t.exhausted
   AND c.source_generation=r.source_generation AND c.input_digest=r.input_digest
   AND s.completed_at>=r.sealed_at AND s.next_block=jsonb_array_length(s.expected_chunks)
   AND _oo.c5_overlay_is_valid(t.physical_name)
   AND EXISTS(SELECT FROM _oo.c5_multipart_baseline b
    WHERE b.source=c.source AND b.schema_name=c.schema_name AND b.table_name=c.table_name
     AND b.config_digest=r.config_digest AND b.schema_digest=r.schema_digest AND b.type_digest=r.type_digest)
   AND s.expected_chunks=(SELECT jsonb_agg(ch.chunk_id ORDER BY ch.chunk_id::numeric)
    FROM _oo.c5_chunk ch WHERE ch.source=c.source AND ch.schema_name=c.schema_name
     AND ch.table_name=c.table_name AND ch.retired_at IS NULL)
   AND NOT EXISTS(SELECT FROM _oo.c5_chunk ch WHERE ch.source=c.source AND ch.schema_name=c.schema_name
    AND ch.table_name=c.table_name AND ch.retired_at IS NULL
    AND (ch.last_digest_scan_id IS DISTINCT FROM s.scan_id OR NOT _oo.c5_digest_receipt_current(ch)))
 );
$$;
REVOKE ALL ON FUNCTION _oo.c5_bounded_publishable(text,text,text,uuid) FROM PUBLIC,oo_pipeline_runtime;

-- native-c5-wide-columns/v1: raise the per-table column caps of the sealed catalog and multipart cell reader from 64 to 1024.
CREATE OR REPLACE FUNCTION _oo.full_import_native_catalog(
  p_run_id uuid,
  p_source text,
  p_owner uuid,
  p_config_digest text,
  p_source_generation bigint,
  p_target_generation bigint,
  p_schema_digest text,
  p_type_digest text,
  p_input_digest text
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $native$
DECLARE
  v_run _oo.full_import_run%ROWTYPE;
  v_partial integer;
  v_hooks integer;
  v_hook text;
  v_tables jsonb := '[]'::jsonb;
  v_table _oo.full_import_table%ROWTYPE;
  v_namespace text;
  v_mapped_table text;
  v_rel oid;
  v_mapped integer;
  v_attributes integer;
  v_joined integer;
  v_columns jsonb;
  v_quality jsonb;
BEGIN
  IF p_run_id IS NULL
     OR p_source IS NULL OR p_source !~ '^[a-z][a-z0-9_]{0,62}$'
     OR p_owner IS NULL
     OR p_config_digest IS NULL OR p_config_digest !~ '^[0-9a-f]{64}$'
     OR p_schema_digest IS NULL OR p_schema_digest !~ '^[0-9a-f]{64}$'
     OR p_type_digest IS NULL OR p_type_digest !~ '^[0-9a-f]{64}$'
     OR p_input_digest IS NULL OR p_input_digest !~ '^[0-9a-f]{64}$'
     OR p_source_generation IS NULL OR p_source_generation < 1
     OR p_target_generation IS NULL OR p_target_generation < 1
  THEN
    RAISE EXCEPTION 'sealed import identity is closed' USING ERRCODE = '22023';
  END IF;
  SELECT run_row.*
    INTO v_run
  FROM _oo.full_import_run AS run_row
  WHERE run_row.run_id = p_run_id;
  IF NOT FOUND OR v_run.state <> 'awaiting_publish' THEN
    RAISE EXCEPTION 'sealed import is absent' USING ERRCODE = 'P0002';
  END IF;
  IF v_run.source_id <> p_source THEN
    RAISE EXCEPTION 'sealed import source' USING ERRCODE = '22023';
  END IF;
  IF v_run.owner_token <> p_owner THEN
    RAISE EXCEPTION 'sealed import owner' USING ERRCODE = '42501';
  END IF;
  IF v_run.config_digest <> p_config_digest
     OR v_run.schema_digest <> p_schema_digest
     OR v_run.type_digest <> p_type_digest
     OR v_run.input_digest <> p_input_digest
     OR v_run.source_generation <> p_source_generation
     OR v_run.target_generation <> p_target_generation
  THEN
    RAISE EXCEPTION 'sealed import digest' USING ERRCODE = '22023';
  END IF;
  SELECT count(*)::integer
    INTO v_partial
  FROM _oo.full_import_table AS table_row
  WHERE table_row.run_id = v_run.run_id
    AND NOT table_row.exhausted;
  IF v_partial <> 0 THEN
    RAISE EXCEPTION 'sealed import is partial' USING ERRCODE = 'P0002';
  END IF;
  SELECT count(*)::integer
    INTO v_partial
  FROM _oo.full_import_table AS table_row
  WHERE table_row.run_id = v_run.run_id;
  IF v_partial < 1 OR v_partial > 16 THEN
    RAISE EXCEPTION 'sealed import is partial' USING ERRCODE = 'P0002';
  END IF;
  SELECT count(DISTINCT receipt.hook_digest)::integer
    INTO v_hooks
  FROM _oo.apply_receipts AS receipt
  WHERE receipt.source = v_run.source_id
    AND receipt.config_digest = v_run.config_digest
    AND receipt.schema_digest = v_run.schema_digest
    AND receipt.type_digest = v_run.type_digest;
  IF v_hooks <> 1 THEN
    RAISE EXCEPTION 'sealed import digest' USING ERRCODE = '22023';
  END IF;
  SELECT min(receipt.hook_digest)
    INTO v_hook
  FROM _oo.apply_receipts AS receipt
  WHERE receipt.source = v_run.source_id
    AND receipt.config_digest = v_run.config_digest
    AND receipt.schema_digest = v_run.schema_digest
    AND receipt.type_digest = v_run.type_digest;
  FOR v_table IN
    SELECT table_row.*
    FROM _oo.full_import_table AS table_row
    WHERE table_row.run_id = v_run.run_id
    ORDER BY table_row.table_index
  LOOP
    IF v_table.canonical_digest IS NULL OR v_table.applied_rows < 0 THEN
      RAISE EXCEPTION 'sealed import is partial' USING ERRCODE = 'P0002';
    END IF;
    SELECT mapping.physical_name
      INTO v_namespace
    FROM _oo.physical_mappings AS mapping
    WHERE mapping.source = v_run.source_id
      AND mapping.config_digest = v_run.config_digest
      AND mapping.run_generation = v_run.source_generation
      AND mapping.level = 'n'
      AND mapping.logical_namespace = v_table.schema_ident
      AND mapping.logical_table = ''
      AND mapping.logical_column = '';
    IF NOT FOUND THEN
      RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'P0002';
    END IF;
    SELECT mapping.physical_name
      INTO v_mapped_table
    FROM _oo.physical_mappings AS mapping
    WHERE mapping.source = v_run.source_id
      AND mapping.config_digest = v_run.config_digest
      AND mapping.run_generation = v_run.source_generation
      AND mapping.level = 't'
      AND mapping.logical_namespace = v_table.schema_ident
      AND mapping.logical_table = v_table.table_ident
      AND mapping.logical_column = '';
    IF NOT FOUND OR v_mapped_table <> v_table.physical_name THEN
      RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'P0002';
    END IF;
    SELECT relation.oid
      INTO v_rel
    FROM pg_catalog.pg_class AS relation
    JOIN pg_catalog.pg_namespace AS namespace
      ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = '_oo'
      AND relation.relname = v_table.physical_name
      AND (relation.relkind = 'r' OR (relation.relkind = 'v' AND _oo.c5_overlay_is_valid(v_table.physical_name)));
    IF NOT FOUND THEN
      RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'P0002';
    END IF;
    SELECT count(*)::integer
      INTO v_mapped
    FROM _oo.physical_mappings AS mapping
    WHERE mapping.source = v_run.source_id
      AND mapping.config_digest = v_run.config_digest
      AND mapping.run_generation = v_run.source_generation
      AND mapping.level = 'c'
      AND mapping.logical_namespace = v_table.schema_ident
      AND mapping.logical_table = v_table.table_ident;
    SELECT count(*)::integer
      INTO v_attributes
    FROM pg_catalog.pg_attribute AS attribute
    WHERE attribute.attrelid = v_rel
      AND attribute.attnum > 0
      AND NOT attribute.attisdropped
      AND attribute.attname <> '_oo_pos';
    SELECT count(*)::integer
      INTO v_joined
    FROM _oo.physical_mappings AS mapping
    JOIN pg_catalog.pg_attribute AS attribute
      ON attribute.attrelid = v_rel
     AND attribute.attname = mapping.physical_name
     AND attribute.attnum > 0
     AND NOT attribute.attisdropped
    WHERE mapping.source = v_run.source_id
      AND mapping.config_digest = v_run.config_digest
      AND mapping.run_generation = v_run.source_generation
      AND mapping.level = 'c'
      AND mapping.logical_namespace = v_table.schema_ident
      AND mapping.logical_table = v_table.table_ident;
    IF v_mapped < 1 OR v_mapped > 1024 OR v_mapped <> v_attributes OR v_joined <> v_mapped THEN
      RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'P0002';
    END IF;
    SELECT coalesce(jsonb_agg(jsonb_build_object(
        'attnum', attribute.attnum,
        'atttypid', attribute.atttypid::bigint,
        'atttypmod', attribute.atttypmod,
        'logical_column', mapping.logical_column,
        'physical_name', mapping.physical_name
      ) ORDER BY attribute.attnum), '[]'::jsonb)
      INTO v_columns
    FROM _oo.physical_mappings AS mapping
    JOIN pg_catalog.pg_attribute AS attribute
      ON attribute.attrelid = v_rel
     AND attribute.attname = mapping.physical_name
     AND attribute.attnum > 0
     AND NOT attribute.attisdropped
    WHERE mapping.source = v_run.source_id
      AND mapping.config_digest = v_run.config_digest
      AND mapping.run_generation = v_run.source_generation
      AND mapping.level = 'c'
      AND mapping.logical_namespace = v_table.schema_ident
      AND mapping.logical_table = v_table.table_ident;
    v_tables := v_tables || jsonb_build_array(jsonb_build_object(
      'applied_rows', v_table.applied_rows,
      'canonical_digest', v_table.canonical_digest,
      'columns', v_columns,
      'physical_name', v_table.physical_name,
      'schema_ident', v_table.schema_ident,
      'stored_namespace', v_namespace,
      'table_ident', v_table.table_ident,
      'table_index', v_table.table_index
    ));
  END LOOP;
  SELECT coalesce(jsonb_agg(jsonb_build_object(
      'accepted_rows', quality_row.accepted_rows,
      'attnum', quality_row.attnum,
      'batch_id', quality_row.batch_id,
      'key_digest', quality_row.key_digest,
      'kind', quality_row.kind,
      'physical_name', quality_row.physical_name,
      'reason', quality_row.reason,
      'rejected_rows', quality_row.rejected_rows,
      'row_index', quality_row.row_index,
      'value_len', quality_row.value_len
    ) ORDER BY quality_row.quality_id), '[]'::jsonb)
    INTO v_quality
  FROM _oo.full_import_quality AS quality_row
  WHERE quality_row.run_id = v_run.run_id;
  RETURN jsonb_build_object(
    'config_digest', v_run.config_digest,
    'input_digest', v_run.input_digest,
    'owner', v_run.owner_token,
    'quality', v_quality,
    'run_id', v_run.run_id,
    'schema_digest', v_run.schema_digest,
    'source_generation', v_run.source_generation,
    'source_id', v_run.source_id,
    'state', v_run.state,
    'tables', v_tables,
    'target_generation', v_run.target_generation,
    'tool_hook_digest', v_hook,
    'type_digest', v_run.type_digest
  );
END;
$native$;

REVOKE ALL ON FUNCTION _oo.full_import_native_catalog(uuid, text, uuid, text, bigint, bigint, text, text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.full_import_native_catalog(uuid, text, uuid, text, bigint, bigint, text, text, text) TO oo_pipeline_runtime;

COMMENT ON FUNCTION _oo.full_import_native_catalog(uuid, text, uuid, text, bigint, bigint, text, text, text) IS
  'Read one awaiting_publish native import by run id. Attribute types come from pg_attribute. Shadow values are not read.';


CREATE OR REPLACE FUNCTION _oo.publish_native_per_table_collection(p_anchor uuid,p_sha text,p_receipt jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $publish$
DECLARE w _oo.source_watermark%ROWTYPE; r _oo.full_import_run%ROWTYPE;
 t _oo.full_import_table%ROWTYPE; f _oo.table_freshness%ROWTYPE;
 e jsonb; n integer; changed integer; accepted bigint; rejected bigint; batches bigint;
 source_name text:=p_receipt->>'source_id'; source_gen bigint:=(p_receipt->>'source_generation')::bigint;
 target_gen bigint:=(p_receipt->>'target_generation')::bigint; method text;
BEGIN
 IF p_anchor IS NULL OR p_sha IS NULL OR p_sha !~ '^[0-9a-f]{64}$'
  OR p_receipt->>'protocol' IS DISTINCT FROM 'oo.pipeline.native-publication-receipt/v1'
  OR p_receipt->>'collection_mode' IS DISTINCT FROM 'per_table'
  OR p_receipt->>'run_id' IS DISTINCT FROM p_anchor::text
  OR p_receipt->>'consistency' IS DISTINCT FROM 'per_table'
  OR coalesce(p_receipt->>'consistency_grade','') NOT IN ('S0','B')
  OR p_receipt->>'cross_table_consistent' IS DISTINCT FROM 'false'
  OR p_receipt->>'position_codec' NOT IN ('snapshot-zero/v1','partition-signature/v1')
  OR (p_receipt->>'applied_pos')::numeric IS DISTINCT FROM 0
  OR jsonb_typeof(p_receipt->'tables') IS DISTINCT FROM 'array'
  OR jsonb_typeof(p_receipt->'run_ids') IS DISTINCT FROM 'array'
 THEN RAISE EXCEPTION 'closed per-table publication receipt required' USING ERRCODE='OO030'; END IF;
 n:=jsonb_array_length(p_receipt->'tables');
 IF n<1 OR n>32 OR jsonb_array_length(p_receipt->'run_ids')<>n
  OR (SELECT count(DISTINCT value) FROM jsonb_array_elements(p_receipt->'run_ids'))<>n
  OR (SELECT count(DISTINCT (value->>'schema',value->>'table')) FROM jsonb_array_elements(p_receipt->'tables'))<>n
  OR (SELECT count(DISTINCT value->>'run_id') FROM jsonb_array_elements(p_receipt->'tables'))<>n
  OR NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p_receipt->'tables') x WHERE x->>'run_id'=p_anchor::text
   AND (x->>'source_generation')::bigint=source_gen AND (x->>'target_generation')::bigint=target_gen)
 THEN RAISE EXCEPTION 'complete per-table vector required' USING ERRCODE='OO030'; END IF;
 SELECT * INTO w FROM _oo.source_watermark WHERE source=source_name FOR UPDATE;
 IF NOT FOUND OR w.source_generation IS DISTINCT FROM source_gen OR w.target_generation IS DISTINCT FROM target_gen
  OR w.consistency<>'per_table' OR w.cross_table_consistent OR w.consistency_grade NOT IN ('S0','B')
  OR w.position_codec IS DISTINCT FROM p_receipt->>'position_codec' OR w.applied_pos<>0
  OR w.position_lineage IS DISTINCT FROM p_receipt->>'position_lineage'
  OR (w.consistency_grade='B' AND p_receipt->>'consistency_grade'<>'B')
 THEN RAISE EXCEPTION 'current per-table source cut required' USING ERRCODE='OO030'; END IF;
 PERFORM 1 FROM _oo.table_freshness WHERE source=source_name ORDER BY schema_name,table_name FOR UPDATE;
 IF (SELECT count(*) FROM _oo.table_freshness WHERE source=source_name)<>n THEN
  RAISE EXCEPTION 'publication omits current source tables' USING ERRCODE='OO030'; END IF;
 FOR e IN SELECT value FROM jsonb_array_elements(p_receipt->'tables') LOOP
  method:=e->>'capture_method';
  IF NOT p_receipt->'run_ids' @> jsonb_build_array(e->>'run_id')
   OR (method IS DISTINCT FROM 'c4_partition' AND method IS DISTINCT FROM 'c5_diff')
   OR e->>'delete_detection' IS DISTINCT FROM (CASE WHEN method='c4_partition' THEN 'partition_replace' ELSE 'reconcile' END)
   OR coalesce(e->>'consistency_grade','') NOT IN ('S0','B')
   OR (p_receipt->>'consistency_grade'='S0' AND e->>'consistency_grade' IS DISTINCT FROM 'S0')
  THEN RAISE EXCEPTION 'per-table method identity differs' USING ERRCODE='OO030'; END IF;
  SELECT * INTO r FROM _oo.full_import_run WHERE run_id=(e->>'run_id')::uuid FOR UPDATE;
  IF NOT FOUND OR r.state NOT IN ('awaiting_publish','published') OR r.source_id<>source_name
   OR r.source_generation IS DISTINCT FROM (e->>'source_generation')::bigint
   OR r.target_generation IS DISTINCT FROM (e->>'target_generation')::bigint
   OR r.source_generation>source_gen OR r.target_generation>target_gen
   OR (SELECT count(*) FROM _oo.full_import_table WHERE run_id=r.run_id)<>1
  THEN RAISE EXCEPTION 'per-table sealed run differs' USING ERRCODE='OO030'; END IF;
  SELECT * INTO t FROM _oo.full_import_table WHERE run_id=r.run_id AND schema_ident=e->>'schema' AND table_ident=e->>'table' FOR UPDATE;
  IF NOT FOUND OR NOT t.exhausted OR t.physical_name IS DISTINCT FROM e->>'physical_name'
   OR t.applied_rows IS DISTINCT FROM (e->>'accepted_rows')::bigint
   OR t.canonical_digest IS DISTINCT FROM e->>'canonical_digest'
   OR t.applied_pos IS DISTINCT FROM (e->>'applied_pos')::numeric
  THEN RAISE EXCEPTION 'per-table sealed table differs' USING ERRCODE='OO030'; END IF;
  SELECT count(*),coalesce(sum(accepted_rows),0),coalesce(sum(rejected_rows),0) INTO batches,accepted,rejected
   FROM _oo.full_import_quality WHERE run_id=r.run_id AND physical_name=t.physical_name AND kind='batch';
  IF batches=0 OR accepted<>t.applied_rows OR rejected<>0 OR EXISTS(
   SELECT 1 FROM _oo.full_import_quality WHERE run_id=r.run_id AND kind='reject')
  THEN RAISE EXCEPTION 'per-table quality proof differs' USING ERRCODE='OO030'; END IF;
  SELECT * INTO f FROM _oo.table_freshness WHERE source=source_name AND schema_name=t.schema_ident AND table_name=t.table_ident;
  IF method='c5_diff' AND f.mode='incremental' AND EXISTS(
   SELECT 1 FROM _oo.c5_block_receipt b WHERE b.run_id=r.run_id AND (
    b.scope->'lo' IS DISTINCT FROM 'null'::jsonb OR b.scope->'hi' IS DISTINCT FROM 'null'::jsonb
    OR (SELECT count(*) FROM _oo.c5_chunk active WHERE active.source=b.source AND active.schema_name=b.schema_name
      AND active.table_name=b.table_name AND active.retired_at IS NULL)<>1))
   AND NOT _oo.c5_bounded_publishable(source_name,t.schema_ident,t.table_ident,r.run_id)
  THEN RAISE EXCEPTION 'C5 bounded publication requires complete scan and immutable overlay' USING ERRCODE='OO030'; END IF;
  IF NOT FOUND OR f.source_generation<>r.source_generation OR f.target_generation<>r.target_generation
   OR f.physical_name<>t.physical_name OR f.state NOT IN ('awaiting_publish','live')
   OR (method='c5_diff' AND (f.last_pos IS DISTINCT FROM t.applied_pos OR NOT (
      f.mode='full' OR (f.mode='incremental' AND EXISTS(
       SELECT 1 FROM _oo.c5_block_receipt b JOIN _oo.c5_table c
        ON c.source=b.source AND c.schema_name=b.schema_name AND c.table_name=b.table_name
       WHERE b.run_id=r.run_id AND b.source=r.source_id AND b.schema_name=t.schema_ident AND b.table_name=t.table_ident
        AND b.source_generation=r.source_generation AND b.target_generation=r.target_generation
        AND b.scope->>'arrow_sha256'=r.input_digest AND c.source_generation=r.source_generation
        AND c.input_digest=r.input_digest AND c.schema_fingerprint=b.scope->>'schema_fingerprint'
        AND ((b.scope->'lo'='null'::jsonb AND b.scope->'hi'='null'::jsonb
        AND (SELECT count(*) FROM _oo.c5_chunk active WHERE active.source=b.source AND active.schema_name=b.schema_name
          AND active.table_name=b.table_name AND active.retired_at IS NULL)=1)
          OR _oo.c5_bounded_publishable(source_name,t.schema_ident,t.table_ident,r.run_id))
      )))))
   OR (method='c4_partition' AND (f.mode<>'incremental' OR f.last_pos IS NOT NULL))
  THEN RAISE EXCEPTION 'per-table current freshness differs' USING ERRCODE='OO030'; END IF;
  IF method='c4_partition' AND (NOT EXISTS(SELECT 1 FROM _oo.native_partition_state s
    WHERE s.source=source_name AND s.schema_name=t.schema_ident AND s.table_name=t.table_ident
     AND s.run_id=r.run_id AND s.source_generation=r.source_generation AND s.target_generation=r.target_generation AND s.acknowledged)
   OR EXISTS(SELECT 1 FROM _oo.native_partition_state s WHERE s.source=source_name
    AND s.schema_name=t.schema_ident AND s.table_name=t.table_ident AND NOT s.acknowledged))
  THEN RAISE EXCEPTION 'C4 partition has no confirmed ACK' USING ERRCODE='OO030'; END IF;
  IF r.state='published' AND NOT EXISTS(SELECT 1 FROM _oo.native_publication_receipt p
    WHERE p.run_id=r.run_id AND p.source_id=source_name AND p.source_generation=r.source_generation
    AND p.target_generation=r.target_generation AND p.table_vector @> jsonb_build_array(e))
  THEN RAISE EXCEPTION 'published table proof differs' USING ERRCODE='OO030'; END IF;
 END LOOP;
 IF EXISTS(SELECT 1 FROM _oo.native_per_table_publication WHERE receipt_sha256=p_sha) THEN
  IF NOT EXISTS(SELECT 1 FROM _oo.native_per_table_publication WHERE receipt_sha256=p_sha AND receipt=p_receipt)
   OR w.state<>'live' THEN RAISE EXCEPTION 'per-table replay conflicts' USING ERRCODE='OO030'; END IF;
  RETURN jsonb_build_object('outcome','replay','run_id',p_anchor,'tables',n,'receipt_sha256',p_sha);
 END IF;
 IF w.state<>'lagging' THEN RAISE EXCEPTION 'per-table cut not awaiting publication' USING ERRCODE='OO030'; END IF;
 INSERT INTO _oo.native_per_table_publication(receipt_sha256,anchor_run,source_id,source_generation,target_generation,receipt,publication_xid)
  VALUES(p_sha,p_anchor,source_name,source_gen,target_gen,p_receipt,pg_current_xact_id());
 FOR e IN SELECT value FROM jsonb_array_elements(p_receipt->'tables') LOOP
  SELECT * INTO r FROM _oo.full_import_run WHERE run_id=(e->>'run_id')::uuid;
  IF r.state='awaiting_publish' THEN
   INSERT INTO _oo.native_publication_receipt(run_id,receipt_sha256,source_id,source_generation,target_generation,
    position_codec,applied_pos,consistency,consistency_grade,cross_table_consistent,table_vector,publication_xid)
   VALUES(r.run_id,p_sha,source_name,r.source_generation,r.target_generation,p_receipt->>'position_codec',0,
    'per_table',p_receipt->>'consistency_grade',false,p_receipt->'tables',pg_current_xact_id());
   UPDATE _oo.full_import_run SET state='published' WHERE run_id=r.run_id AND state='awaiting_publish';
   IF NOT FOUND THEN RAISE EXCEPTION 'per-table publication raced' USING ERRCODE='OO030'; END IF;
  END IF;
  UPDATE _oo.table_freshness SET state='live',applied_at=clock_timestamp() WHERE source=source_name
   AND schema_name=e->>'schema' AND table_name=e->>'table';
 END LOOP;
 UPDATE _oo.source_watermark SET state='live',consistency_grade=p_receipt->>'consistency_grade',applied_at=clock_timestamp()
  WHERE source=source_name;
 RETURN jsonb_build_object('outcome','published','run_id',p_anchor,'source_id',source_name,
  'source_generation',source_gen,'target_generation',target_gen,'tables',n,'receipt_sha256',p_sha,
  'consistency','per_table','consistency_grade',p_receipt->>'consistency_grade','cross_table_consistent',false);
END
$publish$;
REVOKE ALL ON FUNCTION _oo.publish_native_per_table_collection(uuid,text,jsonb) FROM PUBLIC,oo_pipeline_runtime;
DO $grant$
BEGIN
 IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_target_admin') THEN
  GRANT EXECUTE ON FUNCTION _oo.publish_native_per_table_collection(uuid,text,jsonb) TO oo_target_admin;
 END IF;
END
$grant$;
