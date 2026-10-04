-- full-import/v1 phase expand
-- Run as oo_target_owner after physical-mapping/v1. This file does not rewrite
-- 0000-0004, does not create roles, and does not create a publication.
-- Shadows are LOGGED tables in _oo. Their names come from stored physical
-- mappings. An Agent has no privilege on them. An open or retired shadow is
-- not a publication and is not granted to an Agent.
--
-- oo_target_owner is NOINHERIT. A security-definer function therefore cannot
-- see the caller's pg_stat_activity row unless that role inherits
-- pg_read_all_stats. install_full_import grants
--   GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE
-- before this script. SET FALSE blocks assuming that role. oo_pipeline_runtime
-- and an Agent login are not members. The membership only lets the fence read
-- backend_start. It does not replace the pid, owner, generation, or shard-lock
-- checks, and it is not a security proof.

DO $fi$
BEGIN
  IF pg_catalog.to_regclass('_oo.physical_mappings') IS NULL
     OR pg_catalog.to_regclass('_oo.apply_receipts') IS NULL
     OR pg_catalog.to_regclass('_oo.target_generations') IS NULL
  THEN
    RAISE EXCEPTION 'full import requires the production target schema'
      USING ERRCODE = 'OO002';
  END IF;
  IF pg_catalog.pg_has_role('oo_target_owner', 'pg_read_all_stats', 'usage') THEN
    NULL;
  ELSIF EXISTS (
    SELECT 1
    FROM pg_catalog.pg_roles
    WHERE pg_roles.rolname = current_user
      AND pg_roles.rolsuper
  ) THEN
    EXECUTE 'GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE';
  ELSE
    RAISE EXCEPTION 'oo_target_owner must inherit pg_read_all_stats to read the caller backend_start'
      USING ERRCODE = '42501';
  END IF;
END
$fi$;

CREATE FUNCTION _oo.full_import_physical_ok(p_name text)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
SECURITY INVOKER
SET search_path = pg_catalog
AS $fi$
  SELECT p_name IS NOT NULL
    AND octet_length(p_name) = 63
    AND p_name ~ '^(pn|pt|pc)_[0-9a-f]{60}$';
$fi$;

CREATE FUNCTION _oo.full_import_type_sql(p_oid bigint, p_typmod integer)
RETURNS text
LANGUAGE plpgsql
IMMUTABLE
SECURITY INVOKER
SET search_path = pg_catalog
AS $fi$
DECLARE
  v_precision integer;
  v_scale integer;
  v_length integer;
BEGIN
  IF p_oid = 16 THEN RETURN 'bool'; END IF;
  IF p_oid = 21 THEN RETURN 'int2'; END IF;
  IF p_oid = 23 THEN RETURN 'int4'; END IF;
  IF p_oid = 20 THEN RETURN 'int8'; END IF;
  IF p_oid = 700 THEN RETURN 'float4'; END IF;
  IF p_oid = 701 THEN RETURN 'float8'; END IF;
  IF p_oid = 25 THEN RETURN 'text'; END IF;
  IF p_oid = 17 THEN RETURN 'bytea'; END IF;
  IF p_oid = 2950 THEN RETURN 'uuid'; END IF;
  IF p_oid = 1082 THEN RETURN 'date'; END IF;
  IF p_oid = 1083 THEN RETURN 'time without time zone'; END IF;
  IF p_oid = 1114 THEN RETURN 'timestamp without time zone'; END IF;
  IF p_oid = 1184 THEN RETURN 'timestamp with time zone'; END IF;
  IF p_oid = 114 THEN RETURN 'json'; END IF;
  IF p_oid = 1700 THEN
    IF p_typmod IS NULL OR p_typmod < 0 THEN
      RETURN 'numeric';
    END IF;
    v_precision := ((p_typmod - 4) >> 16) & 65535;
    v_scale := (p_typmod - 4) & 65535;
    IF v_precision < 1 OR v_precision > 1000 OR v_scale > 1000 OR v_precision < v_scale THEN
      RAISE EXCEPTION 'closed numeric typmod' USING ERRCODE = '22023';
    END IF;
    RETURN format('numeric(%s,%s)', v_precision, v_scale);
  END IF;
  IF p_oid = 1043 OR p_oid = 1042 THEN
    IF p_typmod IS NULL OR p_typmod < 0 THEN
      IF p_oid = 1043 THEN
        RETURN 'character varying';
      END IF;
      RETURN 'character';
    END IF;
    v_length := p_typmod - 4;
    IF v_length < 1 OR v_length > 10485760 THEN
      RAISE EXCEPTION 'closed character typmod' USING ERRCODE = '22023';
    END IF;
    IF p_oid = 1043 THEN
      RETURN format('character varying(%s)', v_length);
    END IF;
    RETURN format('character(%s)', v_length);
  END IF;
  RAISE EXCEPTION 'closed copy type' USING ERRCODE = '22023';
END;
$fi$;

CREATE TABLE _oo.full_import_session (
  shard_id text PRIMARY KEY,
  owner_token uuid NOT NULL,
  generation bigint NOT NULL,
  backend_pid integer NOT NULL,
  backend_start text NOT NULL,
  CONSTRAINT full_import_session_shard_check CHECK (shard_id ~ '^[a-z][a-z0-9_.:-]{0,127}$'),
  CONSTRAINT full_import_session_generation_check CHECK (generation >= 1),
  CONSTRAINT full_import_session_start_check CHECK (octet_length(backend_start) BETWEEN 1 AND 128)
);

COMMENT ON TABLE _oo.full_import_session IS
  'Backend identity for one adopted shard. backend_start comes from pg_stat_activity and is not a column of target_generations.';

CREATE TABLE _oo.full_import_run (
  run_id uuid PRIMARY KEY,
  shard_id text NOT NULL,
  owner_token uuid NOT NULL,
  target_generation bigint NOT NULL,
  source_id text NOT NULL,
  source_generation bigint NOT NULL,
  config_digest text NOT NULL,
  type_digest text NOT NULL,
  input_digest text NOT NULL,
  schema_digest text NOT NULL,
  state text NOT NULL,
  opened_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  sealed_at timestamptz,
  retired_at timestamptz,
  CONSTRAINT full_import_run_shard_check CHECK (shard_id ~ '^[a-z][a-z0-9_.:-]{0,127}$'),
  CONSTRAINT full_import_run_source_check CHECK (source_id ~ '^[a-z][a-z0-9_]{0,62}$'),
  CONSTRAINT full_import_run_generations_check CHECK (
    target_generation >= 1 AND source_generation >= 1
  ),
  CONSTRAINT full_import_run_digest_check CHECK (
    config_digest ~ '^[0-9a-f]{64}$'
    AND type_digest ~ '^[0-9a-f]{64}$'
    AND input_digest ~ '^[0-9a-f]{64}$'
    AND schema_digest ~ '^[0-9a-f]{64}$'
  ),
  CONSTRAINT full_import_run_state_check CHECK (state IN ('open', 'awaiting_publish', 'retired')),
  CONSTRAINT full_import_run_seal_check CHECK (
    (state = 'open' AND sealed_at IS NULL AND retired_at IS NULL)
    OR (state = 'awaiting_publish' AND sealed_at IS NOT NULL AND retired_at IS NULL)
    OR (state = 'retired' AND sealed_at IS NULL AND retired_at IS NOT NULL)
  )
);

COMMENT ON TABLE _oo.full_import_run IS
  'Unpublished full-import run. awaiting_publish is not a publication and grants nothing.';

CREATE TABLE _oo.full_import_table (
  run_id uuid NOT NULL REFERENCES _oo.full_import_run (run_id),
  table_index integer NOT NULL,
  schema_ident text NOT NULL,
  table_ident text NOT NULL,
  physical_name text NOT NULL,
  exhausted boolean NOT NULL DEFAULT false,
  applied_rows bigint NOT NULL DEFAULT 0,
  applied_pos numeric(38, 0),
  canonical_digest text,
  PRIMARY KEY (run_id, table_index),
  CONSTRAINT full_import_table_index_check CHECK (table_index >= 0),
  CONSTRAINT full_import_table_physical_check CHECK (
    physical_name ~ '^pt_[0-9a-f]{60}$' AND octet_length(physical_name) = 63
  ),
  CONSTRAINT full_import_table_rows_check CHECK (applied_rows >= 0),
  CONSTRAINT full_import_table_digest_check CHECK (
    canonical_digest IS NULL OR canonical_digest ~ '^[0-9a-f]{64}$'
  ),
  CONSTRAINT full_import_table_coverage_check CHECK (
    (NOT exhausted AND canonical_digest IS NULL)
    OR (exhausted AND canonical_digest IS NOT NULL AND applied_pos IS NOT NULL)
  )
);

CREATE TABLE _oo.full_import_quality (
  quality_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  run_id uuid NOT NULL,
  physical_name text NOT NULL,
  batch_id text NOT NULL,
  kind text NOT NULL,
  reason text,
  value_len bigint,
  attnum integer,
  row_index bigint,
  key_digest text,
  input_digest text NOT NULL,
  accepted_rows bigint,
  rejected_rows bigint,
  payload_digest text NOT NULL,
  CONSTRAINT full_import_quality_kind_check CHECK (kind IN ('batch', 'reject')),
  CONSTRAINT full_import_quality_reason_check CHECK (
    reason IS NULL OR reason ~ '^[A-Za-z0-9_.:-]{1,80}$'
  ),
  CONSTRAINT full_import_quality_digest_check CHECK (
    input_digest ~ '^[0-9a-f]{64}$'
    AND payload_digest ~ '^[0-9a-f]{64}$'
    AND (key_digest IS NULL OR key_digest ~ '^[0-9a-f]{64}$')
  ),
  CONSTRAINT full_import_quality_batch_check CHECK (batch_id ~ '^[A-Za-z0-9_.:-]{1,128}$'),
  CONSTRAINT full_import_quality_physical_check CHECK (
    physical_name ~ '^pt_[0-9a-f]{60}$' AND octet_length(physical_name) = 63
  )
);

COMMENT ON TABLE _oo.full_import_quality IS
  'Masked quality. There is no raw value column. input_digest binds the row to the source cut.';

CREATE FUNCTION _oo.full_import_assert_bound(
  p_shard text,
  p_owner uuid,
  p_generation bigint
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_pid integer;
  v_start text;
BEGIN
  PERFORM _oo._load_held_generation(p_shard, p_owner, p_generation);
  PERFORM pg_catalog.set_config('_oo.writer', '', true);
  v_pid := pg_catalog.pg_backend_pid();
  SELECT activity.backend_start::text
    INTO v_start
  FROM pg_catalog.pg_stat_activity AS activity
  WHERE activity.pid = v_pid;
  IF NOT EXISTS (
    SELECT 1
    FROM _oo.full_import_session AS session_row
    WHERE session_row.shard_id = p_shard
      AND session_row.owner_token = p_owner
      AND session_row.generation = p_generation
      AND session_row.backend_pid = v_pid
      AND session_row.backend_start = v_start
  ) THEN
    RAISE EXCEPTION 'target backend identity is not bound' USING ERRCODE = 'OO018';
  END IF;
END;
$fi$;

COMMENT ON FUNCTION _oo.full_import_assert_bound(text, uuid, bigint) IS
  'Proves this backend holds the shard lock and the bound session identity. Clears the receipt writer gate before returning.';

CREATE FUNCTION _oo.full_import_guard_session()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_start text;
BEGIN
  IF TG_OP = 'DELETE' OR TG_OP = 'TRUNCATE' THEN
    RAISE EXCEPTION 'full import session cannot be removed' USING ERRCODE = 'OO014';
  END IF;
  PERFORM _oo._load_held_generation(NEW.shard_id, NEW.owner_token, NEW.generation);
  PERFORM pg_catalog.set_config('_oo.writer', '', true);
  SELECT activity.backend_start::text
    INTO v_start
  FROM pg_catalog.pg_stat_activity AS activity
  WHERE activity.pid = pg_catalog.pg_backend_pid();
  IF NEW.backend_pid IS DISTINCT FROM pg_catalog.pg_backend_pid()
     OR NEW.backend_start IS DISTINCT FROM v_start
  THEN
    RAISE EXCEPTION 'target backend identity is not bound' USING ERRCODE = 'OO018';
  END IF;
  RETURN NEW;
END;
$fi$;

CREATE TRIGGER full_import_session_guard
  BEFORE INSERT OR UPDATE OR DELETE ON _oo.full_import_session
  FOR EACH ROW
  EXECUTE FUNCTION _oo.full_import_guard_session();

CREATE TRIGGER full_import_session_truncate
  BEFORE TRUNCATE ON _oo.full_import_session
  FOR EACH STATEMENT
  EXECUTE FUNCTION _oo.full_import_guard_session();

CREATE FUNCTION _oo.full_import_guard_run()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_shard text;
  v_owner uuid;
  v_generation bigint;
BEGIN
  IF TG_OP = 'DELETE' OR TG_OP = 'TRUNCATE' THEN
    RAISE EXCEPTION 'full import run cannot be removed' USING ERRCODE = 'OO014';
  END IF;
  v_shard := NEW.shard_id;
  v_owner := NEW.owner_token;
  v_generation := NEW.target_generation;
  PERFORM _oo.full_import_assert_bound(v_shard, v_owner, v_generation);
  RETURN NEW;
END;
$fi$;

CREATE TRIGGER full_import_run_guard
  BEFORE INSERT OR UPDATE OR DELETE ON _oo.full_import_run
  FOR EACH ROW
  EXECUTE FUNCTION _oo.full_import_guard_run();

CREATE TRIGGER full_import_run_truncate
  BEFORE TRUNCATE ON _oo.full_import_run
  FOR EACH STATEMENT
  EXECUTE FUNCTION _oo.full_import_guard_run();

CREATE FUNCTION _oo.full_import_guard_child()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_run _oo.full_import_run%ROWTYPE;
  v_run_id uuid;
BEGIN
  IF TG_OP = 'DELETE' OR TG_OP = 'TRUNCATE' THEN
    RAISE EXCEPTION 'full import catalog cannot be removed' USING ERRCODE = 'OO014';
  END IF;
  v_run_id := NEW.run_id;
  SELECT run_row.*
    INTO v_run
  FROM _oo.full_import_run AS run_row
  WHERE run_row.run_id = v_run_id;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'full import run is not open' USING ERRCODE = 'OO031';
  END IF;
  PERFORM _oo.full_import_assert_bound(v_run.shard_id, v_run.owner_token, v_run.target_generation);
  IF TG_OP = 'INSERT' AND v_run.state <> 'open' THEN
    RAISE EXCEPTION 'full import run is not open' USING ERRCODE = 'OO031';
  END IF;
  RETURN NEW;
END;
$fi$;

CREATE TRIGGER full_import_table_guard
  BEFORE INSERT OR UPDATE OR DELETE ON _oo.full_import_table
  FOR EACH ROW
  EXECUTE FUNCTION _oo.full_import_guard_child();

CREATE TRIGGER full_import_table_truncate
  BEFORE TRUNCATE ON _oo.full_import_table
  FOR EACH STATEMENT
  EXECUTE FUNCTION _oo.full_import_guard_child();

CREATE TRIGGER full_import_quality_guard
  BEFORE INSERT OR UPDATE OR DELETE ON _oo.full_import_quality
  FOR EACH ROW
  EXECUTE FUNCTION _oo.full_import_guard_child();

CREATE TRIGGER full_import_quality_truncate
  BEFORE TRUNCATE ON _oo.full_import_quality
  FOR EACH STATEMENT
  EXECUTE FUNCTION _oo.full_import_guard_child();

CREATE FUNCTION _oo.full_import_guard_shadow()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_run _oo.full_import_run%ROWTYPE;
BEGIN
  SELECT run_row.*
    INTO v_run
  FROM _oo.full_import_table AS table_row
  JOIN _oo.full_import_run AS run_row ON run_row.run_id = table_row.run_id
  WHERE table_row.physical_name = TG_TABLE_NAME
    AND run_row.state = 'open';
  IF NOT FOUND THEN
    RAISE EXCEPTION 'unpublished shadow is not open' USING ERRCODE = 'OO031';
  END IF;
  PERFORM _oo.full_import_assert_bound(v_run.shard_id, v_run.owner_token, v_run.target_generation);
  IF TG_OP <> 'INSERT' THEN
    RAISE EXCEPTION 'unpublished shadow is append-only' USING ERRCODE = 'OO014';
  END IF;
  RETURN NULL;
END;
$fi$;

CREATE FUNCTION _oo.full_import_bind_session(
  p_shard text,
  p_owner uuid,
  p_generation bigint
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_pid integer;
  v_start text;
BEGIN
  PERFORM _oo._load_held_generation(p_shard, p_owner, p_generation);
  PERFORM pg_catalog.set_config('_oo.writer', '', true);
  v_pid := pg_catalog.pg_backend_pid();
  SELECT activity.backend_start::text
    INTO v_start
  FROM pg_catalog.pg_stat_activity AS activity
  WHERE activity.pid = v_pid;
  IF v_start IS NULL THEN
    RAISE EXCEPTION 'target backend identity is not bound' USING ERRCODE = 'OO018';
  END IF;
  INSERT INTO _oo.full_import_session AS session_row (
    shard_id, owner_token, generation, backend_pid, backend_start
  ) VALUES (
    p_shard, p_owner, p_generation, v_pid, v_start
  )
  ON CONFLICT (shard_id) DO UPDATE
    SET owner_token = EXCLUDED.owner_token,
        generation = EXCLUDED.generation,
        backend_pid = EXCLUDED.backend_pid,
        backend_start = EXCLUDED.backend_start;
END;
$fi$;

CREATE FUNCTION _oo.full_import_open_run(
  p_shard text,
  p_owner uuid,
  p_generation bigint,
  p_source text,
  p_source_generation bigint,
  p_config_digest text,
  p_type_digest text,
  p_input_digest text,
  p_schema_digest text,
  p_schema_idents text[],
  p_table_idents text[],
  p_physical_names text[]
)
RETURNS uuid
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_run uuid;
  v_index integer;
  v_found text;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_generation);
  PERFORM _oo._require_digest(p_config_digest, 'config_digest');
  PERFORM _oo._require_digest(p_type_digest, 'type_digest');
  PERFORM _oo._require_digest(p_input_digest, 'input_digest');
  PERFORM _oo._require_digest(p_schema_digest, 'schema_digest');
  IF p_source IS NULL OR p_source !~ '^[a-z][a-z0-9_]{0,62}$' THEN
    RAISE EXCEPTION 'closed source identity' USING ERRCODE = '23514';
  END IF;
  IF p_source_generation IS NULL OR p_source_generation < 1 THEN
    RAISE EXCEPTION 'closed source generation' USING ERRCODE = '22003';
  END IF;
  IF p_schema_idents IS NULL
     OR cardinality(p_schema_idents) < 1
     OR cardinality(p_schema_idents) <> cardinality(p_table_idents)
     OR cardinality(p_schema_idents) <> cardinality(p_physical_names)
  THEN
    RAISE EXCEPTION 'closed selected tables' USING ERRCODE = '22023';
  END IF;
  v_run := pg_catalog.gen_random_uuid();
  INSERT INTO _oo.full_import_run (
    run_id, shard_id, owner_token, target_generation, source_id, source_generation,
    config_digest, type_digest, input_digest, schema_digest, state
  ) VALUES (
    v_run, p_shard, p_owner, p_generation, p_source, p_source_generation,
    p_config_digest, p_type_digest, p_input_digest, p_schema_digest, 'open'
  );
  FOR v_index IN 1..cardinality(p_schema_idents) LOOP
    IF NOT _oo.full_import_physical_ok(p_physical_names[v_index])
       OR p_physical_names[v_index] !~ '^pt_'
    THEN
      RAISE EXCEPTION 'physical mapping name is closed' USING ERRCODE = '23514';
    END IF;
    v_found := _oo.lookup_physical_mapping(
      p_source,
      p_config_digest,
      p_source_generation,
      'table',
      p_schema_idents[v_index],
      p_table_idents[v_index],
      ''
    );
    IF v_found IS DISTINCT FROM p_physical_names[v_index] THEN
      RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'OO024';
    END IF;
    INSERT INTO _oo.full_import_table (
      run_id, table_index, schema_ident, table_ident, physical_name
    ) VALUES (
      v_run, v_index - 1, p_schema_idents[v_index], p_table_idents[v_index], v_found
    );
  END LOOP;
  RETURN v_run;
END;
$fi$;

CREATE FUNCTION _oo.full_import_prepare_shadow(
  p_shard text,
  p_owner uuid,
  p_generation bigint,
  p_run uuid,
  p_table_index integer,
  p_source text,
  p_config_digest text,
  p_source_generation bigint,
  p_schema_ident text,
  p_table_ident text,
  p_column_idents text[],
  p_oids bigint[],
  p_typmods integer[],
  p_not_null boolean[]
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_table text;
  v_stored text;
  v_column text;
  v_type text;
  v_cols text := '';
  v_index integer;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_generation);
  IF p_column_idents IS NULL
     OR cardinality(p_column_idents) < 1
     OR cardinality(p_column_idents) <> cardinality(p_oids)
     OR cardinality(p_oids) <> cardinality(p_typmods)
     OR cardinality(p_typmods) <> cardinality(p_not_null)
  THEN
    RAISE EXCEPTION 'closed shadow columns' USING ERRCODE = '22023';
  END IF;
  v_table := _oo.lookup_physical_mapping(
    p_source, p_config_digest, p_source_generation, 'table',
    p_schema_ident, p_table_ident, ''
  );
  IF NOT _oo.full_import_physical_ok(v_table) OR v_table !~ '^pt_' THEN
    RAISE EXCEPTION 'physical mapping name is closed' USING ERRCODE = '23514';
  END IF;
  SELECT table_row.physical_name
    INTO v_stored
  FROM _oo.full_import_table AS table_row
  JOIN _oo.full_import_run AS run_row ON run_row.run_id = table_row.run_id
  WHERE table_row.run_id = p_run
    AND table_row.table_index = p_table_index
    AND table_row.schema_ident = p_schema_ident
    AND table_row.table_ident = p_table_ident
    AND run_row.state = 'open'
    AND run_row.shard_id = p_shard
    AND run_row.owner_token = p_owner
    AND run_row.target_generation = p_generation;
  IF v_stored IS DISTINCT FROM v_table THEN
    RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'OO024';
  END IF;
  FOR v_index IN 1..cardinality(p_column_idents) LOOP
    v_column := _oo.lookup_physical_mapping(
      p_source, p_config_digest, p_source_generation, 'column',
      p_schema_ident, p_table_ident, p_column_idents[v_index]
    );
    IF NOT _oo.full_import_physical_ok(v_column) OR v_column !~ '^pc_' THEN
      RAISE EXCEPTION 'physical mapping name is closed' USING ERRCODE = '23514';
    END IF;
    v_type := _oo.full_import_type_sql(p_oids[v_index], p_typmods[v_index]);
    IF v_cols <> '' THEN
      v_cols := v_cols || ', ';
    END IF;
    v_cols := v_cols || format(
      '%I %s%s',
      v_column,
      v_type,
      CASE WHEN p_not_null[v_index] THEN ' NOT NULL' ELSE '' END
    );
  END LOOP;
  EXECUTE format(
    'CREATE TABLE _oo.%I ("_oo_pos" numeric(38,0) NOT NULL, %s)',
    v_table,
    v_cols
  );
  EXECUTE format(
    'CREATE TRIGGER full_import_shadow_insert BEFORE INSERT ON _oo.%I FOR EACH STATEMENT EXECUTE FUNCTION _oo.full_import_guard_shadow()',
    v_table
  );
  EXECUTE format(
    'CREATE TRIGGER full_import_shadow_mutate BEFORE UPDATE OR DELETE ON _oo.%I FOR EACH ROW EXECUTE FUNCTION _oo.full_import_guard_shadow()',
    v_table
  );
  EXECUTE format(
    'CREATE TRIGGER full_import_shadow_truncate BEFORE TRUNCATE ON _oo.%I FOR EACH STATEMENT EXECUTE FUNCTION _oo.full_import_guard_shadow()',
    v_table
  );
  EXECUTE format(
    'CREATE CONSTRAINT TRIGGER full_import_shadow_commit AFTER INSERT ON _oo.%I DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION _oo.full_import_guard_shadow()',
    v_table
  );
  EXECUTE format('REVOKE ALL ON TABLE _oo.%I FROM PUBLIC', v_table);
  EXECUTE format('GRANT INSERT ON TABLE _oo.%I TO oo_pipeline_runtime', v_table);
  RETURN v_table;
END;
$fi$;

CREATE FUNCTION _oo.full_import_stage_decision(
  p_shard text,
  p_owner uuid,
  p_generation bigint,
  p_source text,
  p_physical text,
  p_batch_id text,
  p_payload_digest text,
  p_config_digest text,
  p_type_digest text,
  p_hook_digest text,
  p_schema_digest text,
  p_applied_pos numeric
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_existing _oo.apply_receipts%ROWTYPE;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_generation);
  PERFORM _oo._require_digest(p_payload_digest, 'payload_digest');
  IF p_physical IS NULL OR NOT _oo.full_import_physical_ok(p_physical) OR p_physical !~ '^pt_' THEN
    RAISE EXCEPTION 'physical mapping name is closed' USING ERRCODE = '23514';
  END IF;
  IF p_batch_id IS NULL OR p_batch_id !~ '^[A-Za-z0-9_.:-]{1,128}$' THEN
    RAISE EXCEPTION 'closed batch identity' USING ERRCODE = '23514';
  END IF;
  SELECT existing.*
    INTO v_existing
  FROM _oo.apply_receipts AS existing
  WHERE existing.source = p_source
    AND existing.table_generation = p_physical
    AND existing.txn_or_batch_id = p_batch_id;
  IF NOT FOUND THEN
    RETURN 'apply';
  END IF;
  IF v_existing.payload_digest = p_payload_digest
     AND v_existing.config_digest = p_config_digest
     AND v_existing.type_digest = p_type_digest
     AND v_existing.hook_digest = p_hook_digest
     AND v_existing.schema_digest = p_schema_digest
     AND v_existing.applied_pos = p_applied_pos
     AND v_existing.position_codec = 'snapshot-zero/v1'
     AND v_existing.position_lineage = 'full-import.snapshot'
     AND v_existing.source_epoch = 0
     AND v_existing.partial_txn = false
     AND v_existing.txn_complete = true
     AND v_existing.cross_table_consistent = false
     AND v_existing.segment_index IS NULL
     AND v_existing.segment_count IS NULL
     AND octet_length(v_existing.segment_bitmap) = 0
  THEN
    RETURN 'replay';
  END IF;
  RAISE EXCEPTION 'apply receipt identity conflict' USING ERRCODE = 'OO010';
END;
$fi$;

CREATE FUNCTION _oo.full_import_write_batch(
  p_shard text,
  p_owner uuid,
  p_generation bigint,
  p_run uuid,
  p_table_index integer,
  p_source text,
  p_physical text,
  p_batch_id text,
  p_payload_digest text,
  p_config_digest text,
  p_type_digest text,
  p_hook_digest text,
  p_schema_digest text,
  p_input_digest text,
  p_applied_pos numeric,
  p_accepted bigint,
  p_rejected bigint,
  p_reasons text[],
  p_value_lens bigint[],
  p_attnums integer[],
  p_row_indexes bigint[],
  p_key_digests text[]
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_decision text;
  v_receipt text;
  v_index integer;
  v_key text;
BEGIN
  v_decision := _oo.full_import_stage_decision(
    p_shard, p_owner, p_generation, p_source, p_physical, p_batch_id, p_payload_digest,
    p_config_digest, p_type_digest, p_hook_digest, p_schema_digest, p_applied_pos
  );
  IF v_decision = 'replay' THEN
    RETURN 'replay';
  END IF;
  IF p_accepted IS NULL OR p_accepted < 0 OR p_rejected IS NULL OR p_rejected < 0 THEN
    RAISE EXCEPTION 'closed quality counts' USING ERRCODE = '22003';
  END IF;
  IF p_reasons IS NULL
     OR cardinality(p_reasons) <> cardinality(p_value_lens)
     OR cardinality(p_reasons) <> cardinality(p_attnums)
     OR cardinality(p_reasons) <> cardinality(p_row_indexes)
     OR cardinality(p_reasons) <> cardinality(p_key_digests)
  THEN
    RAISE EXCEPTION 'closed quality rows' USING ERRCODE = '22023';
  END IF;
  INSERT INTO _oo.full_import_quality (
    run_id, physical_name, batch_id, kind, reason, value_len, attnum, row_index,
    key_digest, input_digest, accepted_rows, rejected_rows, payload_digest
  ) VALUES (
    p_run, p_physical, p_batch_id, 'batch', NULL, NULL, NULL, NULL,
    NULL, p_input_digest, p_accepted, p_rejected, p_payload_digest
  );
  FOR v_index IN 1..cardinality(p_reasons) LOOP
    v_key := NULLIF(p_key_digests[v_index], '');
    INSERT INTO _oo.full_import_quality (
      run_id, physical_name, batch_id, kind, reason, value_len, attnum, row_index,
      key_digest, input_digest, accepted_rows, rejected_rows, payload_digest
    ) VALUES (
      p_run, p_physical, p_batch_id, 'reject', p_reasons[v_index], p_value_lens[v_index],
      p_attnums[v_index], p_row_indexes[v_index], v_key, p_input_digest,
      NULL, NULL, p_payload_digest
    );
  END LOOP;
  UPDATE _oo.full_import_table
  SET applied_rows = applied_rows + p_accepted,
      applied_pos = p_applied_pos
  WHERE run_id = p_run
    AND table_index = p_table_index
    AND physical_name = p_physical;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'full import run is not open' USING ERRCODE = 'OO031';
  END IF;
  v_receipt := _oo.commit_apply_receipt(
    p_shard,
    p_owner,
    p_generation,
    p_source,
    p_physical,
    p_batch_id,
    p_config_digest,
    p_type_digest,
    p_hook_digest,
    p_schema_digest,
    p_payload_digest,
    'snapshot-zero/v1',
    'full-import.snapshot',
    0,
    p_applied_pos,
    false,
    false,
    true,
    NULL,
    NULL,
    ''::bytea
  );
  PERFORM pg_catalog.set_config('_oo.writer', '', true);
  RETURN v_receipt;
END;
$fi$;

CREATE FUNCTION _oo.full_import_note_table(
  p_shard text,
  p_owner uuid,
  p_generation bigint,
  p_run uuid,
  p_table_index integer,
  p_rows bigint,
  p_digest text,
  p_source text
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_physical text;
  v_applied bigint;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_generation);
  PERFORM _oo._require_digest(p_digest, 'canonical_digest');
  SELECT physical_name, applied_rows
    INTO v_physical, v_applied
  FROM _oo.full_import_table
  WHERE run_id = p_run AND table_index = p_table_index;
  IF NOT FOUND OR v_applied IS DISTINCT FROM p_rows THEN
    RAISE EXCEPTION 'applied coverage does not match the run' USING ERRCODE = 'OO030';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM _oo.apply_receipts AS receipt
    WHERE receipt.source = p_source
      AND receipt.table_generation = v_physical
  ) THEN
    RAISE EXCEPTION 'applied coverage is absent' USING ERRCODE = 'OO030';
  END IF;
  UPDATE _oo.full_import_table
  SET exhausted = true,
      canonical_digest = p_digest,
      applied_pos = COALESCE(applied_pos, 0)
  WHERE run_id = p_run
    AND table_index = p_table_index
    AND NOT exhausted;
END;
$fi$;

CREATE FUNCTION _oo.full_import_seal_run(
  p_shard text,
  p_owner uuid,
  p_generation bigint,
  p_run uuid,
  p_source text,
  p_source_generation bigint,
  p_config_digest text,
  p_type_digest text,
  p_input_digest text,
  p_schema_digest text,
  p_schema_idents text[],
  p_table_idents text[]
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_run _oo.full_import_run%ROWTYPE;
  v_row record;
  v_count integer;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_generation);
  SELECT run_row.*
    INTO v_run
  FROM _oo.full_import_run AS run_row
  WHERE run_row.run_id = p_run
  FOR UPDATE;
  IF NOT FOUND OR v_run.state <> 'open' THEN
    RAISE EXCEPTION 'full import run is not open' USING ERRCODE = 'OO030';
  END IF;
  IF v_run.shard_id IS DISTINCT FROM p_shard
     OR v_run.owner_token IS DISTINCT FROM p_owner
     OR v_run.target_generation IS DISTINCT FROM p_generation
     OR v_run.source_id IS DISTINCT FROM p_source
     OR v_run.source_generation IS DISTINCT FROM p_source_generation
     OR v_run.config_digest IS DISTINCT FROM p_config_digest
     OR v_run.type_digest IS DISTINCT FROM p_type_digest
     OR v_run.input_digest IS DISTINCT FROM p_input_digest
     OR v_run.schema_digest IS DISTINCT FROM p_schema_digest
  THEN
    RAISE EXCEPTION 'full import binding does not match the run' USING ERRCODE = 'OO030';
  END IF;
  SELECT count(*) INTO v_count FROM _oo.full_import_table WHERE run_id = p_run;
  IF p_schema_idents IS NULL
     OR v_count <> cardinality(p_schema_idents)
     OR cardinality(p_schema_idents) <> cardinality(p_table_idents)
  THEN
    RAISE EXCEPTION 'full import binding does not match the run' USING ERRCODE = 'OO030';
  END IF;
  FOR v_row IN
    SELECT table_index, schema_ident, table_ident, exhausted
    FROM _oo.full_import_table
    WHERE run_id = p_run
    ORDER BY table_index
  LOOP
    IF NOT v_row.exhausted
       OR v_row.schema_ident IS DISTINCT FROM p_schema_idents[v_row.table_index + 1]
       OR v_row.table_ident IS DISTINCT FROM p_table_idents[v_row.table_index + 1]
    THEN
      RAISE EXCEPTION 'applied coverage does not match the run' USING ERRCODE = 'OO030';
    END IF;
  END LOOP;
  UPDATE _oo.full_import_run
  SET state = 'awaiting_publish',
      sealed_at = clock_timestamp()
  WHERE run_id = p_run
    AND state = 'open';
  IF NOT FOUND THEN
    RAISE EXCEPTION 'full import run is not open' USING ERRCODE = 'OO030';
  END IF;
  RETURN 'awaiting_publish';
END;
$fi$;

CREATE FUNCTION _oo.full_import_retire_run(
  p_shard text,
  p_owner uuid,
  p_generation bigint,
  p_run uuid
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_state text;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_generation);
  SELECT state
    INTO v_state
  FROM _oo.full_import_run
  WHERE run_id = p_run
    AND shard_id = p_shard
    AND owner_token = p_owner
    AND target_generation = p_generation
  FOR UPDATE;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'full import run is not open' USING ERRCODE = 'OO031';
  END IF;
  IF v_state = 'retired' THEN
    RETURN;
  END IF;
  IF v_state <> 'open' THEN
    RAISE EXCEPTION 'full import run is not open' USING ERRCODE = 'OO030';
  END IF;
  UPDATE _oo.full_import_run
  SET state = 'retired',
      retired_at = clock_timestamp()
  WHERE run_id = p_run
    AND state = 'open';
END;
$fi$;

CREATE FUNCTION _oo.full_import_recover(
  p_source text,
  p_physical text,
  p_batch_id text,
  p_payload_digest text
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_digest text;
BEGIN
  PERFORM _oo._require_digest(p_payload_digest, 'payload_digest');
  SELECT receipt.payload_digest
    INTO v_digest
  FROM _oo.apply_receipts AS receipt
  WHERE receipt.source = p_source
    AND receipt.table_generation = p_physical
    AND receipt.txn_or_batch_id = p_batch_id;
  IF NOT FOUND THEN
    RETURN 'absent';
  END IF;
  IF v_digest = p_payload_digest THEN
    RETURN 'replay';
  END IF;
  RAISE EXCEPTION 'apply receipt identity conflict' USING ERRCODE = 'OO010';
END;
$fi$;

CREATE FUNCTION _oo.full_import_record_refusal(
  p_shard text,
  p_owner uuid,
  p_generation bigint,
  p_run uuid,
  p_physical text,
  p_batch_id text,
  p_input_digest text,
  p_payload_digest text,
  p_reasons text[],
  p_value_lens bigint[],
  p_attnums integer[],
  p_row_indexes bigint[],
  p_key_digests text[]
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_index integer;
  v_key text;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_generation);
  PERFORM _oo._require_digest(p_input_digest, 'input_digest');
  PERFORM _oo._require_digest(p_payload_digest, 'payload_digest');
  IF p_reasons IS NULL
     OR cardinality(p_reasons) < 1
     OR cardinality(p_reasons) <> cardinality(p_value_lens)
     OR cardinality(p_reasons) <> cardinality(p_attnums)
     OR cardinality(p_reasons) <> cardinality(p_row_indexes)
     OR cardinality(p_reasons) <> cardinality(p_key_digests)
  THEN
    RAISE EXCEPTION 'closed quality rows' USING ERRCODE = '22023';
  END IF;
  FOR v_index IN 1..cardinality(p_reasons) LOOP
    v_key := NULLIF(p_key_digests[v_index], '');
    INSERT INTO _oo.full_import_quality (
      run_id, physical_name, batch_id, kind, reason, value_len, attnum, row_index,
      key_digest, input_digest, accepted_rows, rejected_rows, payload_digest
    ) VALUES (
      p_run, p_physical, p_batch_id, 'reject', p_reasons[v_index], p_value_lens[v_index],
      p_attnums[v_index], p_row_indexes[v_index], v_key, p_input_digest,
      NULL, NULL, p_payload_digest
    );
  END LOOP;
END;
$fi$;

CREATE FUNCTION _oo.full_import_probe_conflict(
  p_shard text,
  p_owner uuid,
  p_generation bigint,
  p_source text,
  p_physical text,
  p_batch_id text,
  p_payload_digest text,
  p_config_digest text,
  p_type_digest text,
  p_hook_digest text,
  p_schema_digest text,
  p_applied_pos numeric
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $fi$
DECLARE
  v_attname text;
  v_oid bigint;
  v_list text := '';
  v_values text := '';
  v_literal text;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_generation);
  IF NOT _oo.full_import_physical_ok(p_physical) OR p_physical !~ '^pt_' THEN
    RAISE EXCEPTION 'physical mapping name is closed' USING ERRCODE = '23514';
  END IF;
  FOR v_attname, v_oid IN
    SELECT attribute.attname::text, attribute.atttypid::bigint
    FROM pg_catalog.pg_attribute AS attribute
    JOIN pg_catalog.pg_class AS relation ON relation.oid = attribute.attrelid
    JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = '_oo'
      AND relation.relname = p_physical
      AND attribute.attnum > 0
      AND NOT attribute.attisdropped
    ORDER BY attribute.attnum
  LOOP
    IF v_attname = '_oo_pos' THEN
      v_literal := '900000';
    ELSIF v_oid = 16 THEN
      v_literal := 'false';
    ELSIF v_oid IN (21, 23, 20, 700, 701, 1700) THEN
      v_literal := '0';
    ELSIF v_oid IN (25, 1043, 1042) THEN
      v_literal := pg_catalog.quote_literal('');
    ELSIF v_oid = 17 THEN
      v_literal := pg_catalog.quote_literal('') || '::bytea';
    ELSIF v_oid = 2950 THEN
      v_literal := pg_catalog.quote_literal('00000000-0000-0000-0000-000000000000') || '::uuid';
    ELSIF v_oid = 1082 THEN
      v_literal := pg_catalog.quote_literal('2000-01-01') || '::date';
    ELSIF v_oid = 1083 THEN
      v_literal := pg_catalog.quote_literal('00:00:00') || '::time';
    ELSIF v_oid = 1114 THEN
      v_literal := pg_catalog.quote_literal('2000-01-01') || '::timestamp';
    ELSIF v_oid = 1184 THEN
      v_literal := pg_catalog.quote_literal('2000-01-01 00:00:00+00') || '::timestamptz';
    ELSIF v_oid = 114 THEN
      v_literal := pg_catalog.quote_literal('{}') || '::json';
    ELSE
      RAISE EXCEPTION 'closed copy type' USING ERRCODE = '22023';
    END IF;
    IF v_attname <> '_oo_pos' AND NOT _oo.full_import_physical_ok(v_attname) THEN
      RAISE EXCEPTION 'physical mapping name is closed' USING ERRCODE = '23514';
    END IF;
    IF v_list <> '' THEN
      v_list := v_list || ', ';
      v_values := v_values || ', ';
    END IF;
    IF v_attname = '_oo_pos' THEN
      v_list := v_list || '"_oo_pos"';
    ELSE
      v_list := v_list || format('%I', v_attname);
    END IF;
    v_values := v_values || v_literal;
  END LOOP;
  EXECUTE format('INSERT INTO _oo.%I (%s) VALUES (%s)', p_physical, v_list, v_values);
  INSERT INTO _oo.full_import_quality (
    run_id, physical_name, batch_id, kind, input_digest, accepted_rows, rejected_rows, payload_digest
  )
  SELECT run_row.run_id, p_physical, p_batch_id, 'batch', run_row.input_digest, 1, 0, p_payload_digest
  FROM _oo.full_import_table AS table_row
  JOIN _oo.full_import_run AS run_row ON run_row.run_id = table_row.run_id
  WHERE table_row.physical_name = p_physical
    AND run_row.state = 'open'
  LIMIT 1;
  PERFORM _oo.commit_apply_receipt(
    p_shard, p_owner, p_generation, p_source, p_physical, p_batch_id,
    p_config_digest, p_type_digest, p_hook_digest, p_schema_digest, p_payload_digest,
    'snapshot-zero/v1', 'full-import.snapshot', 0, p_applied_pos,
    false, false, true, NULL, NULL, ''::bytea
  );
  PERFORM pg_catalog.set_config('_oo.writer', '', true);
END;
$fi$;

REVOKE ALL ON TABLE _oo.full_import_session FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON TABLE _oo.full_import_run FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON TABLE _oo.full_import_table FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON TABLE _oo.full_import_quality FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.full_import_physical_ok(text) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.full_import_type_sql(bigint, integer) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.full_import_assert_bound(text, uuid, bigint) FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.full_import_guard_session() FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.full_import_guard_run() FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.full_import_guard_child() FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.full_import_guard_shadow() FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.full_import_bind_session(text, uuid, bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.full_import_open_run(text, uuid, bigint, text, bigint, text, text, text, text, text[], text[], text[]) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.full_import_prepare_shadow(text, uuid, bigint, uuid, integer, text, text, bigint, text, text, text[], bigint[], integer[], boolean[]) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.full_import_stage_decision(text, uuid, bigint, text, text, text, text, text, text, text, text, numeric) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.full_import_write_batch(text, uuid, bigint, uuid, integer, text, text, text, text, text, text, text, text, text, numeric, bigint, bigint, text[], bigint[], integer[], bigint[], text[]) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.full_import_note_table(text, uuid, bigint, uuid, integer, bigint, text, text) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.full_import_seal_run(text, uuid, bigint, uuid, text, bigint, text, text, text, text, text[], text[]) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.full_import_retire_run(text, uuid, bigint, uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.full_import_recover(text, text, text, text) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.full_import_record_refusal(text, uuid, bigint, uuid, text, text, text, text, text[], bigint[], integer[], bigint[], text[]) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.full_import_probe_conflict(text, uuid, bigint, text, text, text, text, text, text, text, text, numeric) FROM PUBLIC;

GRANT EXECUTE ON FUNCTION _oo.full_import_bind_session(text, uuid, bigint) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.full_import_open_run(text, uuid, bigint, text, bigint, text, text, text, text, text[], text[], text[]) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.full_import_prepare_shadow(text, uuid, bigint, uuid, integer, text, text, bigint, text, text, text[], bigint[], integer[], boolean[]) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.full_import_stage_decision(text, uuid, bigint, text, text, text, text, text, text, text, text, numeric) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.full_import_write_batch(text, uuid, bigint, uuid, integer, text, text, text, text, text, text, text, text, text, numeric, bigint, bigint, text[], bigint[], integer[], bigint[], text[]) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.full_import_note_table(text, uuid, bigint, uuid, integer, bigint, text, text) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.full_import_seal_run(text, uuid, bigint, uuid, text, bigint, text, text, text, text, text[], text[]) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.full_import_retire_run(text, uuid, bigint, uuid) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.full_import_recover(text, text, text, text) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.full_import_record_refusal(text, uuid, bigint, uuid, text, text, text, text, text[], bigint[], integer[], bigint[], text[]) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.full_import_probe_conflict(text, uuid, bigint, text, text, text, text, text, text, text, text, numeric) TO oo_pipeline_runtime;
