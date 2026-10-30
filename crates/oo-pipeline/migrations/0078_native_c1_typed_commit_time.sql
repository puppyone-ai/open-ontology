-- Typed transactions retain their real binlog END time in the existing C1
-- ledger. A private target receipt is required; no current-clock substitution.
CREATE OR REPLACE FUNCTION _oo.native_c1_baseline_v2_guard_txn()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
BEGIN
  IF EXISTS(SELECT 1 FROM _oo.native_c1_baseline_v2_format WHERE source=NEW.source AND task_id=NEW.task_id)
    AND NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_batch
      WHERE source=NEW.source AND task_id=NEW.task_id AND NEW.applied_pos BETWEEN first_pos AND last_pos)
    AND NOT EXISTS(SELECT 1 FROM _oo.c1_typed_receipt
      WHERE source=NEW.source AND task=NEW.task_id AND kind='cdc'
      AND position=NEW.applied_pos AND digest=NEW.payload_sha256) THEN
    RAISE EXCEPTION 'format_mismatch' USING ERRCODE='OO030';
  END IF;
  RETURN NEW;
END $fn$;

CREATE FUNCTION _oo.c1_typed_commit_time(s text,t uuid,g bigint,e integer,p_seq bigint,p_digest text,p_stamp text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE; r _oo.c1_typed_receipt%ROWTYPE;
  prior _oo.native_c1_baseline_txn%ROWTYPE; old_time timestamptz; stamp timestamptz;
  txn text;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  v:=_oo.native_c1_baseline_v2_assert(s,t,g,e);
  IF p_stamp IS NULL OR p_stamp !~ '^20[0-9]{2}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]{1,6})?Z$' THEN
    RAISE EXCEPTION 'actual binlog END timestamp required' USING ERRCODE='OO030';
  END IF;
  stamp:=p_stamp::timestamptz;
  SELECT * INTO r FROM _oo.c1_typed_receipt WHERE source=s AND task=t AND seq=p_seq AND digest=p_digest AND kind='cdc';
  IF NOT FOUND THEN RAISE EXCEPTION 'typed commit receipt required' USING ERRCODE='OO030'; END IF;
  txn:=substring(v.measured_id from 7)||':'||_oo.native_pos_text(r.position);
  SELECT * INTO prior FROM _oo.native_c1_baseline_txn WHERE source=s AND task_id=t AND applied_pos=r.position;
  IF FOUND AND (prior.txn_id<>txn OR prior.payload_sha256<>p_digest) THEN
    RAISE EXCEPTION 'typed commit replay conflict' USING ERRCODE='OO030';
  END IF;
  SELECT source_committed_at INTO old_time FROM _oo.native_c1_baseline_txn_time
    WHERE source=s AND task_id=t AND applied_pos=r.position;
  IF FOUND AND old_time<>stamp THEN RAISE EXCEPTION 'typed commit time conflict' USING ERRCODE='OO030'; END IF;
  INSERT INTO _oo.native_c1_baseline_txn(source,task_id,applied_pos,txn_id,payload_sha256)
    VALUES(s,t,r.position,txn,p_digest) ON CONFLICT DO NOTHING;
  INSERT INTO _oo.native_c1_baseline_txn_time VALUES(s,t,r.position,stamp) ON CONFLICT DO NOTHING;
END $fn$;
REVOKE ALL ON FUNCTION _oo.c1_typed_commit_time(text,uuid,bigint,integer,bigint,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c1_typed_commit_time(text,uuid,bigint,integer,bigint,text,text) TO oo_pipeline_runtime;
