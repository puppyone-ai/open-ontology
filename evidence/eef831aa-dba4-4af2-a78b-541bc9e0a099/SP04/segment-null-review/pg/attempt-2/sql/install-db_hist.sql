BEGIN;
SET LOCAL lock_timeout = '5s';
SELECT pg_catalog.pg_advisory_xact_lock(6404202605);
-- target-bootstrap/1
-- Installer script. Run it in the target database as a role that can create
-- roles and revoke pg_catalog grants. oo_target_owner is the NOLOGIN schema
-- owner. oo_pipeline_runtime is the NOLOGIN pipeline principal. Neither role
-- is an Agent or Gateway credential. Membership is not granted here.



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
INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES
  ('target-bootstrap/1', '37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208', 'expand', '0000_bootstrap_roles.sql'),
  ('target-fence-receipts/1', '97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9', 'expand', '0001_target_fence_expand.sql');
RESET ROLE;
GRANT CONNECT ON DATABASE db_hist TO oo_a099_bootstrap;
COMMIT;
