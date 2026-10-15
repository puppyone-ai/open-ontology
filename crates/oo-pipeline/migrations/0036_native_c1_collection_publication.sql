-- Legacy receipts have no transaction identity and cannot authorize this transition.
ALTER TABLE _oo.native_publication_receipt ADD COLUMN publication_xid xid8;
CREATE OR REPLACE FUNCTION _oo.full_import_guard_run()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $guard$
BEGIN
 IF TG_OP='UPDATE' AND OLD.state='awaiting_publish' AND NEW.state='published'
  AND (to_jsonb(NEW)-'state')=(to_jsonb(OLD)-'state')
  AND EXISTS(SELECT 1 FROM _oo.native_publication_receipt p WHERE p.run_id=NEW.run_id
   AND p.source_id=NEW.source_id AND p.source_generation=NEW.source_generation
   AND p.target_generation=NEW.target_generation AND p.publication_xid=pg_current_xact_id())
 THEN RETURN NEW; END IF;
 IF TG_OP IN ('DELETE','TRUNCATE') THEN RAISE EXCEPTION 'full import run cannot be removed' USING ERRCODE='OO014'; END IF;
 PERFORM _oo.full_import_assert_bound(NEW.shard_id,NEW.owner_token,NEW.target_generation);
 RETURN NEW;
END
$guard$;
-- Publish the complete acknowledged C1 five-table cut in the coordinator's
-- existing fence/grant transaction. Legacy single-run publication stays intact.
CREATE FUNCTION _oo.publish_native_c1_collection(p_anchor uuid,p_sha text,p_receipt jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $publish$
DECLARE b _oo.native_c1_baseline_typed%ROWTYPE; w _oo.source_watermark%ROWTYPE;
 r _oo.full_import_run%ROWTYPE; t _oo.full_import_table%ROWTYPE; e jsonb; q record;
 n integer; changed integer; rejected bigint; accepted bigint; count_rejects bigint;
 v_source text:=p_receipt->>'source_id'; source_gen bigint:=(p_receipt->>'source_generation')::bigint;
 target_gen bigint:=(p_receipt->>'target_generation')::bigint; pos numeric:=(p_receipt->>'applied_pos')::numeric;
BEGIN
 IF p_anchor IS NULL OR p_sha IS NULL OR p_sha !~ '^[0-9a-f]{64}$'
  OR p_receipt->>'protocol' IS DISTINCT FROM 'oo.pipeline.native-publication-receipt/v1'
  OR p_receipt->>'run_id' IS DISTINCT FROM p_anchor::text
  OR jsonb_typeof(p_receipt->'tables') IS DISTINCT FROM 'array'
  OR jsonb_typeof(p_receipt->'run_ids') IS DISTINCT FROM 'array'
  OR p_receipt->>'position_codec' IS DISTINCT FROM 'mysql-gtid/v1'
  OR p_receipt->>'consistency' IS DISTINCT FROM 'transactional'
  OR p_receipt->>'consistency_grade' IS DISTINCT FROM 'T'
  OR p_receipt->>'cross_table_consistent' IS DISTINCT FROM 'true'
 THEN RAISE EXCEPTION 'closed C1 publication receipt' USING ERRCODE='OO030'; END IF;
 n:=jsonb_array_length(p_receipt->'tables');
 IF n<1 OR n>32 OR jsonb_array_length(p_receipt->'run_ids')<>n
  OR (SELECT count(DISTINCT value) FROM jsonb_array_elements(p_receipt->'run_ids'))<>n
  OR (SELECT count(DISTINCT (value->>'schema',value->>'table')) FROM jsonb_array_elements(p_receipt->'tables'))<>n
  OR NOT p_receipt->'run_ids' @> jsonb_build_array(p_anchor::text)
 THEN RAISE EXCEPTION 'complete C1 publication vector required' USING ERRCODE='OO030'; END IF;
 SELECT * INTO w FROM _oo.source_watermark WHERE source=v_source FOR UPDATE;
 IF NOT FOUND OR w.source_generation IS DISTINCT FROM source_gen OR w.target_generation IS DISTINCT FROM target_gen
  OR w.applied_pos IS DISTINCT FROM pos OR w.position_codec<>'mysql-gtid/v1'
  OR w.position_lineage IS DISTINCT FROM p_receipt->>'position_lineage'
  OR w.source_epoch IS DISTINCT FROM (p_receipt->>'source_epoch')::integer
  OR w.consistency<>'transactional' OR w.consistency_grade<>'T' OR NOT w.cross_table_consistent
 THEN RAISE EXCEPTION 'C1 current source cut required' USING ERRCODE='OO030'; END IF;
 SELECT bt.* INTO b FROM _oo.native_c1_baseline_typed bt JOIN jobs.tasks j ON j.id=bt.task_id
  WHERE bt.source=v_source AND bt.acknowledged_at IS NOT NULL AND j.state='succeeded'
  AND (bt.receipt->>'source_generation')::bigint=source_gen
  AND (bt.receipt->>'target_generation')::bigint=target_gen
  AND (bt.receipt->>'source_epoch')::integer=w.source_epoch AND (bt.receipt->>'applied_pos')::numeric=pos;
 IF NOT FOUND OR jsonb_array_length(b.receipt->'entries')<>n THEN
  RAISE EXCEPTION 'complete acknowledged typed C1 cut required' USING ERRCODE='OO030'; END IF;
 IF w.state='live' THEN
  IF (SELECT count(*) FROM _oo.native_publication_receipt p JOIN _oo.full_import_run rr USING(run_id)
   WHERE p.source_id=v_source AND p.source_generation=source_gen AND p.target_generation=target_gen
   AND p.receipt_sha256=p_sha AND p.table_vector=p_receipt->'tables' AND rr.state='published'
   AND p_receipt->'run_ids' @> jsonb_build_array(p.run_id::text))<>n THEN
   RAISE EXCEPTION 'C1 publication replay conflict' USING ERRCODE='OO030'; END IF;
  RETURN jsonb_build_object('outcome','replay','run_id',p_anchor,'tables',n,'receipt_sha256',p_sha);
 END IF;
 IF w.state<>'lagging' THEN RAISE EXCEPTION 'C1 cut is not awaiting publication' USING ERRCODE='OO030'; END IF;
 FOR e IN SELECT value FROM jsonb_array_elements(p_receipt->'tables') LOOP
  IF NOT p_receipt->'run_ids' @> jsonb_build_array(e->>'run_id') OR NOT EXISTS(
   SELECT 1 FROM jsonb_array_elements(b.receipt->'entries') x WHERE x->>'run_id'=e->>'run_id'
    AND x->>'schema'=e->>'schema' AND x->>'table'=e->>'table' AND x->>'physical_id'=e->>'physical_name')
  THEN RAISE EXCEPTION 'C1 publication table identity' USING ERRCODE='OO030'; END IF;
  SELECT * INTO r FROM _oo.full_import_run WHERE run_id=(e->>'run_id')::uuid FOR UPDATE;
  IF NOT FOUND OR r.state<>'awaiting_publish' OR r.source_id<>v_source OR r.source_generation<>source_gen
   OR r.target_generation<>target_gen OR (e->>'source_generation')::bigint IS DISTINCT FROM source_gen
   OR (e->>'target_generation')::bigint IS DISTINCT FROM target_gen OR (e->>'applied_pos')::numeric IS DISTINCT FROM pos
  THEN RAISE EXCEPTION 'C1 publication run differs' USING ERRCODE='OO030'; END IF;
  SELECT * INTO t FROM _oo.full_import_table WHERE run_id=r.run_id AND schema_ident=e->>'schema' AND table_ident=e->>'table' FOR UPDATE;
  IF NOT FOUND OR NOT t.exhausted OR t.physical_name IS DISTINCT FROM e->>'physical_name'
   OR t.applied_rows IS DISTINCT FROM (e->>'accepted_rows')::bigint OR t.canonical_digest IS DISTINCT FROM e->>'canonical_digest'
   OR t.applied_pos<>pos OR (SELECT count(*) FROM _oo.full_import_table WHERE run_id=r.run_id)<>1
  THEN RAISE EXCEPTION 'C1 publication sealed table differs' USING ERRCODE='OO030'; END IF;
  SELECT coalesce(sum(accepted_rows),0),coalesce(sum(rejected_rows),0) INTO accepted,rejected
   FROM _oo.full_import_quality WHERE run_id=r.run_id AND physical_name=t.physical_name AND kind='batch';
  SELECT count(*) INTO count_rejects FROM _oo.full_import_quality WHERE run_id=r.run_id AND physical_name=t.physical_name AND kind='reject';
  IF accepted<>t.applied_rows OR rejected<>count_rejects OR jsonb_typeof(e->'rejects') IS DISTINCT FROM 'array'
   OR jsonb_array_length(e->'rejects')<>count_rejects THEN
   RAISE EXCEPTION 'C1 publication quality differs' USING ERRCODE='OO030'; END IF;
  FOR q IN SELECT * FROM _oo.full_import_quality WHERE run_id=r.run_id AND physical_name=t.physical_name AND kind='reject' LOOP
   IF q.reason<>'zero_date' OR q.key_digest IS NULL OR q.attnum IS NULL OR q.row_index IS NULL OR q.value_len IS NULL
    OR NOT EXISTS(SELECT 1 FROM pg_attribute a WHERE a.attrelid=to_regclass(format('_oo.%I',t.physical_name))
     AND a.attnum=q.attnum+1 AND a.atttypid IN(1082,1114,1184) AND NOT a.attnotnull)
    OR NOT EXISTS(SELECT 1 FROM jsonb_array_elements(e->'rejects') x WHERE x->>'reason'=q.reason
     AND (x->>'attnum')::integer=q.attnum AND (x->>'row_index')::bigint=q.row_index
     AND (x->>'value_len')::bigint=q.value_len AND x->>'key_digest'=q.key_digest)
   THEN RAISE EXCEPTION 'C1 zero-date quality review differs' USING ERRCODE='OO030'; END IF;
  END LOOP;
  IF NOT EXISTS(SELECT 1 FROM _oo.table_freshness f WHERE f.source=v_source AND f.source_generation=source_gen
   AND f.target_generation=target_gen AND f.schema_name=t.schema_ident AND f.table_name=t.table_ident
   AND f.physical_name=t.physical_name AND f.last_pos=pos AND f.mode='full' AND f.state='awaiting_publish')
  THEN RAISE EXCEPTION 'C1 freshness vector differs' USING ERRCODE='OO030'; END IF;
 END LOOP;
 FOR e IN SELECT value FROM jsonb_array_elements(p_receipt->'run_ids') LOOP
  INSERT INTO _oo.native_publication_receipt(run_id,receipt_sha256,source_id,source_generation,target_generation,
   position_codec,applied_pos,consistency,consistency_grade,cross_table_consistent,table_vector,publication_xid)
  VALUES((e#>>'{}')::uuid,p_sha,v_source,source_gen,target_gen,'mysql-gtid/v1',pos,'transactional','T',true,p_receipt->'tables',pg_current_xact_id());
  UPDATE _oo.full_import_run SET state='published' WHERE run_id=(e#>>'{}')::uuid AND state='awaiting_publish';
  IF NOT FOUND THEN RAISE EXCEPTION 'C1 publication raced' USING ERRCODE='OO030'; END IF;
 END LOOP;
 UPDATE _oo.table_freshness SET state='live',applied_at=clock_timestamp() WHERE source=v_source
  AND source_generation=source_gen AND target_generation=target_gen AND state='awaiting_publish';
 GET DIAGNOSTICS changed=ROW_COUNT;
 IF changed<>n THEN RAISE EXCEPTION 'C1 full source barrier differs' USING ERRCODE='OO030'; END IF;
 UPDATE _oo.source_watermark SET state='live',applied_at=clock_timestamp() WHERE source=v_source;
 RETURN jsonb_build_object('outcome','published','run_id',p_anchor,'source_id',v_source,'source_generation',source_gen,
  'target_generation',target_gen,'applied_pos',pos,'tables',n,'receipt_sha256',p_sha);
END
$publish$;
REVOKE ALL ON FUNCTION _oo.publish_native_c1_collection(uuid,text,jsonb) FROM PUBLIC,oo_pipeline_runtime;
DO $grant$
BEGIN
 IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_target_admin') THEN
  GRANT EXECUTE ON FUNCTION _oo.publish_native_c1_collection(uuid,text,jsonb) TO oo_target_admin;
 END IF;
END
$grant$;
