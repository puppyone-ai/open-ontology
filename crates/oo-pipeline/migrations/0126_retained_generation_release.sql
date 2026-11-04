-- Owner-side retention. Receipts remain as history; no CASCADE, source writes,
-- or grants to Gateway readers. Busy generations are retried on a later cycle.
ALTER TABLE _oo_load.gateway_publications
 ADD COLUMN IF NOT EXISTS released_at timestamptz;

CREATE OR REPLACE FUNCTION _oo_load.release_expired(p_library text,p_age_seconds bigint)
RETURNS jsonb LANGUAGE plpgsql SET search_path=pg_catalog AS $body$
DECLARE c record; g record; v jsonb; s record; n integer := 0; busy integer := 0;
BEGIN
 IF p_age_seconds < 0 OR p_age_seconds IS NULL OR p_library IS NULL THEN
  RAISE EXCEPTION 'invalid retention policy';
 END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('oo-load-library:'||p_library,0));
 -- Serialize the ready-head check with every activation, including other libraries.
 LOCK TABLE _oo.native_library_head IN SHARE MODE;
 FOR c IN
  SELECT candidate.* FROM _oo_load.gateway_publications candidate
  JOIN _oo.native_library_publication p USING(receipt_sha256)
  WHERE candidate.library_id=p_library AND candidate.released_at IS NULL
   AND candidate.activated_at IS NOT NULL
   AND p.published_at < clock_timestamp()-p_age_seconds*interval '1 second'
  ORDER BY candidate.generation
 LOOP
  BEGIN
   -- Protect any ready library using any part of this cut, not just its own head.
   IF EXISTS (
    SELECT 1 FROM _oo.native_library_head h
    JOIN _oo.native_library_publication publication USING(receipt_sha256),
     jsonb_array_elements(publication.shards) shard,
     jsonb_array_elements(shard->'receipt'->'tables') ready_table,
     _oo_load.gateway_publications live,
     jsonb_array_elements(live.data_vector) a, jsonb_array_elements(c.data_vector) b
    WHERE h.state='ready' AND ready_table->>'physical_name'=a->>'physical_name'
     AND a->>'storage_relation'=b->>'storage_relation'
   ) THEN CONTINUE; END IF;
   FOR g IN SELECT gen.* FROM _oo_load.generations gen,
     jsonb_array_elements(c.data_vector) e
     WHERE gen.relation_name=e->>'storage_relation' ORDER BY gen.identity
   LOOP
    IF NOT pg_try_advisory_xact_lock(hashtextextended(g.identity,0)) THEN
     RAISE lock_not_available;
    END IF;
    IF EXISTS(SELECT 1 FROM _oo_load.current x WHERE x.identity=g.identity AND x.run=g.run)
     OR EXISTS(SELECT 1 FROM _oo_load.c5_state x WHERE x.identity=g.identity AND x.run=g.run)
     OR EXISTS(SELECT 1 FROM _oo_load.sync_bindings x
               WHERE x.identity=g.identity AND x.baseline_run=g.run)
    THEN RAISE lock_not_available; END IF;
   END LOOP;
   -- Gateway pins these exact views before executing user SQL. NOWAIT ensures
   -- a held cut is skipped, rather than cancelled or stalled by reclamation.
   FOR v IN SELECT value FROM jsonb_array_elements(c.data_vector) LOOP
    EXECUTE format('LOCK TABLE _oo.%I IN ACCESS EXCLUSIVE MODE NOWAIT',v->>'physical_name');
   END LOOP;
   FOR v IN SELECT value FROM jsonb_array_elements(c.data_vector) LOOP
    EXECUTE format('DROP VIEW _oo.%I',v->>'physical_name');
   END LOOP;
   UPDATE _oo_load.gateway_publications SET released_at=clock_timestamp()
    WHERE generation=c.generation;
   FOR g IN SELECT gen.* FROM _oo_load.generations gen,
     jsonb_array_elements(c.data_vector) e WHERE gen.relation_name=e->>'storage_relation'
   LOOP
    IF EXISTS(SELECT 1 FROM _oo_load.gateway_publications other,
      jsonb_array_elements(other.data_vector) e
      WHERE other.released_at IS NULL AND e->>'storage_relation'=g.relation_name)
    THEN CONTINUE; END IF;
    EXECUTE format('LOCK TABLE _oo_load.%I IN ACCESS EXCLUSIVE MODE NOWAIT',g.relation_name);
    EXECUTE format('DROP VIEW _oo_load.%I',g.relation_name);
    DELETE FROM _oo_load.generations WHERE identity=g.identity AND run=g.run;
   END LOOP;
   n := n+1;
  EXCEPTION WHEN lock_not_available OR dependent_objects_still_exist THEN
   busy := busy+1;
  END;
 END LOOP;
 -- Whole stages may be shared by newer C5 recipes. PostgreSQL dependencies
 -- protect those compositions (and current aliases); never delete their rows.
 FOR s IN SELECT schemas.* FROM _oo_load.schemas schemas
  WHERE NOT EXISTS(SELECT 1 FROM _oo_load.generations remaining,
    jsonb_array_elements(remaining.recipe->'parts') part WHERE part->>'stage'=schemas.stage)
  AND EXISTS(SELECT 1 FROM _oo_load.gateway_publications released,
    jsonb_array_elements(released.relation_proofs) proof
    WHERE released.library_id=p_library AND released.released_at IS NOT NULL
      AND proof->>'schema'='_oo_load' AND proof->>'relation'=schemas.stage)
 LOOP
  BEGIN
   IF NOT pg_try_advisory_xact_lock(hashtextextended(s.identity,0)) THEN
    CONTINUE;
   END IF;
   EXECUTE format('LOCK TABLE _oo_load.%I IN ACCESS EXCLUSIVE MODE NOWAIT',s.stage);
   EXECUTE format('DROP TABLE _oo_load.%I',s.stage);
   DELETE FROM _oo_load.chunks WHERE stage=s.stage;
   DELETE FROM _oo_load.publications WHERE stage=s.stage;
   DELETE FROM _oo_load.schemas WHERE stage=s.stage;
  EXCEPTION WHEN lock_not_available OR dependent_objects_still_exist THEN NULL;
  END;
 END LOOP;
 RETURN jsonb_build_object('released',n,'busy',busy);
END $body$;
REVOKE ALL ON FUNCTION _oo_load.release_expired(text,bigint) FROM PUBLIC;
