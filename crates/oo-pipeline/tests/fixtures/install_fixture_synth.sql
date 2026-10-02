\set ON_ERROR_STOP on
SET ROLE oo_target_owner;

CREATE SCHEMA fixture_synth AUTHORIZATION oo_target_owner;
REVOKE ALL ON SCHEMA fixture_synth FROM PUBLIC;
GRANT USAGE ON SCHEMA fixture_synth TO oo_pipeline_runtime;

CREATE TABLE fixture_synth.applied_row (
  batch_id text PRIMARY KEY,
  payload_digest text NOT NULL,
  applied_pos numeric(38, 0) NOT NULL
);

CREATE TABLE fixture_synth.quality_row (
  batch_id text PRIMARY KEY,
  payload_digest text NOT NULL,
  row_count integer NOT NULL
);

REVOKE ALL ON fixture_synth.applied_row FROM PUBLIC;
REVOKE ALL ON fixture_synth.quality_row FROM PUBLIC;

COMMENT ON SCHEMA fixture_synth IS
  'Fixture-only bundle for one target transaction. Not a product schema and not a publication.';

CREATE FUNCTION fixture_synth.put_receipt(
  p_shard text,
  p_owner uuid,
  p_generation bigint,
  p_batch text,
  p_payload text,
  p_codec text,
  p_lineage text,
  p_pos numeric,
  p_partial boolean DEFAULT false,
  p_cross boolean DEFAULT true,
  p_complete boolean DEFAULT true,
  p_segment_index integer DEFAULT NULL,
  p_segment_count integer DEFAULT NULL,
  p_segment_bitmap bytea DEFAULT ''::bytea
)
RETURNS text
LANGUAGE sql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
  SELECT _oo.commit_apply_receipt(
    p_shard,
    p_owner,
    p_generation,
    'warehouse',
    'orders_g1',
    p_batch,
    repeat('a', 64),
    repeat('b', 64),
    repeat('c', 64),
    repeat('d', 64),
    p_payload,
    p_codec,
    p_lineage,
    1,
    p_pos,
    p_partial,
    p_cross,
    p_complete,
    p_segment_index,
    p_segment_count,
    p_segment_bitmap
  );
$$;

CREATE FUNCTION fixture_synth.commit_bundle(
  p_shard text,
  p_owner uuid,
  p_generation bigint,
  p_batch text,
  p_payload text
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, fixture_synth, _oo
AS $$
DECLARE
  v_existing text;
  v_result text;
BEGIN
  PERFORM _oo.assert_target_fence(p_shard, p_owner, p_generation);
  SELECT existing.payload_digest
    INTO v_existing
  FROM _oo.apply_receipts AS existing
  WHERE existing.source = 'warehouse'
    AND existing.table_generation = 'orders_g1'
    AND existing.txn_or_batch_id = p_batch;
  IF FOUND AND v_existing = p_payload THEN
    RETURN 'replay';
  END IF;
  IF FOUND THEN
    PERFORM fixture_synth.put_receipt(
      p_shard, p_owner, p_generation, p_batch, p_payload,
      'pg-commit-lsn/v1', 'src_main', 42
    );
  END IF;
  INSERT INTO fixture_synth.applied_row (batch_id, payload_digest, applied_pos)
  VALUES (p_batch, p_payload, 42);
  INSERT INTO fixture_synth.quality_row (batch_id, payload_digest, row_count)
  VALUES (p_batch, p_payload, 1);
  v_result := fixture_synth.put_receipt(
    p_shard, p_owner, p_generation, p_batch, p_payload,
    'pg-commit-lsn/v1', 'src_main', 42
  );
  RETURN v_result;
END;
$$;

CREATE FUNCTION fixture_synth.conflict_with_side_rows(
  p_shard text,
  p_owner uuid,
  p_generation bigint
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, fixture_synth, _oo
AS $$
BEGIN
  PERFORM _oo.assert_target_fence(p_shard, p_owner, p_generation);
  INSERT INTO fixture_synth.applied_row (batch_id, payload_digest, applied_pos)
  VALUES ('rollback_temp', repeat('f', 64), 7);
  INSERT INTO fixture_synth.quality_row (batch_id, payload_digest, row_count)
  VALUES ('rollback_temp', repeat('f', 64), 7);
  PERFORM fixture_synth.put_receipt(
    p_shard,
    p_owner,
    p_generation,
    'batch-0001',
    repeat('f', 64),
    'pg-commit-lsn/v1',
    'src_main',
    42
  );
END;
$$;

CREATE FUNCTION fixture_synth.own_lock_count(p_shard text)
RETURNS integer
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
  SELECT count(*)::integer
  FROM pg_catalog.pg_locks AS held
  WHERE held.locktype = 'advisory'
    AND held.pid = pg_catalog.pg_backend_pid()
    AND held.granted
    AND held.mode = 'ExclusiveLock'
    AND held.objsubid = 2
    AND held.classid = (SELECT mapped.key1 FROM _oo.lock_key(p_shard) AS mapped)::oid
    AND held.objid = (SELECT mapped.key2 FROM _oo.lock_key(p_shard) AS mapped)::oid;
$$;

CREATE FUNCTION fixture_synth.lock_key1(p_shard text)
RETURNS integer
LANGUAGE sql
IMMUTABLE
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
  SELECT mapped.key1 FROM _oo.lock_key(p_shard) AS mapped;
$$;

CREATE FUNCTION fixture_synth.shard_line(p_shard text)
RETURNS text
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
  SELECT held.generation::text || '|' || held.state || '|' || held.owner_token::text
  FROM _oo.target_generations AS held
  WHERE held.shard_id = p_shard;
$$;

CREATE FUNCTION fixture_synth.receipt_line(p_batch text)
RETURNS text
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
  SELECT existing.payload_digest
    || '|' || existing.position_codec
    || '|' || existing.position_lineage
    || '|' || existing.fence_generation::text
    || '|' || existing.target_commit_at::text
    || '|' || existing.applied_pos::text
    || '|' || existing.partial_txn::text
    || '|' || existing.txn_complete::text
    || '|' || existing.cross_table_consistent::text
  FROM _oo.apply_receipts AS existing
  WHERE existing.source = 'warehouse'
    AND existing.table_generation = 'orders_g1'
    AND existing.txn_or_batch_id = p_batch;
$$;

CREATE FUNCTION fixture_synth.counts_for(p_batch text)
RETURNS text
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, fixture_synth, _oo
AS $$
  SELECT
    (SELECT count(*) FROM fixture_synth.applied_row WHERE batch_id = p_batch)::text
    || '|' ||
    (SELECT count(*) FROM fixture_synth.quality_row WHERE batch_id = p_batch)::text
    || '|' ||
    (SELECT count(*) FROM _oo.apply_receipts
      WHERE source = 'warehouse'
        AND table_generation = 'orders_g1'
        AND txn_or_batch_id = p_batch)::text;
$$;

CREATE FUNCTION fixture_synth.adopted_year(p_shard text)
RETURNS integer
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
  SELECT extract(year FROM held.adopted_at)::integer
  FROM _oo.target_generations AS held
  WHERE held.shard_id = p_shard;
$$;

CREATE FUNCTION fixture_synth.force_generation(p_shard text, p_generation bigint)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
BEGIN
  PERFORM _oo._allow_write();
  UPDATE _oo.target_generations
  SET generation = p_generation
  WHERE shard_id = p_shard;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'missing shard' USING ERRCODE = 'P0002';
  END IF;
END;
$$;

CREATE FUNCTION fixture_synth.force_lock_key(p_shard text, p_key1 integer, p_key2 integer)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
BEGIN
  PERFORM _oo._allow_write();
  UPDATE _oo.target_generations
  SET lock_key1 = p_key1, lock_key2 = p_key2
  WHERE shard_id = p_shard;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'missing shard' USING ERRCODE = 'P0002';
  END IF;
END;
$$;

COMMENT ON FUNCTION fixture_synth.force_generation(text, bigint) IS
  'Test hook. Not granted to the runtime. Dropped before the fixture finishes. The product API cannot jump a generation.';

REVOKE ALL ON ALL FUNCTIONS IN SCHEMA fixture_synth FROM PUBLIC;
GRANT EXECUTE ON FUNCTION fixture_synth.put_receipt(
  text, uuid, bigint, text, text, text, text, numeric, boolean, boolean, boolean, integer, integer, bytea
) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION fixture_synth.commit_bundle(text, uuid, bigint, text, text) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION fixture_synth.conflict_with_side_rows(text, uuid, bigint) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION fixture_synth.own_lock_count(text) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION fixture_synth.lock_key1(text) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION fixture_synth.shard_line(text) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION fixture_synth.receipt_line(text) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION fixture_synth.counts_for(text) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION fixture_synth.adopted_year(text) TO oo_pipeline_runtime;

RESET ROLE;
