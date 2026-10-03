BEGIN;
SET LOCAL lock_timeout = '15s';
SELECT pg_catalog.pg_advisory_xact_lock(6404202605);
ALTER ROLE oo_target_owner SET search_path = pg_catalog;
ALTER ROLE oo_pipeline_runtime SET search_path = pg_catalog;

DO $$
DECLARE
  target_db text := current_database();
  advisory regprocedure;
BEGIN
  EXECUTE format('REVOKE ALL ON DATABASE %I FROM PUBLIC', target_db);
  EXECUTE format(
    'GRANT CONNECT, CREATE, TEMP ON DATABASE %I TO oo_target_owner',
    target_db
  );
  EXECUTE format('GRANT CONNECT ON DATABASE %I TO oo_pipeline_runtime', target_db);
  EXECUTE format('REVOKE CREATE, TEMP ON DATABASE %I FROM oo_pipeline_runtime', target_db);
  EXECUTE format('REVOKE TEMP ON DATABASE %I FROM PUBLIC', target_db);
  EXECUTE format('REVOKE CREATE ON DATABASE %I FROM PUBLIC', target_db);

  -- Installer closure. The schema owner is not a superuser and cannot revoke
  -- these catalog grants itself. Runtime is not on the grant list.
  -- pg_try_advisory_lock does not match the pg_advisory prefix. A later GRANT
  -- on a still-default ACL would keep PUBLIC execute, so revoke both names
  -- before granting the owner the three functions the fence uses.
  FOR advisory IN
    SELECT pg_proc.oid::regprocedure
    FROM pg_proc
    JOIN pg_namespace ON pg_namespace.oid = pg_proc.pronamespace
    WHERE pg_namespace.nspname = 'pg_catalog'
      AND (
        pg_proc.proname LIKE 'pg\_advisory%' ESCAPE '\'
        OR pg_proc.proname LIKE 'pg\_try\_advisory%' ESCAPE '\'
      )
  LOOP
    EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC', advisory);
  END LOOP;

  EXECUTE 'REVOKE ALL ON FUNCTION pg_catalog.set_config(text, text, boolean) FROM PUBLIC';
  EXECUTE 'GRANT EXECUTE ON FUNCTION pg_catalog.set_config(text, text, boolean) TO oo_target_owner';
  EXECUTE 'GRANT EXECUTE ON FUNCTION pg_catalog.pg_try_advisory_lock(integer, integer) TO oo_target_owner';
  EXECUTE 'GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_unlock(integer, integer) TO oo_target_owner';
  EXECUTE 'GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_xact_lock(bigint) TO oo_target_owner';
END
$$;

REVOKE CREATE ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM oo_pipeline_runtime;
GRANT USAGE ON SCHEMA public TO oo_pipeline_runtime;

SET ROLE oo_target_owner;
-- target-fence-receipts/1 phase expand
-- Run as oo_target_owner after target-bootstrap/1. This file does not create
-- roles, install extensions, or write business tables. It does not store
-- reject events, control-database mirrors, or stream acknowledgements.
-- A receipt write proves this backend holds the target advisory lock.
-- It does not fence arbitrary SQL issued by a role that can write data.

CREATE SCHEMA _oo AUTHORIZATION oo_target_owner;

REVOKE ALL ON SCHEMA _oo FROM PUBLIC;
GRANT USAGE ON SCHEMA _oo TO oo_pipeline_runtime;

ALTER DEFAULT PRIVILEGES FOR ROLE oo_target_owner IN SCHEMA _oo
  REVOKE ALL ON TABLES FROM PUBLIC;
ALTER DEFAULT PRIVILEGES FOR ROLE oo_target_owner IN SCHEMA _oo
  REVOKE ALL ON SEQUENCES FROM PUBLIC;
ALTER DEFAULT PRIVILEGES FOR ROLE oo_target_owner
  REVOKE ALL ON FUNCTIONS FROM PUBLIC;
ALTER DEFAULT PRIVILEGES FOR ROLE oo_target_owner
  REVOKE ALL ON ROUTINES FROM PUBLIC;

CREATE TABLE _oo.schema_migrations (
  version text PRIMARY KEY,
  checksum text NOT NULL,
  phase text NOT NULL,
  filename text NOT NULL,
  applied_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  applied_by text NOT NULL DEFAULT current_user,
  CONSTRAINT schema_migrations_version_check CHECK (octet_length(version) BETWEEN 1 AND 128),
  CONSTRAINT schema_migrations_checksum_check CHECK (checksum ~ '^[0-9a-f]{64}$'),
  CONSTRAINT schema_migrations_phase_check CHECK (phase IN ('expand', 'migrate', 'contract')),
  CONSTRAINT schema_migrations_filename_check CHECK (octet_length(filename) BETWEEN 1 AND 256)
);

CREATE TABLE _oo.migration_lock (
  id integer PRIMARY KEY,
  CONSTRAINT migration_lock_singleton_check CHECK (id = 1)
);

INSERT INTO _oo.migration_lock (id) VALUES (1);

COMMENT ON TABLE _oo.migration_lock IS
  'Single-executor migration lock row. Applicators take pg_advisory_xact_lock(6404202605) and then lock this row FOR UPDATE. The advisory key is not a shard fence.';

CREATE TABLE _oo.target_generations (
  shard_id text PRIMARY KEY,
  generation bigint NOT NULL,
  state text NOT NULL,
  owner_token uuid NOT NULL,
  session_nonce uuid NOT NULL,
  lock_key1 integer NOT NULL,
  lock_key2 integer NOT NULL,
  backend_pid integer NOT NULL,
  adopted_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  renewed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  failed_at timestamptz,
  CONSTRAINT target_generations_shard_check CHECK (shard_id ~ '^[a-z][a-z0-9_.:-]{0,127}$'),
  CONSTRAINT target_generations_generation_check CHECK (generation >= 1),
  CONSTRAINT target_generations_state_check CHECK (state IN ('adopted', 'failed')),
  CONSTRAINT target_generations_lock_key_unique UNIQUE (lock_key1, lock_key2),
  CONSTRAINT target_generations_failed_at_check CHECK (
    (state = 'failed' AND failed_at IS NOT NULL)
    OR (state = 'adopted' AND failed_at IS NULL)
  )
);

COMMENT ON TABLE _oo.target_generations IS
  'LOGGED shard fence. generation increases on adoption after the session lock was not already held. A control-database lease is not a row in this table.';

CREATE TABLE _oo.apply_receipts (
  source text NOT NULL,
  table_generation text NOT NULL,
  txn_or_batch_id text NOT NULL,
  config_version text NOT NULL,
  config_digest text NOT NULL,
  type_policy_version text NOT NULL,
  type_digest text NOT NULL,
  hook_digest text NOT NULL,
  schema_digest text NOT NULL,
  payload_digest text NOT NULL,
  protocol_version text NOT NULL,
  position_codec text NOT NULL,
  position_lineage text NOT NULL,
  source_epoch integer NOT NULL,
  applied_pos numeric(38, 0) NOT NULL,
  partial_txn boolean NOT NULL,
  cross_table_consistent boolean NOT NULL,
  txn_complete boolean NOT NULL,
  segment_index integer,
  segment_count integer,
  segment_bitmap bytea NOT NULL,
  fence_shard text NOT NULL,
  fence_generation bigint NOT NULL,
  target_commit_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (source, table_generation, txn_or_batch_id),
  CONSTRAINT apply_receipts_source_check CHECK (source ~ '^[a-z][a-z0-9_]{0,62}$'),
  CONSTRAINT apply_receipts_table_generation_check CHECK (
    table_generation ~ '^[a-z][a-z0-9_]{0,62}$'
  ),
  CONSTRAINT apply_receipts_batch_check CHECK (
    txn_or_batch_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
  ),
  CONSTRAINT apply_receipts_config_version_check CHECK (config_version = 'config/v1'),
  CONSTRAINT apply_receipts_type_version_check CHECK (type_policy_version = 'type-policy/v1'),
  CONSTRAINT apply_receipts_protocol_check CHECK (protocol_version = 'batch/v1'),
  CONSTRAINT apply_receipts_config_digest_check CHECK (config_digest ~ '^[0-9a-f]{64}$'),
  CONSTRAINT apply_receipts_type_digest_check CHECK (type_digest ~ '^[0-9a-f]{64}$'),
  CONSTRAINT apply_receipts_hook_digest_check CHECK (hook_digest ~ '^[0-9a-f]{64}$'),
  CONSTRAINT apply_receipts_schema_digest_check CHECK (schema_digest ~ '^[0-9a-f]{64}$'),
  CONSTRAINT apply_receipts_payload_digest_check CHECK (payload_digest ~ '^[0-9a-f]{64}$'),
  CONSTRAINT apply_receipts_codec_check CHECK (
    position_codec IN (
      'pg-commit-lsn/v1',
      'mysql-binlog-pos/v1',
      'batch-cursor-us/v1',
      'snapshot-zero/v1'
    )
  ),
  CONSTRAINT apply_receipts_lineage_check CHECK (
    position_lineage ~ '^[a-z][a-z0-9_.:-]{0,127}$'
  ),
  CONSTRAINT apply_receipts_epoch_check CHECK (source_epoch >= 0),
  CONSTRAINT apply_receipts_pos_check CHECK (applied_pos >= 0),
  CONSTRAINT apply_receipts_completion_check CHECK (
    (
      partial_txn = true
      AND txn_complete = false
      AND cross_table_consistent = false
    )
    OR (
      partial_txn = false
      AND txn_complete = true
      AND cross_table_consistent = true
    )
    OR (
      partial_txn = false
      AND txn_complete = true
      AND cross_table_consistent = false
    )
  ),
  CONSTRAINT apply_receipts_segment_check CHECK (
    (
      segment_count IS NULL
      AND segment_index IS NULL
      AND octet_length(segment_bitmap) = 0
    )
    OR (
      segment_count >= 1
      AND segment_index >= 0
      AND segment_index < segment_count
      AND octet_length(segment_bitmap) BETWEEN 1 AND 8192
    )
  ),
  CONSTRAINT apply_receipts_fence_shard_check CHECK (
    fence_shard ~ '^[a-z][a-z0-9_.:-]{0,127}$'
  ),
  CONSTRAINT apply_receipts_fence_generation_check CHECK (fence_generation >= 1)
);

COMMENT ON TABLE _oo.apply_receipts IS
  'LOGGED target receipt. Identity is source, table_generation, txn_or_batch_id. Reject events are not stored. applied_pos is not comparable across position_codec or position_lineage.';

COMMENT ON COLUMN _oo.apply_receipts.table_generation IS
  'Logical table-generation token. It is not a PostgreSQL identifier. pgident/v1 q_ hex names can exceed 63 bytes and are not used as DDL here.';

CREATE FUNCTION _oo._backend_holds_lock(p_key1 integer, p_key2 integer)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
  SELECT EXISTS (
    SELECT 1
    FROM pg_catalog.pg_locks AS held
    WHERE held.locktype = 'advisory'
      AND held.pid = pg_catalog.pg_backend_pid()
      AND held.granted
      AND held.mode = 'ExclusiveLock'
      AND held.objsubid = 2
      AND held.classid = p_key1::oid
      AND held.objid = p_key2::oid
      AND held.database = (
        SELECT dat.oid
        FROM pg_catalog.pg_database AS dat
        WHERE dat.datname = pg_catalog.current_database()
      )
  );
$$;

CREATE FUNCTION _oo.lock_key(p_shard text, OUT key1 integer, OUT key2 integer)
LANGUAGE sql
IMMUTABLE
SECURITY INVOKER
SET search_path = pg_catalog
AS $$
  SELECT
    (('x' || substr(pg_catalog.md5('target-fence-lock/v1|' || p_shard), 1, 8))::bit(32)::integer),
    (('x' || substr(pg_catalog.md5('target-fence-lock/v1|' || p_shard), 9, 8))::bit(32)::integer);
$$;

COMMENT ON FUNCTION _oo.lock_key(text) IS
  'Immutable lock-key/v1 mapping. MD5 selects two int4 advisory keys. It is not a password hash and it is not a PostgreSQL identifier.';

CREATE FUNCTION _oo._allow_write()
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
BEGIN
  PERFORM pg_catalog.set_config('_oo.writer', 'apply-receipt', true);
END;
$$;

CREATE FUNCTION _oo._bind_session(p_shard text, p_nonce uuid)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
BEGIN
  CREATE TEMP TABLE IF NOT EXISTS oo_target_fence_session (
    shard_id text PRIMARY KEY,
    session_nonce uuid NOT NULL
  ) ON COMMIT PRESERVE ROWS;
  INSERT INTO pg_temp.oo_target_fence_session AS session_row (shard_id, session_nonce)
  VALUES (p_shard, p_nonce)
  ON CONFLICT (shard_id) DO UPDATE
    SET session_nonce = EXCLUDED.session_nonce;
END;
$$;

CREATE FUNCTION _oo._session_nonce(p_shard text)
RETURNS uuid
LANGUAGE plpgsql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
DECLARE
  v_nonce uuid;
BEGIN
  SELECT session_row.session_nonce
    INTO v_nonce
  FROM pg_temp.oo_target_fence_session AS session_row
  WHERE session_row.shard_id = p_shard;
  RETURN v_nonce;
EXCEPTION
  WHEN undefined_table THEN
    RETURN NULL;
END;
$$;

CREATE FUNCTION _oo._require_shard(p_shard text)
RETURNS void
LANGUAGE plpgsql
IMMUTABLE
SECURITY INVOKER
SET search_path = pg_catalog
AS $$
BEGIN
  IF p_shard IS NULL OR p_shard !~ '^[a-z][a-z0-9_.:-]{0,127}$' THEN
    RAISE EXCEPTION 'target shard id is closed' USING ERRCODE = '23514';
  END IF;
END;
$$;

CREATE FUNCTION _oo._require_positive_generation(p_generation bigint)
RETURNS void
LANGUAGE plpgsql
IMMUTABLE
SECURITY INVOKER
SET search_path = pg_catalog
AS $$
BEGIN
  IF p_generation IS NULL OR p_generation <= 0 THEN
    RAISE EXCEPTION 'target generation must be positive' USING ERRCODE = '22003';
  END IF;
END;
$$;

CREATE FUNCTION _oo.adopt_target_generation(p_shard text, p_owner uuid)
RETURNS bigint
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
DECLARE
  v_key1 integer;
  v_key2 integer;
  v_already boolean;
  v_generation bigint;
  v_state text;
  v_owner uuid;
  v_nonce uuid;
  v_stored_nonce uuid;
  v_pid integer;
  v_stored_key1 integer;
  v_stored_key2 integer;
BEGIN
  PERFORM _oo._require_shard(p_shard);
  IF p_owner IS NULL THEN
    RAISE EXCEPTION 'owner token is required' USING ERRCODE = '23514';
  END IF;
  SELECT mapped.key1, mapped.key2
    INTO v_key1, v_key2
  FROM _oo.lock_key(p_shard) AS mapped;
  v_already := _oo._backend_holds_lock(v_key1, v_key2);
  BEGIN
    IF NOT v_already THEN
      IF NOT pg_catalog.pg_try_advisory_lock(v_key1, v_key2) THEN
        RAISE EXCEPTION 'target shard lock is held by another backend' USING ERRCODE = '55P03';
      END IF;
    END IF;
    IF EXISTS (
      SELECT 1
      FROM _oo.target_generations AS other
      WHERE other.lock_key1 = v_key1
        AND other.lock_key2 = v_key2
        AND other.shard_id <> p_shard
    ) THEN
      RAISE EXCEPTION 'target lock key collision' USING ERRCODE = 'OO020';
    END IF;
    PERFORM _oo._allow_write();
    SELECT
      held.generation,
      held.state,
      held.owner_token,
      held.session_nonce,
      held.backend_pid,
      held.lock_key1,
      held.lock_key2
      INTO
        v_generation,
        v_state,
        v_owner,
        v_stored_nonce,
        v_pid,
        v_stored_key1,
        v_stored_key2
    FROM _oo.target_generations AS held
    WHERE held.shard_id = p_shard
    FOR UPDATE;
    IF FOUND AND (v_stored_key1 IS DISTINCT FROM v_key1 OR v_stored_key2 IS DISTINCT FROM v_key2) THEN
      RAISE EXCEPTION 'target lock key mapping changed' USING ERRCODE = 'OO019';
    END IF;
    v_nonce := _oo._session_nonce(p_shard);
    IF FOUND
       AND v_already
       AND v_state = 'adopted'
       AND v_owner = p_owner
       AND v_pid = pg_catalog.pg_backend_pid()
       AND v_nonce IS NOT NULL
       AND v_nonce = v_stored_nonce
    THEN
      UPDATE _oo.target_generations
      SET renewed_at = clock_timestamp()
      WHERE shard_id = p_shard;
      RETURN v_generation;
    END IF;
    IF FOUND AND v_already THEN
      RAISE EXCEPTION 'this backend already holds the shard' USING ERRCODE = 'OO017';
    END IF;
    IF NOT FOUND THEN
      v_nonce := pg_catalog.gen_random_uuid();
      INSERT INTO _oo.target_generations (
        shard_id, generation, state, owner_token, session_nonce,
        lock_key1, lock_key2, backend_pid, adopted_at, renewed_at, failed_at
      ) VALUES (
        p_shard, 1, 'adopted', p_owner, v_nonce,
        v_key1, v_key2, pg_catalog.pg_backend_pid(),
        clock_timestamp(), clock_timestamp(), NULL
      );
      PERFORM _oo._bind_session(p_shard, v_nonce);
      RETURN 1;
    END IF;
    IF v_generation >= 9223372036854775807 THEN
      RAISE EXCEPTION 'target generation overflow' USING ERRCODE = '22003';
    END IF;
    v_nonce := pg_catalog.gen_random_uuid();
    UPDATE _oo.target_generations
    SET
      generation = generation + 1,
      state = 'adopted',
      owner_token = p_owner,
      session_nonce = v_nonce,
      backend_pid = pg_catalog.pg_backend_pid(),
      adopted_at = clock_timestamp(),
      renewed_at = clock_timestamp(),
      failed_at = NULL
    WHERE shard_id = p_shard
    RETURNING generation INTO v_generation;
    PERFORM _oo._bind_session(p_shard, v_nonce);
    RETURN v_generation;
  EXCEPTION
    WHEN OTHERS THEN
      IF NOT v_already THEN
        PERFORM pg_catalog.pg_advisory_unlock(v_key1, v_key2);
      END IF;
      RAISE;
  END;
END;
$$;

COMMENT ON FUNCTION _oo.adopt_target_generation(text, uuid) IS
  'Takes this backend''s session advisory lock and adopts or renews the shard generation. Renewal keeps the generation only when this same backend already holds the lock, the session nonce, and the owner token. Any other call from a backend that already holds the lock raises OO017 and does not increment. A new acquisition increments the generation. Knowing the owner token is not adoption.';

CREATE FUNCTION _oo._load_held_generation(
  p_shard text,
  p_owner uuid,
  p_generation bigint
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
DECLARE
  v_key1 integer;
  v_key2 integer;
  v_generation bigint;
  v_state text;
  v_owner uuid;
  v_stored_nonce uuid;
  v_nonce uuid;
  v_stored_key1 integer;
  v_stored_key2 integer;
BEGIN
  PERFORM _oo._require_shard(p_shard);
  PERFORM _oo._require_positive_generation(p_generation);
  IF p_owner IS NULL THEN
    RAISE EXCEPTION 'owner token is required' USING ERRCODE = '23514';
  END IF;
  SELECT mapped.key1, mapped.key2
    INTO v_key1, v_key2
  FROM _oo.lock_key(p_shard) AS mapped;
  SELECT held.state
    INTO v_state
  FROM _oo.target_generations AS held
  WHERE held.shard_id = p_shard;
  IF FOUND AND v_state = 'failed' THEN
    RAISE EXCEPTION 'target generation is failed' USING ERRCODE = 'OO018';
  END IF;
  IF NOT _oo._backend_holds_lock(v_key1, v_key2) THEN
    RAISE EXCEPTION 'this backend does not hold the target lock' USING ERRCODE = 'OO011';
  END IF;
  PERFORM _oo._allow_write();
  SELECT
    held.generation,
    held.state,
    held.owner_token,
    held.session_nonce,
    held.lock_key1,
    held.lock_key2
    INTO
      v_generation,
      v_state,
      v_owner,
      v_stored_nonce,
      v_stored_key1,
      v_stored_key2
  FROM _oo.target_generations AS held
  WHERE held.shard_id = p_shard
  FOR UPDATE;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'target generation is not adopted' USING ERRCODE = 'OO013';
  END IF;
  IF v_state = 'failed' THEN
    RAISE EXCEPTION 'target generation is failed' USING ERRCODE = 'OO018';
  END IF;
  IF v_stored_key1 IS DISTINCT FROM v_key1 OR v_stored_key2 IS DISTINCT FROM v_key2 THEN
    RAISE EXCEPTION 'target lock key mapping changed' USING ERRCODE = 'OO019';
  END IF;
  v_nonce := _oo._session_nonce(p_shard);
  IF v_nonce IS NULL OR v_nonce IS DISTINCT FROM v_stored_nonce THEN
    RAISE EXCEPTION 'this backend does not hold the target lock' USING ERRCODE = 'OO011';
  END IF;
  IF v_state IS DISTINCT FROM 'adopted'
     OR v_generation IS DISTINCT FROM p_generation
     OR v_owner IS DISTINCT FROM p_owner
  THEN
    RAISE EXCEPTION 'target generation or owner is stale' USING ERRCODE = 'OO013';
  END IF;
END;
$$;

COMMENT ON FUNCTION _oo._load_held_generation(text, uuid, bigint) IS
  'Transaction-held fence check. It locks the generation row and proves this backend holds the session advisory lock. It does not authorize a later transaction.';

CREATE FUNCTION _oo.assert_target_fence(
  p_shard text,
  p_owner uuid,
  p_generation bigint
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
BEGIN
  PERFORM _oo._load_held_generation(p_shard, p_owner, p_generation);
END;
$$;

COMMENT ON FUNCTION _oo.assert_target_fence(text, uuid, bigint) IS
  'Proves the current backend owns the target lock for this open transaction. Committing the transaction ends the proof. A later write has to prove it again. Another connection that knows the owner and generation cannot pass.';

CREATE FUNCTION _oo.renew_target_generation(
  p_shard text,
  p_owner uuid,
  p_generation bigint
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
BEGIN
  PERFORM _oo._load_held_generation(p_shard, p_owner, p_generation);
  UPDATE _oo.target_generations
  SET renewed_at = clock_timestamp()
  WHERE shard_id = p_shard
    AND generation = p_generation
    AND owner_token = p_owner
    AND state = 'adopted';
END;
$$;

COMMENT ON FUNCTION _oo.renew_target_generation(text, uuid, bigint) IS
  'Refreshes renewed_at for the generation this backend already holds. It does not increment the generation and it does not consult a control-database lease.';

CREATE FUNCTION _oo.fail_target_generation(
  p_shard text,
  p_owner uuid,
  p_generation bigint
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
DECLARE
  v_key1 integer;
  v_key2 integer;
BEGIN
  PERFORM _oo._load_held_generation(p_shard, p_owner, p_generation);
  SELECT mapped.key1, mapped.key2
    INTO v_key1, v_key2
  FROM _oo.lock_key(p_shard) AS mapped;
  UPDATE _oo.target_generations
  SET
    state = 'failed',
    failed_at = clock_timestamp(),
    renewed_at = clock_timestamp()
  WHERE shard_id = p_shard
    AND generation = p_generation
    AND owner_token = p_owner;
  DELETE FROM pg_temp.oo_target_fence_session AS session_row
  WHERE session_row.shard_id = p_shard;
  PERFORM pg_catalog.pg_advisory_unlock(v_key1, v_key2);
END;
$$;

COMMENT ON FUNCTION _oo.fail_target_generation(text, uuid, bigint) IS
  'Marks the generation failed and releases this backend''s session lock. The unlock survives a later rollback of this transaction. A committed failure rejects a later receipt with OO018. A rolled-back failure leaves the row adopted and the lock released, so a later receipt raises OO011.';

CREATE FUNCTION _oo._require_digest(p_value text, p_name text)
RETURNS void
LANGUAGE plpgsql
IMMUTABLE
SECURITY INVOKER
SET search_path = pg_catalog
AS $$
BEGIN
  IF p_value IS NULL OR p_value !~ '^[0-9a-f]{64}$' THEN
    RAISE EXCEPTION 'closed digest %', p_name USING ERRCODE = '23514';
  END IF;
END;
$$;

CREATE FUNCTION _oo._require_position(p_applied_pos numeric)
RETURNS void
LANGUAGE plpgsql
IMMUTABLE
SECURITY INVOKER
SET search_path = pg_catalog
AS $$
BEGIN
  IF p_applied_pos IS NULL
     OR p_applied_pos < 0
     OR p_applied_pos <> trunc(p_applied_pos)
     OR p_applied_pos >= 100000000000000000000000000000000000000::numeric
  THEN
    RAISE EXCEPTION 'applied position is outside decimal128(38,0)' USING ERRCODE = '22003';
  END IF;
END;
$$;

CREATE FUNCTION _oo.commit_apply_receipt(
  p_shard text,
  p_owner uuid,
  p_generation bigint,
  p_source text,
  p_table_generation text,
  p_txn_or_batch_id text,
  p_config_digest text,
  p_type_digest text,
  p_hook_digest text,
  p_schema_digest text,
  p_payload_digest text,
  p_position_codec text,
  p_position_lineage text,
  p_source_epoch integer,
  p_applied_pos numeric,
  p_partial_txn boolean,
  p_cross_table_consistent boolean,
  p_txn_complete boolean,
  p_segment_index integer,
  p_segment_count integer,
  p_segment_bitmap bytea
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
DECLARE
  v_existing _oo.apply_receipts%ROWTYPE;
BEGIN
  IF p_source IS NULL OR p_source !~ '^[a-z][a-z0-9_]{0,62}$' THEN
    RAISE EXCEPTION 'closed source identity' USING ERRCODE = '23514';
  END IF;
  IF p_table_generation IS NULL OR p_table_generation !~ '^[a-z][a-z0-9_]{0,62}$' THEN
    RAISE EXCEPTION 'closed table generation' USING ERRCODE = '23514';
  END IF;
  IF p_txn_or_batch_id IS NULL OR p_txn_or_batch_id !~ '^[A-Za-z0-9_.:-]{1,128}$' THEN
    RAISE EXCEPTION 'closed batch identity' USING ERRCODE = '23514';
  END IF;
  PERFORM _oo._require_digest(p_config_digest, 'config_digest');
  PERFORM _oo._require_digest(p_type_digest, 'type_digest');
  PERFORM _oo._require_digest(p_hook_digest, 'hook_digest');
  PERFORM _oo._require_digest(p_schema_digest, 'schema_digest');
  PERFORM _oo._require_digest(p_payload_digest, 'payload_digest');
  IF p_position_codec IS NULL OR p_position_codec NOT IN (
    'pg-commit-lsn/v1',
    'mysql-binlog-pos/v1',
    'batch-cursor-us/v1',
    'snapshot-zero/v1'
  ) THEN
    RAISE EXCEPTION 'closed position codec' USING ERRCODE = 'OO015';
  END IF;
  IF p_position_lineage IS NULL OR p_position_lineage !~ '^[a-z][a-z0-9_.:-]{0,127}$' THEN
    RAISE EXCEPTION 'closed position lineage' USING ERRCODE = 'OO015';
  END IF;
  IF p_source_epoch IS NULL OR p_source_epoch < 0 THEN
    RAISE EXCEPTION 'closed source epoch' USING ERRCODE = '22003';
  END IF;
  PERFORM _oo._require_position(p_applied_pos);
  IF p_partial_txn IS NULL
     OR p_cross_table_consistent IS NULL
     OR p_txn_complete IS NULL
     OR p_segment_bitmap IS NULL
  THEN
    RAISE EXCEPTION 'closed completion metadata' USING ERRCODE = '23514';
  END IF;
  IF NOT (
    (
      p_partial_txn
      AND NOT p_txn_complete
      AND NOT p_cross_table_consistent
    )
    OR (
      NOT p_partial_txn
      AND p_txn_complete
      AND p_cross_table_consistent
    )
    OR (
      NOT p_partial_txn
      AND p_txn_complete
      AND NOT p_cross_table_consistent
    )
  ) THEN
    RAISE EXCEPTION 'closed completion metadata' USING ERRCODE = '23514';
  END IF;
  IF NOT (
    (
      p_segment_count IS NULL
      AND p_segment_index IS NULL
      AND octet_length(p_segment_bitmap) = 0
    )
    OR (
      p_segment_count >= 1
      AND p_segment_index >= 0
      AND p_segment_index < p_segment_count
      AND octet_length(p_segment_bitmap) BETWEEN 1 AND 8192
    )
  ) THEN
    RAISE EXCEPTION 'closed segment metadata' USING ERRCODE = '23514';
  END IF;
  PERFORM _oo._load_held_generation(p_shard, p_owner, p_generation);
  SELECT existing.*
    INTO v_existing
  FROM _oo.apply_receipts AS existing
  WHERE existing.source = p_source
    AND existing.table_generation = p_table_generation
    AND existing.txn_or_batch_id = p_txn_or_batch_id
  FOR UPDATE;
  IF FOUND THEN
    IF v_existing.config_digest = p_config_digest
       AND v_existing.type_digest = p_type_digest
       AND v_existing.hook_digest = p_hook_digest
       AND v_existing.schema_digest = p_schema_digest
       AND v_existing.payload_digest = p_payload_digest
       AND v_existing.position_codec = p_position_codec
       AND v_existing.position_lineage = p_position_lineage
       AND v_existing.source_epoch = p_source_epoch
       AND v_existing.applied_pos = p_applied_pos
       AND v_existing.partial_txn = p_partial_txn
       AND v_existing.cross_table_consistent = p_cross_table_consistent
       AND v_existing.txn_complete = p_txn_complete
       AND v_existing.segment_index IS NOT DISTINCT FROM p_segment_index
       AND v_existing.segment_count IS NOT DISTINCT FROM p_segment_count
       AND v_existing.segment_bitmap = p_segment_bitmap
       AND v_existing.config_version = 'config/v1'
       AND v_existing.type_policy_version = 'type-policy/v1'
       AND v_existing.protocol_version = 'batch/v1'
    THEN
      RETURN 'replay';
    END IF;
    RAISE EXCEPTION 'apply receipt identity conflict' USING ERRCODE = 'OO010';
  END IF;
  INSERT INTO _oo.apply_receipts (
    source,
    table_generation,
    txn_or_batch_id,
    config_version,
    config_digest,
    type_policy_version,
    type_digest,
    hook_digest,
    schema_digest,
    payload_digest,
    protocol_version,
    position_codec,
    position_lineage,
    source_epoch,
    applied_pos,
    partial_txn,
    cross_table_consistent,
    txn_complete,
    segment_index,
    segment_count,
    segment_bitmap,
    fence_shard,
    fence_generation,
    target_commit_at
  ) VALUES (
    p_source,
    p_table_generation,
    p_txn_or_batch_id,
    'config/v1',
    p_config_digest,
    'type-policy/v1',
    p_type_digest,
    p_hook_digest,
    p_schema_digest,
    p_payload_digest,
    'batch/v1',
    p_position_codec,
    p_position_lineage,
    p_source_epoch,
    p_applied_pos,
    p_partial_txn,
    p_cross_table_consistent,
    p_txn_complete,
    p_segment_index,
    p_segment_count,
    p_segment_bitmap,
    p_shard,
    p_generation,
    clock_timestamp()
  );
  RETURN 'inserted';
END;
$$;

COMMENT ON FUNCTION _oo.commit_apply_receipt(
  text, uuid, bigint, text, text, text, text, text, text, text, text, text, text, integer, numeric,
  boolean, boolean, boolean, integer, integer, bytea
) IS
  'Inserts one receipt or returns replay when every immutable field matches. Same identity with a different payload, codec, or lineage raises OO010 and does not update the stored row. The fence is checked again in this call. This function does not write user data, quality summaries, reject events, or a control-database mirror. The Loader must call it in the same target transaction as those writes.';

CREATE FUNCTION _oo.reject_direct_write()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog
AS $$
BEGIN
  IF TG_OP = 'TRUNCATE' THEN
    RAISE EXCEPTION 'target fence relations cannot be truncated' USING ERRCODE = 'OO014';
  END IF;
  IF TG_TABLE_NAME = 'apply_receipts' AND TG_OP <> 'INSERT' THEN
    RAISE EXCEPTION 'apply receipts are immutable' USING ERRCODE = 'OO014';
  END IF;
  IF TG_TABLE_NAME = 'target_generations' AND TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'target generations cannot be deleted' USING ERRCODE = 'OO014';
  END IF;
  IF pg_catalog.current_setting('_oo.writer', true) IS DISTINCT FROM 'apply-receipt' THEN
    RAISE EXCEPTION 'direct write to target fence relations is closed' USING ERRCODE = 'OO014';
  END IF;
  -- Nested so NEW.generation is not resolved on apply_receipts, which has no such column.
  IF TG_TABLE_NAME = 'target_generations' AND TG_OP = 'UPDATE' THEN
    IF NEW.generation < OLD.generation THEN
      RAISE EXCEPTION 'target generation decreased' USING ERRCODE = '22003';
    END IF;
    IF NEW.shard_id IS DISTINCT FROM OLD.shard_id
       OR NEW.lock_key1 IS DISTINCT FROM OLD.lock_key1
       OR NEW.lock_key2 IS DISTINCT FROM OLD.lock_key2
    THEN
      RAISE EXCEPTION 'target lock identity is immutable' USING ERRCODE = 'OO019';
    END IF;
  END IF;
  RETURN COALESCE(NEW, OLD);
END;
$$;

CREATE TRIGGER target_generations_reject_write
  BEFORE INSERT OR UPDATE OR DELETE ON _oo.target_generations
  FOR EACH ROW
  EXECUTE FUNCTION _oo.reject_direct_write();

CREATE TRIGGER target_generations_reject_truncate
  BEFORE TRUNCATE ON _oo.target_generations
  FOR EACH STATEMENT
  EXECUTE FUNCTION _oo.reject_direct_write();

CREATE TRIGGER apply_receipts_reject_write
  BEFORE INSERT OR UPDATE OR DELETE ON _oo.apply_receipts
  FOR EACH ROW
  EXECUTE FUNCTION _oo.reject_direct_write();

CREATE TRIGGER apply_receipts_reject_truncate
  BEFORE TRUNCATE ON _oo.apply_receipts
  FOR EACH STATEMENT
  EXECUTE FUNCTION _oo.reject_direct_write();

CREATE FUNCTION _oo.reject_migration_mutation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog
AS $$
BEGIN
  RAISE EXCEPTION 'schema migration rows are immutable' USING ERRCODE = 'OO014';
END;
$$;

CREATE TRIGGER schema_migrations_reject_update
  BEFORE UPDATE ON _oo.schema_migrations
  FOR EACH ROW
  EXECUTE FUNCTION _oo.reject_migration_mutation();

CREATE TRIGGER schema_migrations_reject_delete
  BEFORE DELETE ON _oo.schema_migrations
  FOR EACH ROW
  EXECUTE FUNCTION _oo.reject_migration_mutation();

CREATE FUNCTION _oo.schema_ready(p_checksum text)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY INVOKER
SET search_path = pg_catalog, _oo
AS $$
  SELECT EXISTS (
    SELECT 1
    FROM _oo.schema_migrations AS recorded
    WHERE recorded.version = 'target-fence-receipts/1'
      AND recorded.phase = 'expand'
      AND recorded.checksum = p_checksum
      AND p_checksum ~ '^[0-9a-f]{64}$'
  );
$$;

COMMENT ON FUNCTION _oo.schema_ready(text) IS
  'True only when the expand row exists and the stored checksum equals the caller''s release checksum. A mismatch returns false and does not change the row.';

CREATE VIEW _oo.service_readiness
WITH (security_invoker = true) AS
SELECT
  version,
  checksum,
  phase,
  filename,
  applied_at,
  (version = 'target-fence-receipts/1' AND phase = 'expand') AS schema_version_present
FROM _oo.schema_migrations
WHERE version = 'target-fence-receipts/1';

COMMENT ON VIEW _oo.service_readiness IS
  'Version marker. schema_version_present does not prove the file bytes match. Compare checksum outside this view.';

REVOKE ALL ON ALL TABLES IN SCHEMA _oo FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA _oo FROM PUBLIC;

GRANT SELECT ON _oo.schema_migrations TO oo_pipeline_runtime;
GRANT SELECT ON _oo.service_readiness TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.schema_ready(text) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.adopt_target_generation(text, uuid) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.renew_target_generation(text, uuid, bigint) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.fail_target_generation(text, uuid, bigint) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.assert_target_fence(text, uuid, bigint) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.commit_apply_receipt(
  text, uuid, bigint, text, text, text, text, text, text, text, text, text, text, integer, numeric,
  boolean, boolean, boolean, integer, integer, bytea
) TO oo_pipeline_runtime;

SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;
-- target-fence-receipts/3 phase expand
-- Run as oo_target_owner after an approved target-fence-receipts/1 schema,
-- or after target-fence-receipts/2 when that marker is already stored.
-- The applicator holds pg_advisory_xact_lock(6404202605) and, when the row
-- exists, _oo.migration_lock FOR UPDATE. This file does not insert a
-- readiness row. The applicator inserts target-fence-receipts/3 only after
-- this file returns, in the same transaction.
--
-- 0000_bootstrap_roles.sql, 0001_target_fence_expand.sql, and
-- 0002_target_segment_null_expand.sql stay byte-identical. This file
-- supersedes 0002 when the applicator is installing the closed segment
-- rule from v1. It does not rewrite a stored v1 or v2 checksum.
--
-- 0002 folds a regclass cast of the receipt relation while testing
-- to_regclass. On a database with no such relation that cast raises 42P01
-- before the OO002 handler. A later migration in the same aborted
-- transaction cannot repair that failure. The reference applicator
-- therefore executes this file instead of 0002. Running 0002 first on a
-- missing relation is not this install path.
--
-- Preflight resolves the relation with to_regclass into an oid, then looks
-- up apply_receipts_segment_check by that oid. There is no regclass literal.
-- A following probe catches undefined_table and raises the same static OO002.
-- LOCK TABLE is a top-level statement so the share lock is held until the
-- applicator transaction commits. This file does not delete, update, or
-- rewrite _oo.apply_receipts.

SELECT pg_catalog.set_config('search_path', 'pg_catalog, _oo', true);

DO $segment_preflight$
DECLARE
  relation_id oid;
BEGIN
  relation_id := pg_catalog.to_regclass('_oo.apply_receipts');
  IF relation_id IS NULL THEN
    RAISE EXCEPTION 'target-fence-receipts/1 segment relation is absent'
      USING ERRCODE = 'OO002';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM pg_catalog.pg_constraint AS constraint_row
    WHERE constraint_row.conname = 'apply_receipts_segment_check'
      AND constraint_row.conrelid = relation_id
  ) THEN
    RAISE EXCEPTION 'target-fence-receipts/1 segment relation is absent'
      USING ERRCODE = 'OO002';
  END IF;
END
$segment_preflight$;

DO $segment_owner$
DECLARE
  function_owner text;
  function_count integer;
  view_owner text;
  ready_owner text;
  contract_owner text;
  preflight_owner text;
BEGIN
  SELECT count(*), min(pg_catalog.pg_get_userbyid(proc.proowner))
    INTO function_count, function_owner
  FROM pg_catalog.pg_proc AS proc
  JOIN pg_catalog.pg_namespace AS nsp ON nsp.oid = proc.pronamespace
  WHERE nsp.nspname = '_oo'
    AND proc.proname = 'commit_apply_receipt';
  IF function_count > 1
     OR (function_count = 1 AND function_owner IS DISTINCT FROM 'oo_target_owner')
  THEN
    RAISE EXCEPTION 'target segment preflight refuses to adopt commit_apply_receipt'
      USING ERRCODE = '42501';
  END IF;

  SELECT pg_catalog.pg_get_userbyid(proc.proowner)
    INTO contract_owner
  FROM pg_catalog.pg_proc AS proc
  JOIN pg_catalog.pg_namespace AS nsp ON nsp.oid = proc.pronamespace
  WHERE nsp.nspname = '_oo'
    AND proc.proname = 'segment_contract_ready';
  IF contract_owner IS NOT NULL AND contract_owner IS DISTINCT FROM 'oo_target_owner' THEN
    RAISE EXCEPTION 'target segment preflight refuses to adopt segment_contract_ready'
      USING ERRCODE = '42501';
  END IF;

  SELECT pg_catalog.pg_get_userbyid(proc.proowner)
    INTO preflight_owner
  FROM pg_catalog.pg_proc AS proc
  JOIN pg_catalog.pg_namespace AS nsp ON nsp.oid = proc.pronamespace
  WHERE nsp.nspname = '_oo'
    AND proc.proname = 'segment_preflight_ready';
  IF preflight_owner IS NOT NULL AND preflight_owner IS DISTINCT FROM 'oo_target_owner' THEN
    RAISE EXCEPTION 'target segment preflight refuses to adopt segment_preflight_ready'
      USING ERRCODE = '42501';
  END IF;

  SELECT pg_catalog.pg_get_userbyid(cls.relowner)
    INTO view_owner
  FROM pg_catalog.pg_class AS cls
  JOIN pg_catalog.pg_namespace AS nsp ON nsp.oid = cls.relnamespace
  WHERE nsp.nspname = '_oo'
    AND cls.relname = 'segment_service_readiness'
    AND cls.relkind = 'v';
  IF view_owner IS NOT NULL AND view_owner IS DISTINCT FROM 'oo_target_owner' THEN
    RAISE EXCEPTION 'target segment preflight refuses to adopt segment_service_readiness'
      USING ERRCODE = '42501';
  END IF;

  SELECT pg_catalog.pg_get_userbyid(cls.relowner)
    INTO ready_owner
  FROM pg_catalog.pg_class AS cls
  JOIN pg_catalog.pg_namespace AS nsp ON nsp.oid = cls.relnamespace
  WHERE nsp.nspname = '_oo'
    AND cls.relname = 'segment_preflight_readiness'
    AND cls.relkind = 'v';
  IF ready_owner IS NOT NULL AND ready_owner IS DISTINCT FROM 'oo_target_owner' THEN
    RAISE EXCEPTION 'target segment preflight refuses to adopt segment_preflight_readiness'
      USING ERRCODE = '42501';
  END IF;
END
$segment_owner$;

DO $segment_touch$
BEGIN
  PERFORM 1
  FROM _oo.apply_receipts AS receipt
  WHERE false;
EXCEPTION
  WHEN undefined_table THEN
    RAISE EXCEPTION 'target-fence-receipts/1 segment relation is absent'
      USING ERRCODE = 'OO002';
END
$segment_touch$;

LOCK TABLE _oo.apply_receipts IN SHARE ROW EXCLUSIVE MODE;

DO $segment_history$
BEGIN
  IF EXISTS (
    SELECT 1
    FROM _oo.apply_receipts AS receipt
    WHERE (
      (
        receipt.segment_count IS NULL
        AND receipt.segment_index IS NULL
        AND octet_length(receipt.segment_bitmap) = 0
      )
      OR (
        receipt.segment_count IS NOT NULL
        AND receipt.segment_index IS NOT NULL
        AND receipt.segment_count >= 1
        AND receipt.segment_index >= 0
        AND receipt.segment_index < receipt.segment_count
        AND octet_length(receipt.segment_bitmap) BETWEEN 1 AND 8192
      )
    ) IS NOT TRUE
  ) THEN
    RAISE EXCEPTION 'refusing target segment correction: preexisting apply receipts violate the closed segment rule; this migration does not delete, rewrite, or mutate receipts'
      USING ERRCODE = 'OO021';
  END IF;
END
$segment_history$;

ALTER TABLE _oo.apply_receipts
  DROP CONSTRAINT apply_receipts_segment_check;

ALTER TABLE _oo.apply_receipts
  ADD CONSTRAINT apply_receipts_segment_check CHECK (
    (
      (
        segment_count IS NULL
        AND segment_index IS NULL
        AND octet_length(segment_bitmap) = 0
      )
      OR (
        segment_count IS NOT NULL
        AND segment_index IS NOT NULL
        AND segment_count >= 1
        AND segment_index >= 0
        AND segment_index < segment_count
        AND octet_length(segment_bitmap) BETWEEN 1 AND 8192
      )
    ) IS TRUE
  );

COMMENT ON CONSTRAINT apply_receipts_segment_check ON _oo.apply_receipts IS
  'Closed segment metadata. Both index and count are null only with an empty bitmap. A present segment has both fields non-null, count at least 1, index in range, and a bitmap of 1 to 8192 bytes. The IS TRUE wrapper rejects unknown. This constraint does not delete or rewrite an existing row.';

CREATE OR REPLACE FUNCTION _oo.commit_apply_receipt(
  p_shard text,
  p_owner uuid,
  p_generation bigint,
  p_source text,
  p_table_generation text,
  p_txn_or_batch_id text,
  p_config_digest text,
  p_type_digest text,
  p_hook_digest text,
  p_schema_digest text,
  p_payload_digest text,
  p_position_codec text,
  p_position_lineage text,
  p_source_epoch integer,
  p_applied_pos numeric,
  p_partial_txn boolean,
  p_cross_table_consistent boolean,
  p_txn_complete boolean,
  p_segment_index integer,
  p_segment_count integer,
  p_segment_bitmap bytea
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
DECLARE
  v_existing _oo.apply_receipts%ROWTYPE;
BEGIN
  IF p_source IS NULL OR p_source !~ '^[a-z][a-z0-9_]{0,62}$' THEN
    RAISE EXCEPTION 'closed source identity' USING ERRCODE = '23514';
  END IF;
  IF p_table_generation IS NULL OR p_table_generation !~ '^[a-z][a-z0-9_]{0,62}$' THEN
    RAISE EXCEPTION 'closed table generation' USING ERRCODE = '23514';
  END IF;
  IF p_txn_or_batch_id IS NULL OR p_txn_or_batch_id !~ '^[A-Za-z0-9_.:-]{1,128}$' THEN
    RAISE EXCEPTION 'closed batch identity' USING ERRCODE = '23514';
  END IF;
  PERFORM _oo._require_digest(p_config_digest, 'config_digest');
  PERFORM _oo._require_digest(p_type_digest, 'type_digest');
  PERFORM _oo._require_digest(p_hook_digest, 'hook_digest');
  PERFORM _oo._require_digest(p_schema_digest, 'schema_digest');
  PERFORM _oo._require_digest(p_payload_digest, 'payload_digest');
  IF p_position_codec IS NULL OR p_position_codec NOT IN (
    'pg-commit-lsn/v1',
    'mysql-binlog-pos/v1',
    'batch-cursor-us/v1',
    'snapshot-zero/v1'
  ) THEN
    RAISE EXCEPTION 'closed position codec' USING ERRCODE = 'OO015';
  END IF;
  IF p_position_lineage IS NULL OR p_position_lineage !~ '^[a-z][a-z0-9_.:-]{0,127}$' THEN
    RAISE EXCEPTION 'closed position lineage' USING ERRCODE = 'OO015';
  END IF;
  IF p_source_epoch IS NULL OR p_source_epoch < 0 THEN
    RAISE EXCEPTION 'closed source epoch' USING ERRCODE = '22003';
  END IF;
  PERFORM _oo._require_position(p_applied_pos);
  IF p_partial_txn IS NULL
     OR p_cross_table_consistent IS NULL
     OR p_txn_complete IS NULL
     OR p_segment_bitmap IS NULL
  THEN
    RAISE EXCEPTION 'closed completion metadata' USING ERRCODE = '23514';
  END IF;
  IF NOT (
    (
      p_partial_txn
      AND NOT p_txn_complete
      AND NOT p_cross_table_consistent
    )
    OR (
      NOT p_partial_txn
      AND p_txn_complete
      AND p_cross_table_consistent
    )
    OR (
      NOT p_partial_txn
      AND p_txn_complete
      AND NOT p_cross_table_consistent
    )
  ) THEN
    RAISE EXCEPTION 'closed completion metadata' USING ERRCODE = '23514';
  END IF;
  IF (
    (
      p_segment_count IS NULL
      AND p_segment_index IS NULL
      AND octet_length(p_segment_bitmap) = 0
    )
    OR (
      p_segment_count IS NOT NULL
      AND p_segment_index IS NOT NULL
      AND p_segment_count >= 1
      AND p_segment_index >= 0
      AND p_segment_index < p_segment_count
      AND octet_length(p_segment_bitmap) BETWEEN 1 AND 8192
    )
  ) IS NOT TRUE THEN
    RAISE EXCEPTION 'closed segment metadata' USING ERRCODE = '23514';
  END IF;
  PERFORM _oo._load_held_generation(p_shard, p_owner, p_generation);
  SELECT existing.*
    INTO v_existing
  FROM _oo.apply_receipts AS existing
  WHERE existing.source = p_source
    AND existing.table_generation = p_table_generation
    AND existing.txn_or_batch_id = p_txn_or_batch_id
  FOR UPDATE;
  IF FOUND THEN
    IF v_existing.config_digest = p_config_digest
       AND v_existing.type_digest = p_type_digest
       AND v_existing.hook_digest = p_hook_digest
       AND v_existing.schema_digest = p_schema_digest
       AND v_existing.payload_digest = p_payload_digest
       AND v_existing.position_codec = p_position_codec
       AND v_existing.position_lineage = p_position_lineage
       AND v_existing.source_epoch = p_source_epoch
       AND v_existing.applied_pos = p_applied_pos
       AND v_existing.partial_txn = p_partial_txn
       AND v_existing.cross_table_consistent = p_cross_table_consistent
       AND v_existing.txn_complete = p_txn_complete
       AND v_existing.segment_index IS NOT DISTINCT FROM p_segment_index
       AND v_existing.segment_count IS NOT DISTINCT FROM p_segment_count
       AND v_existing.segment_bitmap = p_segment_bitmap
       AND v_existing.config_version = 'config/v1'
       AND v_existing.type_policy_version = 'type-policy/v1'
       AND v_existing.protocol_version = 'batch/v1'
    THEN
      RETURN 'replay';
    END IF;
    RAISE EXCEPTION 'apply receipt identity conflict' USING ERRCODE = 'OO010';
  END IF;
  INSERT INTO _oo.apply_receipts (
    source,
    table_generation,
    txn_or_batch_id,
    config_version,
    config_digest,
    type_policy_version,
    type_digest,
    hook_digest,
    schema_digest,
    payload_digest,
    protocol_version,
    position_codec,
    position_lineage,
    source_epoch,
    applied_pos,
    partial_txn,
    cross_table_consistent,
    txn_complete,
    segment_index,
    segment_count,
    segment_bitmap,
    fence_shard,
    fence_generation,
    target_commit_at
  ) VALUES (
    p_source,
    p_table_generation,
    p_txn_or_batch_id,
    'config/v1',
    p_config_digest,
    'type-policy/v1',
    p_type_digest,
    p_hook_digest,
    p_schema_digest,
    p_payload_digest,
    'batch/v1',
    p_position_codec,
    p_position_lineage,
    p_source_epoch,
    p_applied_pos,
    p_partial_txn,
    p_cross_table_consistent,
    p_txn_complete,
    p_segment_index,
    p_segment_count,
    p_segment_bitmap,
    p_shard,
    p_generation,
    clock_timestamp()
  );
  RETURN 'inserted';
END;
$$;

COMMENT ON FUNCTION _oo.commit_apply_receipt(
  text, uuid, bigint, text, text, text, text, text, text, text, text, text, text, integer, numeric,
  boolean, boolean, boolean, integer, integer, bytea
) IS
  'Inserts one receipt or returns replay when every immutable field matches. Same identity with a different payload, codec, or lineage raises OO010 and does not update the stored row. Segment index and count are closed before the fence check: both null with an empty bitmap, or both non-null with a legal index and a bitmap of 1 to 8192 bytes. Unknown combinations raise 23514 and are not reported as OO013. The fence is checked again in this call. This function does not write user data, quality summaries, reject events, or a control-database mirror. The Loader must call it in the same target transaction as those writes.';

CREATE OR REPLACE FUNCTION _oo.segment_preflight_ready(p_checksum text)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY INVOKER
SET search_path = pg_catalog, _oo
AS $preflight_ready$
  SELECT EXISTS (
    SELECT 1
    FROM _oo.schema_migrations AS recorded
    WHERE recorded.version = 'target-fence-receipts/3'
      AND recorded.phase = 'expand'
      AND recorded.checksum = p_checksum
      AND p_checksum ~ '^[0-9a-f]{64}$'
  );
$preflight_ready$;

COMMENT ON FUNCTION _oo.segment_preflight_ready(text) IS
  'True only when the target-fence-receipts/3 expand row stores the caller checksum. This function does not read 0003_target_segment_preflight_expand.sql and does not treat the stored checksum as a file hash. A driver that requires this contract is authorized only when the caller computed that file''s SHA-256 and this function returns true for that digest. _oo.schema_ready for target-fence-receipts/1 and _oo.segment_contract_ready for target-fence-receipts/2 do not authorize that driver. A stored lie is true only for the lie.';

CREATE OR REPLACE VIEW _oo.segment_preflight_readiness
WITH (security_invoker = true) AS
SELECT
  version,
  checksum,
  phase,
  filename,
  applied_at,
  (version = 'target-fence-receipts/3' AND phase = 'expand') AS schema_version_present
FROM _oo.schema_migrations
WHERE version = 'target-fence-receipts/3';

COMMENT ON VIEW _oo.segment_preflight_readiness IS
  'Version marker for target-fence-receipts/3. schema_version_present does not prove these file bytes. Compare checksum to the SHA-256 of 0003_target_segment_preflight_expand.sql outside the database. target-fence-receipts/1 and target-fence-receipts/2 readiness do not satisfy this view.';

REVOKE ALL ON FUNCTION _oo.segment_preflight_ready(text) FROM PUBLIC;
REVOKE ALL ON TABLE _oo.segment_preflight_readiness FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.segment_preflight_ready(text) TO oo_pipeline_runtime;
GRANT SELECT ON _oo.segment_preflight_readiness TO oo_pipeline_runtime;

-- physical-mapping/v1 phase expand
-- Run as oo_target_owner after 0001_target_fence_expand.sql.
-- 0003_target_segment_preflight_expand.sql already occupies migration 0003,
-- so this binding is 0004. It does not modify 0000, 0001, 0002, or 0003.
-- Logical names are pgident/v1 strings. This file stores their PostgreSQL
-- identifiers. It does not write user data, shadows, or receipts.

CREATE FUNCTION _oo.pm_rotr(p_value bigint, p_bits integer)
RETURNS bigint
LANGUAGE sql
IMMUTABLE
STRICT
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
  SELECT (
    ((p_value & 4294967295) >> p_bits)
    | (((p_value & 4294967295) << (32 - p_bits)) & 4294967295)
  ) & 4294967295;
$pm$;

CREATE FUNCTION _oo.pm_sha256(p_message bytea)
RETURNS bytea
LANGUAGE plpgsql
IMMUTABLE
STRICT
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
DECLARE
  k bigint[] := ARRAY[
    1116352408, 1899447441, 3049323471, 3921009573, 961987163, 1508970993, 2453635748, 2870763221,
    3624381080, 310598401, 607225278, 1426881987, 1925078388, 2162078206, 2614888103, 3248222580,
    3835390401, 4022224774, 264347078, 604807628, 770255983, 1249150122, 1555081692, 1996064986,
    2554220882, 2821834349, 2952996808, 3210313671, 3336571891, 3584528711, 113926993, 338241895,
    666307205, 773529912, 1294757372, 1396182291, 1695183700, 1986661051, 2177026350, 2456956037,
    2730485921, 2820302411, 3259730800, 3345764771, 3516065817, 3600352804, 4094571909, 275423344,
    430227734, 506948616, 659060556, 883997877, 958139571, 1322822218, 1537002063, 1747873779,
    1955562222, 2024104815, 2227730452, 2361852424, 2428436474, 2756734187, 3204031479, 3329325298
  ];
  h bigint[] := ARRAY[
    1779033703,
    3144134277,
    1013904242,
    2773480762,
    1359893119,
    2600822924,
    528734635,
    1541459225
  ];
  ml integer;
  bitlen bigint;
  pad_zeros integer;
  padded bytea;
  len_bytes bytea;
  blocks integer;
  block_index integer;
  v_offset integer;
  i integer;
  w bigint[];
  word bigint;
  a bigint;
  b bigint;
  c bigint;
  d bigint;
  e bigint;
  f bigint;
  g bigint;
  hh bigint;
  s0 bigint;
  s1 bigint;
  ch bigint;
  maj bigint;
  t1 bigint;
  t2 bigint;
  out_bytes bytea;
BEGIN
  ml := octet_length(p_message);
  bitlen := ml::bigint * 8;
  pad_zeros := 55 - (ml % 64);
  IF pad_zeros < 0 THEN
    pad_zeros := pad_zeros + 64;
  END IF;
  padded := p_message || decode('80', 'hex');
  IF pad_zeros > 0 THEN
    padded := padded || decode(repeat('00', pad_zeros), 'hex');
  END IF;
  len_bytes := decode('0000000000000000', 'hex');
  FOR i IN 0..7 LOOP
    len_bytes := set_byte(len_bytes, i, ((bitlen >> (56 - i * 8)) & 255)::integer);
  END LOOP;
  padded := padded || len_bytes;
  IF octet_length(padded) % 64 <> 0 THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  blocks := octet_length(padded) / 64;
  FOR block_index IN 0..blocks - 1 LOOP
    w := array_fill(0::bigint, ARRAY[64]);
    FOR i IN 0..15 LOOP
      v_offset := block_index * 64 + i * 4;
      word := (get_byte(padded, v_offset)::bigint << 24)
        | (get_byte(padded, v_offset + 1)::bigint << 16)
        | (get_byte(padded, v_offset + 2)::bigint << 8)
        | get_byte(padded, v_offset + 3)::bigint;
      w[i + 1] := word;
    END LOOP;
    FOR i IN 16..63 LOOP
      s0 := (
        _oo.pm_rotr(w[i - 15 + 1], 7)
        # _oo.pm_rotr(w[i - 15 + 1], 18)
        # ((w[i - 15 + 1] >> 3) & 4294967295)
      ) & 4294967295;
      s1 := (
        _oo.pm_rotr(w[i - 2 + 1], 17)
        # _oo.pm_rotr(w[i - 2 + 1], 19)
        # ((w[i - 2 + 1] >> 10) & 4294967295)
      ) & 4294967295;
      w[i + 1] := (w[i - 16 + 1] + s0 + w[i - 7 + 1] + s1) & 4294967295;
    END LOOP;
    a := h[1];
    b := h[2];
    c := h[3];
    d := h[4];
    e := h[5];
    f := h[6];
    g := h[7];
    hh := h[8];
    FOR i IN 0..63 LOOP
      s1 := (_oo.pm_rotr(e, 6) # _oo.pm_rotr(e, 11) # _oo.pm_rotr(e, 25)) & 4294967295;
      ch := ((e & f) # ((4294967295 # e) & g)) & 4294967295;
      t1 := (hh + s1 + ch + k[i + 1] + w[i + 1]) & 4294967295;
      s0 := (_oo.pm_rotr(a, 2) # _oo.pm_rotr(a, 13) # _oo.pm_rotr(a, 22)) & 4294967295;
      maj := ((a & b) # (a & c) # (b & c)) & 4294967295;
      t2 := (s0 + maj) & 4294967295;
      hh := g;
      g := f;
      f := e;
      e := (d + t1) & 4294967295;
      d := c;
      c := b;
      b := a;
      a := (t1 + t2) & 4294967295;
    END LOOP;
    h[1] := (h[1] + a) & 4294967295;
    h[2] := (h[2] + b) & 4294967295;
    h[3] := (h[3] + c) & 4294967295;
    h[4] := (h[4] + d) & 4294967295;
    h[5] := (h[5] + e) & 4294967295;
    h[6] := (h[6] + f) & 4294967295;
    h[7] := (h[7] + g) & 4294967295;
    h[8] := (h[8] + hh) & 4294967295;
  END LOOP;
  out_bytes := decode('', 'hex');
  FOR i IN 1..8 LOOP
    out_bytes := out_bytes || decode(lpad(to_hex(h[i]), 8, '0'), 'hex');
  END LOOP;
  RETURN out_bytes;
END;
$pm$;

CREATE FUNCTION _oo.pm_source_ok(p_name text)
RETURNS boolean
LANGUAGE plpgsql
STABLE
STRICT
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
DECLARE
  v_bytes bytea;
  v_len integer;
  v_index integer;
  v_byte integer;
BEGIN
  v_bytes := convert_to(p_name, 'UTF8');
  v_len := octet_length(v_bytes);
  IF v_len < 1 OR v_len > 63 THEN
    RETURN false;
  END IF;
  v_byte := get_byte(v_bytes, 0);
  IF v_byte < 97 OR v_byte > 122 THEN
    RETURN false;
  END IF;
  FOR v_index IN 1..v_len - 1 LOOP
    v_byte := get_byte(v_bytes, v_index);
    IF NOT (v_byte = 95 OR (v_byte BETWEEN 48 AND 57) OR (v_byte BETWEEN 97 AND 122)) THEN
      RETURN false;
    END IF;
  END LOOP;
  RETURN true;
END;
$pm$;

CREATE FUNCTION _oo.pm_config_ok(p_name text)
RETURNS boolean
LANGUAGE plpgsql
STABLE
STRICT
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
DECLARE
  v_bytes bytea;
  v_len integer;
  v_index integer;
  v_byte integer;
BEGIN
  v_bytes := convert_to(p_name, 'UTF8');
  v_len := octet_length(v_bytes);
  IF v_len <> 64 THEN
    RETURN false;
  END IF;
  FOR v_index IN 0..63 LOOP
    v_byte := get_byte(v_bytes, v_index);
    IF NOT ((v_byte BETWEEN 48 AND 57) OR (v_byte BETWEEN 97 AND 102)) THEN
      RETURN false;
    END IF;
  END LOOP;
  RETURN true;
END;
$pm$;

CREATE FUNCTION _oo.pm_lower_hex(p_text text, p_len integer)
RETURNS boolean
LANGUAGE plpgsql
STABLE
STRICT
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
DECLARE
  v_bytes bytea;
  v_len integer;
  v_index integer;
  v_byte integer;
BEGIN
  IF p_len < 0 THEN
    RETURN false;
  END IF;
  v_bytes := convert_to(p_text, 'UTF8');
  v_len := octet_length(v_bytes);
  IF v_len <> p_len THEN
    RETURN false;
  END IF;
  FOR v_index IN 0..v_len - 1 LOOP
    v_byte := get_byte(v_bytes, v_index);
    IF NOT ((v_byte BETWEEN 48 AND 57) OR (v_byte BETWEEN 97 AND 102)) THEN
      RETURN false;
    END IF;
  END LOOP;
  RETURN true;
END;
$pm$;

CREATE FUNCTION _oo.pm_logical_ok(p_name text)
RETURNS boolean
LANGUAGE plpgsql
STABLE
STRICT
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
DECLARE
  v_bytes bytea;
  v_len integer;
  v_index integer;
  v_byte integer;
  v_reserved text[] := ARRAY[
-- pm-reserved-begin
    'all',
    'analyse',
    'analyze',
    'and',
    'any',
    'array',
    'as',
    'asc',
    'asymmetric',
    'both',
    'case',
    'cast',
    'check',
    'collate',
    'column',
    'constraint',
    'create',
    'current_catalog',
    'current_date',
    'current_role',
    'current_schema',
    'current_time',
    'current_timestamp',
    'current_user',
    'default',
    'deferrable',
    'desc',
    'distinct',
    'do',
    'else',
    'end',
    'except',
    'false',
    'fetch',
    'for',
    'foreign',
    'from',
    'grant',
    'group',
    'having',
    'in',
    'initially',
    'intersect',
    'into',
    'lateral',
    'leading',
    'limit',
    'localtime',
    'localtimestamp',
    'not',
    'null',
    'offset',
    'on',
    'only',
    'or',
    'order',
    'placing',
    'primary',
    'references',
    'returning',
    'select',
    'session_user',
    'some',
    'symmetric',
    'system_user',
    'table',
    'then',
    'to',
    'trailing',
    'true',
    'union',
    'unique',
    'user',
    'using',
    'variadic',
    'when',
    'where',
    'window',
    'with'
-- pm-reserved-end
  ];
  v_reserved_hit boolean;
BEGIN
  v_bytes := convert_to(p_name, 'UTF8');
  v_len := octet_length(v_bytes);
  IF v_len < 1 OR v_len > 4096 THEN
    RETURN false;
  END IF;
  IF v_len <= 63
     AND NOT (
       v_len >= 2
       AND get_byte(v_bytes, 0) = 113
       AND get_byte(v_bytes, 1) = 95
     )
  THEN
    v_byte := get_byte(v_bytes, 0);
    IF NOT (v_byte = 95 OR (v_byte BETWEEN 97 AND 122)) THEN
      RETURN false;
    END IF;
    FOR v_index IN 1..v_len - 1 LOOP
      v_byte := get_byte(v_bytes, v_index);
      IF NOT (v_byte = 95 OR (v_byte BETWEEN 48 AND 57) OR (v_byte BETWEEN 97 AND 122)) THEN
        RETURN false;
      END IF;
    END LOOP;
    SELECT EXISTS (
      SELECT 1
      FROM unnest(v_reserved) AS word
      WHERE convert_to(word, 'UTF8') = v_bytes
    )
      INTO v_reserved_hit;
    RETURN NOT v_reserved_hit;
  END IF;
  IF v_len < 4 OR v_len = 63 OR ((v_len - 2) % 2) <> 0 THEN
    RETURN false;
  END IF;
  IF get_byte(v_bytes, 0) <> 113 OR get_byte(v_bytes, 1) <> 95 THEN
    RETURN false;
  END IF;
  FOR v_index IN 2..v_len - 1 LOOP
    v_byte := get_byte(v_bytes, v_index);
    IF NOT ((v_byte BETWEEN 48 AND 57) OR (v_byte BETWEEN 97 AND 102)) THEN
      RETURN false;
    END IF;
  END LOOP;
  RETURN true;
END;
$pm$;

CREATE FUNCTION _oo.pm_payload(
  p_source text,
  p_config_digest text,
  p_run_generation bigint,
  p_level text,
  p_logical_namespace text,
  p_logical_table text,
  p_logical_column text
)
RETURNS bytea
LANGUAGE sql
STABLE
STRICT
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
  SELECT convert_to('physical-mapping/v1', 'UTF8') || decode('00', 'hex')
    || convert_to(p_source, 'UTF8') || decode('00', 'hex')
    || convert_to(p_config_digest, 'UTF8') || decode('00', 'hex')
    || convert_to(p_run_generation::text, 'UTF8') || decode('00', 'hex')
    || convert_to(p_level, 'UTF8')
    || convert_to(p_logical_namespace, 'UTF8') || decode('00', 'hex')
    || convert_to(p_logical_table, 'UTF8') || decode('00', 'hex')
    || convert_to(p_logical_column, 'UTF8') || decode('00', 'hex');
$pm$;

CREATE FUNCTION _oo.pm_physical(p_level text, p_digest bytea)
RETURNS text
LANGUAGE sql
IMMUTABLE
STRICT
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
  SELECT CASE
    WHEN octet_length(p_digest) = 32 AND p_level IN ('n', 't', 'c') THEN
      CASE p_level
        WHEN 'n' THEN 'pn_'
        WHEN 't' THEN 'pt_'
        ELSE 'pc_'
      END || substring(encode(p_digest, 'hex') FROM 1 FOR 60)
    ELSE NULL
  END;
$pm$;

CREATE TABLE _oo.physical_mappings (
  source text NOT NULL,
  config_digest text NOT NULL,
  run_generation bigint NOT NULL,
  level text NOT NULL,
  logical_namespace text NOT NULL,
  logical_table text NOT NULL,
  logical_column text NOT NULL,
  identity_digest bytea NOT NULL,
  physical_name text NOT NULL,
  scope_namespace_physical text NOT NULL,
  scope_table_physical text,
  algorithm text NOT NULL,
  registered_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  CONSTRAINT physical_mappings_pkey PRIMARY KEY (
    source, config_digest, run_generation, level, logical_namespace, logical_table, logical_column
  ),
  CONSTRAINT physical_mappings_identity_digest_unique UNIQUE (identity_digest),
  CONSTRAINT physical_mappings_physical_name_unique UNIQUE (physical_name),
  CONSTRAINT physical_mappings_table_scope_unique UNIQUE (scope_namespace_physical, physical_name),
  CONSTRAINT physical_mappings_column_scope_unique UNIQUE NULLS NOT DISTINCT (
    scope_namespace_physical, scope_table_physical, physical_name
  ),
  CONSTRAINT physical_mappings_generation_check CHECK (run_generation >= 1),
  CONSTRAINT physical_mappings_level_check CHECK (level IN ('n', 't', 'c')),
  CONSTRAINT physical_mappings_algorithm_check CHECK (algorithm = 'physical-mapping/v1'),
  CONSTRAINT physical_mappings_digest_len_check CHECK (octet_length(identity_digest) = 32),
  CONSTRAINT physical_mappings_source_check CHECK (_oo.pm_source_ok(source)),
  CONSTRAINT physical_mappings_config_check CHECK (_oo.pm_config_ok(config_digest)),
  CONSTRAINT physical_mappings_namespace_check CHECK (_oo.pm_logical_ok(logical_namespace)),
  CONSTRAINT physical_mappings_table_name_check CHECK (
    (level = 'n' AND logical_table = '')
    OR (level IN ('t', 'c') AND _oo.pm_logical_ok(logical_table))
  ),
  CONSTRAINT physical_mappings_column_name_check CHECK (
    (level IN ('n', 't') AND logical_column = '')
    OR (level = 'c' AND _oo.pm_logical_ok(logical_column))
  ),
  CONSTRAINT physical_mappings_physical_check CHECK (
    octet_length(physical_name) = 63
    AND physical_name = (
      CASE level
        WHEN 'n' THEN 'pn_'
        WHEN 't' THEN 'pt_'
        WHEN 'c' THEN 'pc_'
      END || substring(encode(identity_digest, 'hex') FROM 1 FOR 60)
    )
  ),
  CONSTRAINT physical_mappings_scope_check CHECK (
    (
      level = 'n'
      AND logical_table = ''
      AND logical_column = ''
      AND scope_namespace_physical = physical_name
      AND scope_table_physical IS NULL
    )
    OR (
      level = 't'
      AND logical_column = ''
      AND scope_table_physical IS NOT NULL
      AND scope_table_physical = physical_name
      AND octet_length(scope_namespace_physical) = 63
      AND substring(scope_namespace_physical FROM 1 FOR 3) = 'pn_'
      AND coalesce(_oo.pm_lower_hex(substring(scope_namespace_physical FROM 4), 60), false)
    )
    OR (
      level = 'c'
      AND scope_table_physical IS NOT NULL
      AND octet_length(scope_namespace_physical) = 63
      AND substring(scope_namespace_physical FROM 1 FOR 3) = 'pn_'
      AND coalesce(_oo.pm_lower_hex(substring(scope_namespace_physical FROM 4), 60), false)
      AND octet_length(scope_table_physical) = 63
      AND substring(scope_table_physical FROM 1 FOR 3) = 'pt_'
      AND coalesce(_oo.pm_lower_hex(substring(scope_table_physical FROM 4), 60), false)
    )
  )
);

COMMENT ON TABLE _oo.physical_mappings IS
  'physical-mapping/v1. One pgident/v1 logical name in a source, config digest, and run generation binds to one stored PostgreSQL identifier of 63 bytes. Lookup reads the stored name.';

CREATE FUNCTION _oo.reject_physical_mapping_write()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog
AS $pm$
BEGIN
  IF TG_OP <> 'INSERT' THEN
    RAISE EXCEPTION 'direct write to physical mapping is closed' USING ERRCODE = 'OO014';
  END IF;
  IF current_setting('_oo.physical_mapping_writer', true) IS DISTINCT FROM 'register' THEN
    RAISE EXCEPTION 'direct write to physical mapping is closed' USING ERRCODE = 'OO014';
  END IF;
  RETURN NEW;
END;
$pm$;

CREATE TRIGGER physical_mappings_reject_write
  BEFORE INSERT OR UPDATE OR DELETE ON _oo.physical_mappings
  FOR EACH ROW
  EXECUTE FUNCTION _oo.reject_physical_mapping_write();

CREATE TRIGGER physical_mappings_reject_truncate
  BEFORE TRUNCATE ON _oo.physical_mappings
  FOR EACH STATEMENT
  EXECUTE FUNCTION _oo.reject_physical_mapping_write();

CREATE FUNCTION _oo.register_physical_mapping(
  p_source text,
  p_config_digest text,
  p_run_generation bigint,
  p_level text,
  p_logical_namespace text,
  p_logical_table text,
  p_logical_column text,
  p_identity_digest bytea
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $pm$
DECLARE
  v_level text;
  v_digest bytea;
  v_physical text;
  v_scope_ns text;
  v_scope_table text;
  v_lock bigint;
  v_existing _oo.physical_mappings%ROWTYPE;
  v_found text;
BEGIN
  IF current_setting('server_encoding') <> 'UTF8' THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_source IS NULL OR NOT _oo.pm_source_ok(p_source) THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_config_digest IS NULL OR NOT _oo.pm_config_ok(p_config_digest) THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_run_generation IS NULL OR p_run_generation < 1 THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_level IS NULL THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF convert_to(p_level, 'UTF8') = convert_to('namespace', 'UTF8') THEN
    v_level := 'n';
  ELSIF convert_to(p_level, 'UTF8') = convert_to('table', 'UTF8') THEN
    v_level := 't';
  ELSIF convert_to(p_level, 'UTF8') = convert_to('column', 'UTF8') THEN
    v_level := 'c';
  ELSE
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_logical_namespace IS NULL OR NOT _oo.pm_logical_ok(p_logical_namespace) THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF v_level = 'n' THEN
    IF p_logical_table IS NULL
       OR octet_length(convert_to(p_logical_table, 'UTF8')) <> 0
       OR p_logical_column IS NULL
       OR octet_length(convert_to(p_logical_column, 'UTF8')) <> 0
    THEN
      RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
    END IF;
  ELSIF v_level = 't' THEN
    IF p_logical_table IS NULL
       OR NOT _oo.pm_logical_ok(p_logical_table)
       OR p_logical_column IS NULL
       OR octet_length(convert_to(p_logical_column, 'UTF8')) <> 0
    THEN
      RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
    END IF;
  ELSE
    IF p_logical_table IS NULL
       OR NOT _oo.pm_logical_ok(p_logical_table)
       OR p_logical_column IS NULL
       OR NOT _oo.pm_logical_ok(p_logical_column)
    THEN
      RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
    END IF;
  END IF;
  IF p_identity_digest IS NULL OR octet_length(p_identity_digest) <> 32 THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  v_digest := _oo.pm_sha256(_oo.pm_payload(
    p_source,
    p_config_digest,
    p_run_generation,
    v_level,
    p_logical_namespace,
    p_logical_table,
    p_logical_column
  ));
  IF p_identity_digest <> v_digest THEN
    RAISE EXCEPTION 'physical mapping conflict' USING ERRCODE = 'OO022';
  END IF;
  v_physical := _oo.pm_physical(v_level, v_digest);
  IF v_physical IS NULL OR octet_length(v_physical) <> 63 THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF v_level = 'n' THEN
    v_scope_ns := v_physical;
    v_scope_table := NULL;
  ELSE
    v_scope_ns := _oo.pm_physical('n', _oo.pm_sha256(_oo.pm_payload(
      p_source, p_config_digest, p_run_generation, 'n', p_logical_namespace, '', ''
    )));
    IF v_level = 't' THEN
      v_scope_table := v_physical;
    ELSE
      v_scope_table := _oo.pm_physical('t', _oo.pm_sha256(_oo.pm_payload(
        p_source,
        p_config_digest,
        p_run_generation,
        't',
        p_logical_namespace,
        p_logical_table,
        ''
      )));
    END IF;
  END IF;
  v_lock := hashtextextended(
    p_source || '|' || p_config_digest || '|' || p_run_generation::text || '|' || v_level
      || '|' || p_logical_namespace || '|' || p_logical_table || '|' || p_logical_column,
    0::bigint
  );
  IF v_lock = 6404202605::bigint THEN
    v_lock := v_lock + 1;
  END IF;
  PERFORM pg_advisory_xact_lock(v_lock);
  SELECT existing.*
    INTO v_existing
  FROM _oo.physical_mappings AS existing
  WHERE existing.source = p_source
    AND existing.config_digest = p_config_digest
    AND existing.run_generation = p_run_generation
    AND existing.level = v_level
    AND existing.logical_namespace = p_logical_namespace
    AND existing.logical_table = p_logical_table
    AND existing.logical_column = p_logical_column;
  IF FOUND THEN
    IF v_existing.identity_digest = v_digest
       AND v_existing.physical_name = v_physical
       AND v_existing.algorithm = 'physical-mapping/v1'
       AND v_existing.scope_namespace_physical = v_scope_ns
       AND v_existing.scope_table_physical IS NOT DISTINCT FROM v_scope_table
    THEN
      RETURN 'replay';
    END IF;
    RAISE EXCEPTION 'physical mapping conflict' USING ERRCODE = 'OO022';
  END IF;
  IF v_level IN ('t', 'c') THEN
    SELECT existing.physical_name
      INTO v_found
    FROM _oo.physical_mappings AS existing
    WHERE existing.source = p_source
      AND existing.config_digest = p_config_digest
      AND existing.run_generation = p_run_generation
      AND existing.level = 'n'
      AND existing.logical_namespace = p_logical_namespace
      AND existing.logical_table = ''
      AND existing.logical_column = '';
    IF NOT FOUND THEN
      RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'OO024';
    END IF;
    IF v_found IS DISTINCT FROM v_scope_ns THEN
      RAISE EXCEPTION 'physical mapping conflict' USING ERRCODE = 'OO022';
    END IF;
  END IF;
  IF v_level = 'c' THEN
    SELECT existing.physical_name
      INTO v_found
    FROM _oo.physical_mappings AS existing
    WHERE existing.source = p_source
      AND existing.config_digest = p_config_digest
      AND existing.run_generation = p_run_generation
      AND existing.level = 't'
      AND existing.logical_namespace = p_logical_namespace
      AND existing.logical_table = p_logical_table
      AND existing.logical_column = '';
    IF NOT FOUND THEN
      RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'OO024';
    END IF;
    IF v_found IS DISTINCT FROM v_scope_table THEN
      RAISE EXCEPTION 'physical mapping conflict' USING ERRCODE = 'OO022';
    END IF;
  END IF;
  BEGIN
    PERFORM set_config('_oo.physical_mapping_writer', 'register', true);
    INSERT INTO _oo.physical_mappings (
      source,
      config_digest,
      run_generation,
      level,
      logical_namespace,
      logical_table,
      logical_column,
      identity_digest,
      physical_name,
      scope_namespace_physical,
      scope_table_physical,
      algorithm,
      registered_at
    ) VALUES (
      p_source,
      p_config_digest,
      p_run_generation,
      v_level,
      p_logical_namespace,
      p_logical_table,
      p_logical_column,
      v_digest,
      v_physical,
      v_scope_ns,
      v_scope_table,
      'physical-mapping/v1',
      clock_timestamp()
    );
    PERFORM set_config('_oo.physical_mapping_writer', '', true);
  EXCEPTION
    WHEN unique_violation THEN
      PERFORM set_config('_oo.physical_mapping_writer', '', true);
      SELECT existing.*
        INTO v_existing
      FROM _oo.physical_mappings AS existing
      WHERE existing.source = p_source
        AND existing.config_digest = p_config_digest
        AND existing.run_generation = p_run_generation
        AND existing.level = v_level
        AND existing.logical_namespace = p_logical_namespace
        AND existing.logical_table = p_logical_table
        AND existing.logical_column = p_logical_column;
      IF FOUND
         AND v_existing.identity_digest = v_digest
         AND v_existing.physical_name = v_physical
         AND v_existing.algorithm = 'physical-mapping/v1'
         AND v_existing.scope_namespace_physical = v_scope_ns
         AND v_existing.scope_table_physical IS NOT DISTINCT FROM v_scope_table
      THEN
        RETURN 'replay';
      END IF;
      RAISE EXCEPTION 'physical mapping conflict' USING ERRCODE = 'OO022';
    WHEN OTHERS THEN
      PERFORM set_config('_oo.physical_mapping_writer', '', true);
      RAISE;
  END;
  RETURN 'inserted';
END;
$pm$;

COMMENT ON FUNCTION _oo.register_physical_mapping(
  text, text, bigint, text, text, text, text, bytea
) IS
  'Binds one pgident/v1 logical name. An identical call returns replay and leaves registered_at in place. A different digest or a stored row that does not match raises OO022. Lookup is a separate function and does not hash.';

CREATE FUNCTION _oo.lookup_physical_mapping(
  p_source text,
  p_config_digest text,
  p_run_generation bigint,
  p_level text,
  p_logical_namespace text,
  p_logical_table text,
  p_logical_column text
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $pm$
DECLARE
  v_level text;
  v_name text;
BEGIN
  IF current_setting('server_encoding') <> 'UTF8' THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_source IS NULL OR NOT _oo.pm_source_ok(p_source) THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_config_digest IS NULL OR NOT _oo.pm_config_ok(p_config_digest) THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_run_generation IS NULL OR p_run_generation < 1 THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_level IS NULL THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF convert_to(p_level, 'UTF8') = convert_to('namespace', 'UTF8') THEN
    v_level := 'n';
  ELSIF convert_to(p_level, 'UTF8') = convert_to('table', 'UTF8') THEN
    v_level := 't';
  ELSIF convert_to(p_level, 'UTF8') = convert_to('column', 'UTF8') THEN
    v_level := 'c';
  ELSE
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF p_logical_namespace IS NULL OR NOT _oo.pm_logical_ok(p_logical_namespace) THEN
    RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
  END IF;
  IF v_level = 'n' THEN
    IF p_logical_table IS NULL
       OR octet_length(convert_to(p_logical_table, 'UTF8')) <> 0
       OR p_logical_column IS NULL
       OR octet_length(convert_to(p_logical_column, 'UTF8')) <> 0
    THEN
      RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
    END IF;
  ELSIF v_level = 't' THEN
    IF p_logical_table IS NULL
       OR NOT _oo.pm_logical_ok(p_logical_table)
       OR p_logical_column IS NULL
       OR octet_length(convert_to(p_logical_column, 'UTF8')) <> 0
    THEN
      RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
    END IF;
  ELSE
    IF p_logical_table IS NULL
       OR NOT _oo.pm_logical_ok(p_logical_table)
       OR p_logical_column IS NULL
       OR NOT _oo.pm_logical_ok(p_logical_column)
    THEN
      RAISE EXCEPTION 'physical mapping payload is closed' USING ERRCODE = '23514';
    END IF;
  END IF;
  SELECT existing.physical_name
    INTO v_name
  FROM _oo.physical_mappings AS existing
  WHERE existing.source = p_source
    AND existing.config_digest = p_config_digest
    AND existing.run_generation = p_run_generation
    AND existing.level = v_level
    AND existing.logical_namespace = p_logical_namespace
    AND existing.logical_table = p_logical_table
    AND existing.logical_column = p_logical_column;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'OO024';
  END IF;
  RETURN v_name;
END;
$pm$;

COMMENT ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) IS
  'Returns the stored PostgreSQL identifier for one logical identity. A missing row raises OO024. This function does not derive a name.';

REVOKE ALL ON TABLE _oo.physical_mappings FROM PUBLIC;
REVOKE ALL ON TABLE _oo.physical_mappings FROM oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pm_rotr(bigint, integer) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pm_sha256(bytea) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pm_source_ok(text) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pm_config_ok(text) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pm_lower_hex(text, integer) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pm_logical_ok(text) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pm_payload(text, text, bigint, text, text, text, text) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pm_physical(text, bytea) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.reject_physical_mapping_write() FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.register_physical_mapping(text, text, bigint, text, text, text, text, bytea) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.register_physical_mapping(text, text, bigint, text, text, text, text, bytea) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO oo_pipeline_runtime;

INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES ('target-fence-receipts/3', 'd046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3', 'expand', '0003_target_segment_preflight_expand.sql');
INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES ('physical-mapping/v1', '844de6f645f8f1ebf29d05118ae25d4669d8dc870eed1fa7697d4b98456eaf95', 'expand', '0004_physical_mapping.sql');
RESET ROLE;
COMMIT;