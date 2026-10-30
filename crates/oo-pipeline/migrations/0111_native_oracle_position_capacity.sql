-- Complete Oracle transactions and capture ordinals have no business-count cap.
-- Canonical finite positions, full END/order evidence, leases and replay guards remain.
CREATE OR REPLACE FUNCTION _oo.native_position_closed(
  p_codec text,
  p_lineage text,
  p_pos numeric,
  p_epoch integer
)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
SET search_path = pg_catalog
AS $$
  SELECT CASE
    WHEN p_codec = 'oracle-capture-sequence/v1' THEN
      p_lineage IS NOT NULL AND p_lineage ~ '^oracle\.[1-9][0-9]{0,19}\.(0|[1-9][0-9]{0,19})\.[0-9a-f]{32}$'
      AND p_pos IS NOT NULL AND p_pos >= 0 AND p_pos = trunc(p_pos)
      AND _oo.native_pos_text(p_pos) ~ '^(0|[1-9][0-9]{0,37})$'
      AND (p_epoch IS NULL OR p_epoch >= 0)
    WHEN p_codec IN (
      'pg-commit-lsn/v1',
      'mysql-binlog-pos/v1',
      'batch-cursor-us/v1',
      'snapshot-zero/v1',
      'partition-signature/v1',
      'mssql-ct-version/v1'
    ) THEN true
    WHEN p_codec = 'mariadb-gtid/v1'
      AND p_lineage ~ '^d(0|[1-9][0-9]{0,9})\.s([1-9][0-9]{0,9})$'
      AND substring(p_lineage from '^d([0-9]+)')::numeric <= 4294967295
      AND substring(p_lineage from '\.s([0-9]+)$')::numeric BETWEEN 1 AND 4294967295
      AND p_pos IS NOT NULL
      AND _oo.native_pos_text(p_pos) ~ '^(0|[1-9][0-9]{0,19})$'
      AND (p_epoch IS NULL OR p_epoch >= 0)
    THEN true
    WHEN p_codec = 'mysql-gtid/v1'
      AND p_lineage ~ '^mysql\.[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
      AND p_pos IS NOT NULL
      AND _oo.native_pos_text(p_pos) ~ '^[1-9][0-9]{0,19}$'
      AND (p_epoch IS NULL OR p_epoch >= 0)
    THEN true
    WHEN p_codec = 'mongo-cluster-time/v1'
      AND p_lineage ~ '^[a-z][a-z0-9_.:-]{0,127}$'
      AND position(',' in p_lineage) = 0
      AND p_pos IS NOT NULL
      AND _oo.native_pos_text(p_pos) ~ '^[1-9][0-9]{0,37}$'
      AND (p_epoch IS NULL OR p_epoch >= 0)
    THEN true
    ELSE false
  END;
$$;

CREATE OR REPLACE FUNCTION _oo.c1_typed_record_oracle_position(s text,t uuid,g bigint,e integer,p_seq bigint,p_digest text,p jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE; r _oo.c1_typed_receipt%ROWTYPE;
  old _oo.c1_typed_source_position%ROWTYPE; prev _oo.c1_typed_source_position%ROWTYPE;
  n bigint; scn numeric; before_sha text; after_sha text;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  v:=_oo.native_c1_baseline_v2_assert(s,t,g,e);
  IF v.engine<>'oracle' OR p->>'protocol' IS DISTINCT FROM 'oo.c1.oracle-logminer-position/v1'
    OR p->>'engine' IS DISTINCT FROM 'oracle' OR p->>'transaction_id' IS NULL
    OR p->>'transaction_id' !~ '^[A-Za-z0-9_$#-]{1,128}$'
    OR p->>'commit_scn' IS NULL OR p->>'commit_scn' !~ '^[1-9][0-9]{0,37}$'
    OR p->>'events_sha256' IS NULL OR p->>'events_sha256' !~ '^[0-9a-f]{64}$' THEN
    RAISE EXCEPTION 'Oracle complete transaction required' USING ERRCODE='OO030';
  END IF;
  n:=(p->>'event_count')::bigint;scn:=(p->>'commit_scn')::numeric;
  before_sha:=p#>>'{offset_before,sha256}';after_sha:=p#>>'{offset_after,sha256}';
  IF n IS NULL OR n<1 OR (p->>'end_event_count')::bigint IS DISTINCT FROM n
    OR (p->>'event_order_first')::bigint IS DISTINCT FROM 1 OR (p->>'event_order_last')::bigint IS DISTINCT FROM n
    OR before_sha IS NULL OR before_sha !~ '^[0-9a-f]{64}$' OR after_sha IS NULL OR after_sha !~ '^[0-9a-f]{64}$'
    OR before_sha=after_sha THEN RAISE EXCEPTION 'Oracle opaque checkpoint required' USING ERRCODE='OO030'; END IF;
  SELECT * INTO r FROM _oo.c1_typed_receipt WHERE source=s AND task=t AND seq=p_seq AND digest=p_digest AND kind='cdc';
  IF NOT FOUND THEN RAISE EXCEPTION 'typed target receipt required' USING ERRCODE='OO030'; END IF;
  SELECT * INTO old FROM _oo.c1_typed_source_position WHERE source=s AND task=t AND seq=p_seq;
  IF FOUND THEN
    IF old.evidence<>p THEN RAISE EXCEPTION 'source checkpoint replay conflict' USING ERRCODE='OO030'; END IF;
    RETURN;
  END IF;
  SELECT * INTO prev FROM _oo.c1_typed_source_position WHERE source=s AND task=t ORDER BY seq DESC LIMIT 1;
  IF before_sha<>COALESCE(prev.opaque_after,v.l0) OR (prev.commit_scn IS NOT NULL AND scn<prev.commit_scn) THEN
    RAISE EXCEPTION 'opaque checkpoint chain or source position regression' USING ERRCODE='OO030';
  END IF;
  INSERT INTO _oo.c1_typed_source_position VALUES(s,t,p_seq,r.position,'oracle',p->>'transaction_id',scn,before_sha,after_sha,p);
END $fn$;
