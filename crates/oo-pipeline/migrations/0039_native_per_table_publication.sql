-- Per-table C4/C5 collections retain each table's actual sealed generation.
-- Existing installed publication functions and C1 receipts remain unchanged.
CREATE TABLE _oo.native_per_table_publication (
 receipt_sha256 text PRIMARY KEY CHECK(receipt_sha256 ~ '^[0-9a-f]{64}$'),
 anchor_run uuid NOT NULL REFERENCES _oo.full_import_run(run_id),
 source_id text NOT NULL, source_generation bigint NOT NULL, target_generation bigint NOT NULL,
 receipt jsonb NOT NULL, publication_xid xid8 NOT NULL,
 published_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
REVOKE ALL ON _oo.native_per_table_publication FROM PUBLIC,oo_pipeline_runtime;
CREATE TRIGGER native_per_table_publication_guard BEFORE UPDATE OR DELETE OR TRUNCATE
 ON _oo.native_per_table_publication FOR EACH STATEMENT EXECUTE FUNCTION _oo.native_publication_guard_receipt();
CREATE FUNCTION _oo.publish_native_per_table_collection(p_anchor uuid,p_sha text,p_receipt jsonb)
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
 -- Lock the entire source vector before checking completeness; a concurrent Loader
 -- cannot substitute another table generation while the grants are opened.
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
  IF NOT FOUND OR f.source_generation<>r.source_generation OR f.target_generation<>r.target_generation
   OR f.physical_name<>t.physical_name OR f.state NOT IN ('awaiting_publish','live')
   OR (method='c5_diff' AND (f.mode<>'full' OR f.last_pos IS DISTINCT FROM t.applied_pos))
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
