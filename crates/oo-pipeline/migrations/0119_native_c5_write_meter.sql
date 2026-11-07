-- Logical tuple bytes, not WAL, index, TOAST or filesystem allocation bytes.
-- Receipt triggers run in the same transaction as the measured writes.
CREATE TABLE _oo.c5_write_meter (
 source text NOT NULL, schema_name text NOT NULL, table_name text NOT NULL,
 source_generation bigint NOT NULL, chunk_id text NOT NULL,
 phase text NOT NULL CHECK(phase IN ('inbox','stage','delta')),
 ordinal bigint NOT NULL, receipt_id uuid NOT NULL, task uuid, run_id uuid,
 rows bigint NOT NULL CHECK(rows>=0), bytes bigint NOT NULL CHECK(bytes>=0),
 recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(source,receipt_id,phase,ordinal)
);
REVOKE ALL ON _oo.c5_write_meter FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_meter_part() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE c _oo.c5_parts_capture; n bigint; size bigint; phase text; relation text;
BEGIN
 SELECT * INTO STRICT c FROM _oo.c5_parts_capture WHERE source=NEW.source AND task=NEW.task;
 FOREACH phase IN ARRAY ARRAY['inbox','stage'] LOOP
  relation:=CASE phase WHEN 'inbox' THEN c.inbox ELSE c.physical END;
  EXECUTE format('SELECT count(*),coalesce(sum(pg_column_size(r)),0)::bigint FROM _oo.%I r%s',
    relation,CASE phase WHEN 'stage' THEN ' WHERE part=$1' ELSE '' END)
    INTO n,size USING NEW.ordinal;
  IF n<>NEW.rows THEN RAISE EXCEPTION 'C5 meter part rows differ'; END IF;
  INSERT INTO _oo.c5_write_meter(source,schema_name,table_name,source_generation,chunk_id,
    phase,ordinal,receipt_id,task,rows,bytes)
   VALUES(NEW.source,c.header#>>'{range,schema}',c.header#>>'{range,table}',
    (c.header#>>'{range,source_generation}')::bigint,c.header#>>'{range,chunk_id}',
    phase,NEW.ordinal,NEW.task,NEW.task,n,size);
 END LOOP;
 RETURN NEW;
END $$;

CREATE FUNCTION _oo.c5_meter_delta() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE o _oo.c5_overlay; n bigint; size bigint;
BEGIN
 SELECT * INTO o FROM _oo.c5_overlay WHERE run_id=NEW.run_id;
 IF NOT FOUND THEN RETURN NEW; END IF;
 IF to_regclass(format('_oo.%I',o.delta_name)) IS DISTINCT FROM o.delta_oid
 THEN RAISE EXCEPTION 'C5 meter delta relation differs'; END IF;
 -- Scan only the new chunk heap, never its composed view or immutable parent.
 EXECUTE format('SELECT count(*),coalesce(sum(pg_column_size(r)),0)::bigint FROM _oo.%I r',
   o.delta_name) INTO n,size;
 IF n IS DISTINCT FROM o.delta_rows THEN RAISE EXCEPTION 'C5 meter delta rows differ'; END IF;
 INSERT INTO _oo.c5_write_meter(source,schema_name,table_name,source_generation,chunk_id,
   phase,ordinal,receipt_id,task,run_id,rows,bytes)
  VALUES(NEW.source,NEW.schema_name,NEW.table_name,NEW.source_generation,NEW.chunk_id,
   'delta',-1,NEW.run_id,(NEW.scope->>'parts_task')::uuid,NEW.run_id,n,size);
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_meter_part(),_oo.c5_meter_delta() FROM PUBLIC,oo_pipeline_runtime;
CREATE TRIGGER c5_meter_part AFTER INSERT ON _oo.c5_parts_receipt
 FOR EACH ROW EXECUTE FUNCTION _oo.c5_meter_part();
CREATE TRIGGER c5_meter_delta AFTER INSERT ON _oo.c5_block_receipt
 FOR EACH ROW EXECUTE FUNCTION _oo.c5_meter_delta();

COMMENT ON TABLE _oo.c5_write_meter IS
 'Committed logical tuple bytes by task/generation. Sum phases for a cycle; replay and rollback add zero. Excludes WAL/index/TOAST/metadata overhead; historical writes are not backfilled.';
