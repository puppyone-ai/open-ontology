-- chunked-typed/v2 C1 baseline, single-write target objects (DESIGN s3.1-s3.5, s4.1).
-- Run as oo_target_owner after 0041. v1 objects 0028-0038 are not replaced.
--
-- A v2 run is the v1 stage row (_oo.native_c1_baseline_run) plus a format marker.
-- Format pinning: v2 tasks cannot receive v1 JSON rows, v1 transactions that are
-- not covered by a v2 batch, or a v1 seal. Those guards fire only for marked tasks.
--
-- Concurrency (R1/R2): every public write function takes the task advisory lock
-- first. Writes are authorized by the C1 lease (task generation and source
-- epoch), as v1 stage writes are. The target fence is required only to open a
-- table run, to rebind its full-import run to the current generation, and to seal.
-- The shadow table T_new has no grant to the runtime role.

DO $v2$
BEGIN
  IF pg_catalog.to_regclass('_oo.native_c1_baseline_run') IS NULL
     OR pg_catalog.to_regclass('_oo.native_c1_baseline_txn') IS NULL
     OR pg_catalog.to_regclass('_oo.native_c1_baseline_txn_time') IS NULL
     OR pg_catalog.to_regclass('_oo.native_c1_baseline_row') IS NULL
     OR pg_catalog.to_regclass('_oo.full_import_run') IS NULL
     OR pg_catalog.to_regclass('_oo.full_import_table') IS NULL
     OR pg_catalog.to_regprocedure('_oo.native_c1_baseline_rebind(text,uuid,bigint)') IS NULL
  THEN
    RAISE EXCEPTION 'chunked C1 baseline requires the v1 C1 stage and full import'
      USING ERRCODE = 'OO002';
  END IF;
END
$v2$;

CREATE TABLE _oo.native_c1_baseline_v2_format (
  source text NOT NULL,
  task_id uuid NOT NULL,
  format text NOT NULL CHECK (format = 'chunked-typed/v2'),
  sealed boolean NOT NULL DEFAULT false,
  opened_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (source, task_id),
  FOREIGN KEY (source, task_id) REFERENCES _oo.native_c1_baseline_run (source, task_id)
);

CREATE TABLE _oo.native_c1_baseline_v2_table (
  source text NOT NULL,
  task_id uuid NOT NULL,
  schema_ident text NOT NULL,
  table_ident text NOT NULL,
  table_index integer NOT NULL CHECK (table_index >= 0),
  run_id uuid NOT NULL UNIQUE REFERENCES _oo.full_import_run (run_id),
  physical_name text NOT NULL CHECK (physical_name ~ '^pt_[0-9a-f]{60}$' AND octet_length(physical_name) = 63),
  stage_name text NOT NULL CHECK (stage_name ~ '^cs_[0-9a-f]{32}$'),
  tomb_name text NOT NULL CHECK (tomb_name ~ '^c1t_[0-9a-f]{24}$'),
  schema_digest text NOT NULL CHECK (schema_digest ~ '^[0-9a-f]{64}$'),
  columns_ident text[] NOT NULL,
  physical_columns text[] NOT NULL,
  column_types text[] NOT NULL,
  key_ident text NOT NULL,
  key_column text NOT NULL,
  key_type text NOT NULL,
  PRIMARY KEY (source, task_id, schema_ident, table_ident),
  UNIQUE (source, task_id, table_index),
  FOREIGN KEY (source, task_id) REFERENCES _oo.native_c1_baseline_v2_format (source, task_id)
);

CREATE TABLE _oo.native_c1_baseline_chunk (
  source text NOT NULL,
  task_id uuid NOT NULL,
  schema_ident text NOT NULL,
  table_ident text NOT NULL,
  chunk_no integer NOT NULL CHECK (chunk_no >= 1),
  attempt integer NOT NULL CHECK (attempt >= 1),
  key_lo numeric(38, 0),
  key_hi numeric(38, 0),
  terminal boolean NOT NULL,
  row_count bigint NOT NULL CHECK (row_count >= 0),
  inserted bigint NOT NULL CHECK (inserted >= 0),
  skipped_tomb bigint NOT NULL CHECK (skipped_tomb >= 0),
  skipped_existing bigint NOT NULL CHECK (skipped_existing >= 0),
  part_sha256 text NOT NULL CHECK (part_sha256 ~ '^[0-9a-f]{64}$'),
  lease_generation bigint NOT NULL CHECK (lease_generation >= 1),
  loaded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (source, task_id, schema_ident, table_ident, chunk_no),
  FOREIGN KEY (source, task_id, schema_ident, table_ident)
    REFERENCES _oo.native_c1_baseline_v2_table (source, task_id, schema_ident, table_ident),
  CHECK (inserted + skipped_tomb + skipped_existing = row_count),
  CHECK (
    CASE WHEN terminal
      THEN key_hi IS NULL AND row_count = 0
      ELSE key_hi IS NOT NULL AND (key_lo IS NULL OR key_lo < key_hi)
    END
  )
);

CREATE TABLE _oo.native_c1_baseline_batch (
  source text NOT NULL,
  task_id uuid NOT NULL,
  batch_no integer NOT NULL CHECK (batch_no >= 1),
  first_pos numeric(38, 0) NOT NULL,
  last_pos numeric(38, 0) NOT NULL,
  txns integer NOT NULL CHECK (txns >= 1),
  row_count bigint NOT NULL CHECK (row_count >= 0),
  batch_sha256 text NOT NULL CHECK (batch_sha256 ~ '^[0-9a-f]{64}$'),
  lease_generation bigint NOT NULL CHECK (lease_generation >= 1),
  committed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (source, task_id, batch_no),
  UNIQUE (source, task_id, first_pos),
  CHECK (last_pos >= first_pos AND last_pos - first_pos + 1 = txns),
  FOREIGN KEY (source, task_id) REFERENCES _oo.native_c1_baseline_v2_format (source, task_id)
);

REVOKE ALL ON _oo.native_c1_baseline_v2_format, _oo.native_c1_baseline_v2_table,
  _oo.native_c1_baseline_chunk, _oo.native_c1_baseline_batch FROM PUBLIC, oo_pipeline_runtime;

-- Helper: task advisory lock (R2). The runtime role cannot call pg_advisory directly.
CREATE FUNCTION _oo.native_c1_baseline_lock(p_source text, p_task uuid)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
BEGIN
  IF p_source IS NULL OR p_source !~ '^[a-z][a-z0-9_]{0,62}$' OR p_task IS NULL THEN
    RAISE EXCEPTION 'closed C1 baseline identity' USING ERRCODE = 'OO030';
  END IF;
  PERFORM pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended(p_source || ':' || p_task::text, 0));
END
$v2$;

-- Helper: live C1 lease for a v2 run. Plain read, no row lock (R2 serializes).
CREATE FUNCTION _oo.native_c1_baseline_v2_assert(p_source text, p_task uuid, p_generation bigint, p_epoch integer)
RETURNS _oo.native_c1_baseline_run
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE;
BEGIN
  SELECT * INTO v FROM _oo.native_c1_baseline_run WHERE source = p_source AND task_id = p_task;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'C1 v2 baseline is not open' USING ERRCODE = 'OO010';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo.native_c1_baseline_v2_format f WHERE f.source = p_source AND f.task_id = p_task) THEN
    RAISE EXCEPTION 'format_mismatch' USING ERRCODE = 'OO030';
  END IF;
  IF EXISTS (SELECT 1 FROM _oo.native_c1_baseline_v2_format f
             WHERE f.source = p_source AND f.task_id = p_task AND f.sealed) THEN
    RAISE EXCEPTION 'C1 v2 baseline is sealed' USING ERRCODE = 'OO010';
  END IF;
  IF v.lease_generation <> p_generation OR v.source_epoch <> p_epoch OR v.state <> 'open'
     OR NOT EXISTS (SELECT 1 FROM jobs.tasks t WHERE t.id = p_task AND t.state = 'running'
       AND t.generation = p_generation AND t.lease_until > clock_timestamp())
     OR NOT EXISTS (SELECT 1 FROM _oo.source_instance i WHERE i.source = p_source
       AND i.source_epoch = p_epoch AND i.engine = 'mysql' AND i.measured_id = v.measured_id
       AND i.config_digest = v.config_digest AND i.schema_digest = v.schema_digest
       AND i.lease_id = p_task AND i.lease_generation = p_generation) THEN
    RAISE EXCEPTION 'C1 baseline lease or source epoch is not live' USING ERRCODE = 'OO010';
  END IF;
  RETURN v;
END
$v2$;

-- Helper: registered table of a v2 run.
CREATE FUNCTION _oo.native_c1_baseline_v2_reg(p_source text, p_task uuid, p_schema text, p_table text)
RETURNS _oo.native_c1_baseline_v2_table
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
DECLARE t _oo.native_c1_baseline_v2_table%ROWTYPE;
BEGIN
  SELECT * INTO t FROM _oo.native_c1_baseline_v2_table
   WHERE source = p_source AND task_id = p_task AND schema_ident = p_schema AND table_ident = p_table;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'table_unregistered' USING ERRCODE = 'OO030';
  END IF;
  RETURN t;
END
$v2$;

-- Helper: comma-separated quoted column list, optionally qualified by alias.
CREATE FUNCTION _oo.native_c1_baseline_v2_collist(p_cols text[], p_alias text)
RETURNS text LANGUAGE sql IMMUTABLE SET search_path = pg_catalog
AS $v2$
  SELECT string_agg(CASE WHEN p_alias = '' THEN format('%I', c) ELSE format('%s.%I', p_alias, c) END, ', ' ORDER BY ord)
  FROM unnest(p_cols) WITH ORDINALITY AS u(c, ord)
$v2$;

-- Helper: SET list for ON CONFLICT DO UPDATE from EXCLUDED.
CREATE FUNCTION _oo.native_c1_baseline_v2_setlist(p_cols text[])
RETURNS text LANGUAGE sql IMMUTABLE SET search_path = pg_catalog
AS $v2$
  SELECT string_agg(format('%1$I = EXCLUDED.%1$I', c), ', ' ORDER BY ord)
  FROM unnest(p_cols) WITH ORDINALITY AS u(c, ord)
$v2$;

-- Lease, format and guard triggers on the v1 tables. They act only on v2-marked tasks.
CREATE FUNCTION _oo.native_c1_baseline_v2_guard_row()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
BEGIN
  IF EXISTS (SELECT 1 FROM _oo.native_c1_baseline_v2_format f
             WHERE f.source = NEW.source AND f.task_id = NEW.task_id) THEN
    RAISE EXCEPTION 'format_mismatch' USING ERRCODE = 'OO030';
  END IF;
  RETURN NEW;
END
$v2$;

CREATE FUNCTION _oo.native_c1_baseline_v2_guard_txn()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
BEGIN
  IF EXISTS (SELECT 1 FROM _oo.native_c1_baseline_v2_format f
             WHERE f.source = NEW.source AND f.task_id = NEW.task_id)
     AND NOT EXISTS (SELECT 1 FROM _oo.native_c1_baseline_batch b
             WHERE b.source = NEW.source AND b.task_id = NEW.task_id
               AND NEW.applied_pos BETWEEN b.first_pos AND b.last_pos) THEN
    RAISE EXCEPTION 'format_mismatch' USING ERRCODE = 'OO030';
  END IF;
  RETURN NEW;
END
$v2$;

CREATE FUNCTION _oo.native_c1_baseline_v2_guard_run()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
BEGIN
  IF NEW.state = 'awaiting_publish' AND OLD.state IS DISTINCT FROM NEW.state
     AND EXISTS (SELECT 1 FROM _oo.native_c1_baseline_v2_format f
                 WHERE f.source = NEW.source AND f.task_id = NEW.task_id AND NOT f.sealed) THEN
    RAISE EXCEPTION 'format_mismatch' USING ERRCODE = 'OO030';
  END IF;
  RETURN NEW;
END
$v2$;

CREATE TRIGGER native_c1_baseline_v2_guard_row
  BEFORE INSERT ON _oo.native_c1_baseline_row
  FOR EACH ROW EXECUTE FUNCTION _oo.native_c1_baseline_v2_guard_row();
CREATE TRIGGER native_c1_baseline_v2_guard_txn
  BEFORE INSERT ON _oo.native_c1_baseline_txn
  FOR EACH ROW EXECUTE FUNCTION _oo.native_c1_baseline_v2_guard_txn();
CREATE TRIGGER native_c1_baseline_v2_guard_run
  BEFORE UPDATE ON _oo.native_c1_baseline_run
  FOR EACH ROW EXECUTE FUNCTION _oo.native_c1_baseline_v2_guard_run();

-- Open: the v1 open is reused for identity and lease. Format is pinned here.
CREATE FUNCTION _oo.native_c1_baseline_v2_open(
  p_source text, p_task uuid, p_generation bigint, p_epoch integer,
  p_measured_id text, p_config_digest text, p_schema_digest text,
  p_l0 text, p_l0_pos numeric
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
DECLARE result jsonb;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(p_source, p_task);
  IF EXISTS (SELECT 1 FROM _oo.native_c1_baseline_run WHERE source = p_source AND task_id = p_task)
     AND NOT EXISTS (SELECT 1 FROM _oo.native_c1_baseline_v2_format WHERE source = p_source AND task_id = p_task)
  THEN
    RAISE EXCEPTION 'format_mismatch' USING ERRCODE = 'OO030';
  END IF;
  result := _oo.native_c1_baseline_open(p_source, p_task, p_generation, p_epoch, p_measured_id,
    p_config_digest, p_schema_digest, p_l0, p_l0_pos);
  INSERT INTO _oo.native_c1_baseline_v2_format (source, task_id, format)
    VALUES (p_source, p_task, 'chunked-typed/v2')
    ON CONFLICT (source, task_id) DO NOTHING;
  RETURN result || jsonb_build_object('format', 'chunked-typed/v2');
END
$v2$;

-- Open one table: shadow T_new (no grant), tombstone TB, unlogged part stage.
-- The fence is required here because full_import_open_run binds the run.
CREATE FUNCTION _oo.native_c1_baseline_v2_open_target(
  p_source text, p_task uuid, p_generation bigint, p_epoch integer,
  p_shard text, p_owner uuid, p_target_generation bigint, p_source_generation bigint,
  p_type_digest text, p_input_digest text, p_table_schema_digest text,
  p_schema text, p_table text, p_columns text[], p_oids bigint[], p_typmods integer[],
  p_not_null boolean[], p_key text
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
DECLARE
  v _oo.native_c1_baseline_run%ROWTYPE;
  r _oo.native_c1_baseline_v2_table%ROWTYPE;
  n integer; k integer; i integer; v_index integer;
  v_phys text; v_pc text; v_run uuid; v_hash text; v_tomb text; v_stage text;
  v_types text[] := '{}'; v_pcs text[] := '{}';
  v_defs text[] := '{}'; v_sdefs text[] := '{}'; v_key_type text; v_key_pc text;
  v_scale bigint;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(p_source, p_task);
  v := _oo.native_c1_baseline_v2_assert(p_source, p_task, p_generation, p_epoch);
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_target_generation);
  n := cardinality(p_columns);
  IF p_schema IS NULL OR p_schema = '' OR p_table IS NULL OR p_table = '' OR n IS NULL OR n < 1
     OR cardinality(p_oids) <> n OR cardinality(p_typmods) <> n OR cardinality(p_not_null) <> n
     OR p_key IS NULL OR NOT (p_key = ANY (p_columns)) OR p_source_generation < 1
     OR p_table_schema_digest IS NULL OR p_table_schema_digest !~ '^[0-9a-f]{64}$'
  THEN
    RAISE EXCEPTION 'closed_baseline_table' USING ERRCODE = 'OO030';
  END IF;
  k := array_position(p_columns, p_key);
  v_scale := CASE WHEN p_oids[k] = 1700 THEN (p_typmods[k] - 4) & 65535 ELSE 0 END;
  IF p_oids[k] NOT IN (20, 21, 23, 1700) OR NOT p_not_null[k]
     OR (p_oids[k] = 1700 AND (p_typmods[k] IS NULL OR p_typmods[k] < 0 OR v_scale <> 0))
  THEN
    RAISE EXCEPTION 'closed_baseline_key' USING ERRCODE = 'OO030';
  END IF;
  FOR i IN 1..n LOOP
    v_types := v_types || _oo.full_import_type_sql(p_oids[i], p_typmods[i]);
  END LOOP;

  SELECT * INTO r FROM _oo.native_c1_baseline_v2_table
   WHERE source = p_source AND task_id = p_task AND schema_ident = p_schema AND table_ident = p_table;
  IF FOUND THEN
    IF r.columns_ident IS DISTINCT FROM p_columns OR r.column_types IS DISTINCT FROM v_types
       OR r.key_ident IS DISTINCT FROM p_key OR r.schema_digest IS DISTINCT FROM p_table_schema_digest
    THEN
      RAISE EXCEPTION 'table_identity_conflict' USING ERRCODE = 'OO030';
    END IF;
    RETURN jsonb_build_object('protocol', 'oo.pipeline.native-c1-baseline-stage/v2', 'outcome', 'replay',
      'physical', r.physical_name, 'run_id', r.run_id, 'stage', r.stage_name, 'tomb', r.tomb_name,
      'key_column', r.key_column, 'stage_columns',
      to_jsonb(array['_oo_part_kind', '_oo_part_no', '_oo_op', '_oo_pos'] || r.physical_columns));
  END IF;

  SELECT count(*) INTO v_index FROM _oo.native_c1_baseline_v2_table WHERE source = p_source AND task_id = p_task;
  v_phys := _oo.lookup_physical_mapping(p_source, v.config_digest, p_source_generation, 'table', p_schema, p_table, '');
  IF v_phys IS NULL OR NOT _oo.full_import_physical_ok(v_phys) OR v_phys !~ '^pt_' THEN
    RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'OO024';
  END IF;
  FOR i IN 1..n LOOP
    v_pc := _oo.lookup_physical_mapping(p_source, v.config_digest, p_source_generation, 'column', p_schema, p_table, p_columns[i]);
    IF v_pc IS NULL OR NOT _oo.full_import_physical_ok(v_pc) OR v_pc !~ '^pc_' THEN
      RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'OO024';
    END IF;
    v_pcs := v_pcs || v_pc;
    v_defs := v_defs || format('%I %s%s', v_pc, v_types[i], CASE WHEN p_not_null[i] THEN ' NOT NULL' ELSE '' END);
    v_sdefs := v_sdefs || format('%I %s', v_pc, v_types[i]);
  END LOOP;
  v_key_pc := v_pcs[k];
  v_key_type := v_types[k];

  v_run := _oo.full_import_open_run(p_shard, p_owner, p_target_generation, p_source, p_source_generation,
    v.config_digest, p_type_digest, p_input_digest, p_table_schema_digest,
    ARRAY[p_schema], ARRAY[p_table], ARRAY[v_phys]);

  EXECUTE format('CREATE TABLE _oo.%I ("_oo_pos" numeric(38,0) NOT NULL, %s, PRIMARY KEY (%I))',
    v_phys, array_to_string(v_defs, ', '), v_key_pc);
  EXECUTE format('REVOKE ALL ON TABLE _oo.%I FROM PUBLIC', v_phys);

  v_hash := md5(p_source || ':' || p_task::text || ':' || p_schema || '.' || p_table);
  v_tomb := 'c1t_' || left(v_hash, 24);
  v_stage := 'cs_' || v_hash;
  EXECUTE format('CREATE TABLE _oo.%I (k %s PRIMARY KEY, pos numeric(38,0) NOT NULL)', v_tomb, v_key_type);
  EXECUTE format('REVOKE ALL ON TABLE _oo.%I FROM PUBLIC', v_tomb);
  EXECUTE format('CREATE UNLOGGED TABLE _oo.%I (_oo_part_kind text, _oo_part_no integer, _oo_op text, _oo_pos numeric(38,0), %s)',
    v_stage, array_to_string(v_sdefs, ', '));
  EXECUTE format('REVOKE ALL ON TABLE _oo.%I FROM PUBLIC', v_stage);
  EXECUTE format('GRANT INSERT ON TABLE _oo.%I TO oo_pipeline_runtime', v_stage);

  INSERT INTO _oo.native_c1_baseline_v2_table (source, task_id, schema_ident, table_ident, table_index, run_id,
    physical_name, stage_name, tomb_name, schema_digest, columns_ident, physical_columns, column_types,
    key_ident, key_column, key_type)
  VALUES (p_source, p_task, p_schema, p_table, v_index, v_run, v_phys, v_stage, v_tomb,
    p_table_schema_digest, p_columns, v_pcs, v_types, p_key, v_key_pc, v_key_type);

  RETURN jsonb_build_object('protocol', 'oo.pipeline.native-c1-baseline-stage/v2', 'outcome', 'opened',
    'physical', v_phys, 'run_id', v_run, 'stage', v_stage, 'tomb', v_tomb, 'key_column', v_key_pc,
    'stage_columns', to_jsonb(array['_oo_part_kind', '_oo_part_no', '_oo_op', '_oo_pos'] || v_pcs));
END
$v2$;

-- Unlocked core for one snapshot chunk (DESIGN s3.1 fill row). Not granted.
-- Only the public wrapper may call it, so the R2 control test can run it bare.
CREATE FUNCTION _oo.native_c1_baseline_v2_chunk_core(
  p_source text, p_task uuid, p_generation bigint, p_epoch integer,
  p_schema text, p_table text, p_chunk_no integer, p_attempt integer,
  p_key_lo numeric, p_key_hi numeric, p_terminal boolean, p_row_count bigint, p_part_sha256 text
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
DECLARE
  v _oo.native_c1_baseline_run%ROWTYPE;
  t _oo.native_c1_baseline_v2_table%ROWTYPE;
  c _oo.native_c1_baseline_chunk%ROWTYPE;
  prev _oo.native_c1_baseline_chunk%ROWTYPE;
  v_part bigint; v_stage bigint; v_distinct bigint; v_bad bigint; v_tomb bigint; v_ins bigint;
  v_cols text; v_scols text;
BEGIN
  v := _oo.native_c1_baseline_v2_assert(p_source, p_task, p_generation, p_epoch);
  t := _oo.native_c1_baseline_v2_reg(p_source, p_task, p_schema, p_table);
  IF p_chunk_no IS NULL OR p_chunk_no < 1 OR p_attempt IS NULL OR p_attempt < 1
     OR p_row_count IS NULL OR p_row_count < 0 OR p_terminal IS NULL
     OR p_part_sha256 IS NULL OR p_part_sha256 !~ '^[0-9a-f]{64}$'
     OR (p_terminal AND (p_key_hi IS NOT NULL OR p_row_count <> 0))
     OR (NOT p_terminal AND (p_key_hi IS NULL OR (p_key_lo IS NOT NULL AND p_key_lo >= p_key_hi)))
  THEN
    RAISE EXCEPTION 'invalid_chunk' USING ERRCODE = 'OO030';
  END IF;

  SELECT * INTO c FROM _oo.native_c1_baseline_chunk
   WHERE source = p_source AND task_id = p_task AND schema_ident = p_schema AND table_ident = p_table
     AND chunk_no = p_chunk_no;
  IF FOUND THEN
    IF c.key_lo IS NOT DISTINCT FROM p_key_lo AND c.key_hi IS NOT DISTINCT FROM p_key_hi
       AND c.terminal = p_terminal AND c.row_count = p_row_count AND c.part_sha256 = p_part_sha256
    THEN
      EXECUTE format('DELETE FROM _oo.%I', t.stage_name);
      RETURN jsonb_build_object('protocol', 'oo.pipeline.native-c1-baseline-stage/v2', 'outcome', 'replay',
        'chunk_no', p_chunk_no, 'inserted', c.inserted, 'skipped_tomb', c.skipped_tomb,
        'skipped_existing', c.skipped_existing);
    END IF;
    RAISE EXCEPTION 'chunk_conflict' USING ERRCODE = 'OO030';
  END IF;

  SELECT * INTO prev FROM _oo.native_c1_baseline_chunk
   WHERE source = p_source AND task_id = p_task AND schema_ident = p_schema AND table_ident = p_table
   ORDER BY chunk_no DESC LIMIT 1;
  IF NOT FOUND THEN
    IF p_chunk_no <> 1 OR p_key_lo IS NOT NULL THEN
      RAISE EXCEPTION 'chunk_sequence' USING ERRCODE = 'OO030';
    END IF;
  ELSIF prev.terminal OR p_chunk_no <> prev.chunk_no + 1 OR p_key_lo IS DISTINCT FROM prev.key_hi THEN
    RAISE EXCEPTION 'chunk_sequence' USING ERRCODE = 'OO030';
  END IF;

  EXECUTE format('SELECT count(*) FILTER (WHERE _oo_part_kind = ''snapshot'' AND _oo_part_no = $1), count(*) FROM _oo.%I', t.stage_name)
    INTO v_part, v_stage USING p_chunk_no;
  IF v_part <> p_row_count OR v_stage <> p_row_count THEN
    RAISE EXCEPTION 'chunk_part_rows' USING ERRCODE = 'OO030';
  END IF;
  EXECUTE format($q$SELECT count(DISTINCT %1$I), count(*) FILTER (WHERE %1$I IS NULL
      OR ($1::numeric IS NOT NULL AND %1$I::numeric <= $1::numeric)
      OR ($2::numeric IS NULL OR %1$I::numeric > $2::numeric))
    FROM _oo.%2$I WHERE _oo_part_kind = 'snapshot' AND _oo_part_no = $3$q$, t.key_column, t.stage_name)
    INTO v_distinct, v_bad USING p_key_lo, p_key_hi, p_chunk_no;
  IF v_distinct <> p_row_count OR v_bad <> 0 THEN
    RAISE EXCEPTION 'chunk_keys' USING ERRCODE = 'OO030';
  END IF;

  v_cols := _oo.native_c1_baseline_v2_collist(t.physical_columns, '');
  v_scols := _oo.native_c1_baseline_v2_collist(t.physical_columns, 's');
  EXECUTE format('SELECT count(*) FROM _oo.%1$I s WHERE s._oo_part_kind = ''snapshot'' AND s._oo_part_no = $1 AND EXISTS (SELECT 1 FROM _oo.%2$I w WHERE w.k = s.%3$I)',
    t.stage_name, t.tomb_name, t.key_column) INTO v_tomb USING p_chunk_no;
  EXECUTE format($q$WITH ins AS (INSERT INTO _oo.%1$I (%2$s, "_oo_pos")
      SELECT %3$s, 0 FROM _oo.%4$I s
      WHERE s._oo_part_kind = 'snapshot' AND s._oo_part_no = $1
        AND NOT EXISTS (SELECT 1 FROM _oo.%5$I w WHERE w.k = s.%6$I)
      ON CONFLICT (%6$I) DO NOTHING RETURNING 1)
    SELECT count(*) FROM ins$q$, t.physical_name, v_cols, v_scols, t.stage_name, t.tomb_name, t.key_column)
    INTO v_ins USING p_chunk_no;

  EXECUTE format('DELETE FROM _oo.%I', t.stage_name);
  INSERT INTO _oo.native_c1_baseline_chunk (source, task_id, schema_ident, table_ident, chunk_no, attempt,
    key_lo, key_hi, terminal, row_count, inserted, skipped_tomb, skipped_existing, part_sha256, lease_generation)
  VALUES (p_source, p_task, p_schema, p_table, p_chunk_no, p_attempt, p_key_lo, p_key_hi, p_terminal,
    p_row_count, v_ins, v_tomb, p_row_count - v_tomb - v_ins, p_part_sha256, p_generation);

  RETURN jsonb_build_object('protocol', 'oo.pipeline.native-c1-baseline-stage/v2', 'outcome', 'loaded',
    'chunk_no', p_chunk_no, 'inserted', v_ins, 'skipped_tomb', v_tomb,
    'skipped_existing', p_row_count - v_tomb - v_ins);
END
$v2$;

-- Public chunk write (R2 first, then the core).
CREATE FUNCTION _oo.native_c1_baseline_v2_chunk_apply(
  p_source text, p_task uuid, p_generation bigint, p_epoch integer,
  p_schema text, p_table text, p_chunk_no integer, p_attempt integer,
  p_key_lo numeric, p_key_hi numeric, p_terminal boolean, p_row_count bigint, p_part_sha256 text
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
BEGIN
  PERFORM _oo.native_c1_baseline_lock(p_source, p_task);
  RETURN _oo.native_c1_baseline_v2_chunk_core(p_source, p_task, p_generation, p_epoch, p_schema, p_table,
    p_chunk_no, p_attempt, p_key_lo, p_key_hi, p_terminal, p_row_count, p_part_sha256);
END
$v2$;

-- Clear the staged CDC rows of one batch in every table of the task.
CREATE FUNCTION _oo.native_c1_baseline_v2_clear_cdc(p_source text, p_task uuid, p_batch_no integer)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
DECLARE t record;
BEGIN
  FOR t IN SELECT stage_name FROM _oo.native_c1_baseline_v2_table
           WHERE source = p_source AND task_id = p_task LOOP
    EXECUTE format('DELETE FROM _oo.%I WHERE _oo_part_kind = ''cdc'' AND _oo_part_no = $1', t.stage_name)
      USING p_batch_no;
  END LOOP;
END
$v2$;

-- Unlocked core for one CDC batch (DESIGN s3.1 CDC rows; s3.4 batch identity).
-- The batch row is written before its transactions, so the txn guard trigger sees
-- the coverage. Per key only the last event of the batch is applied (s3.1).
CREATE FUNCTION _oo.native_c1_baseline_v2_batch_core(
  p_source text, p_task uuid, p_generation bigint, p_epoch integer,
  p_batch_no integer, p_first_pos numeric, p_last_pos numeric, p_txns integer,
  p_row_count bigint, p_batch_sha256 text, p_txn_records jsonb
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
DECLARE
  v _oo.native_c1_baseline_run%ROWTYPE;
  b _oo.native_c1_baseline_batch%ROWTYPE;
  t _oo.native_c1_baseline_v2_table%ROWTYPE;
  e jsonb; i integer; pos numeric; stamp timestamptz;
  v_rows bigint; v_distinct bigint; v_bad bigint; v_txn_rows bigint; v_total bigint := 0;
  v_up bigint; v_del bigint; v_upserts bigint; v_deletes bigint; v_tomb bigint;
  v_written bigint := 0; v_removed bigint := 0; v_left bigint;
  v_cols text; v_scols text; v_set text;
  v_stamp_re constant text := '^20[0-9]{2}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]{1,6})?Z$';
BEGIN
  v := _oo.native_c1_baseline_v2_assert(p_source, p_task, p_generation, p_epoch);
  IF p_batch_no IS NULL OR p_batch_no < 1 OR p_first_pos IS NULL OR p_last_pos IS NULL
     OR p_txns IS NULL OR p_txns < 1 OR p_row_count IS NULL OR p_row_count < 0
     OR p_batch_sha256 IS NULL OR p_batch_sha256 !~ '^[0-9a-f]{64}$'
     OR p_last_pos < p_first_pos OR p_last_pos - p_first_pos + 1 <> p_txns
     OR p_first_pos <= v.l0_pos
     OR jsonb_typeof(p_txn_records) IS DISTINCT FROM 'array' OR jsonb_array_length(p_txn_records) <> p_txns
  THEN
    RAISE EXCEPTION 'invalid_batch' USING ERRCODE = 'OO030';
  END IF;

  SELECT * INTO b FROM _oo.native_c1_baseline_batch
   WHERE source = p_source AND task_id = p_task AND batch_no = p_batch_no;
  IF FOUND THEN
    IF b.first_pos = p_first_pos AND b.last_pos = p_last_pos AND b.txns = p_txns
       AND b.row_count = p_row_count AND b.batch_sha256 = p_batch_sha256
    THEN
      PERFORM _oo.native_c1_baseline_v2_clear_cdc(p_source, p_task, p_batch_no);
      RETURN jsonb_build_object('protocol', 'oo.pipeline.native-c1-baseline-stage/v2', 'outcome', 'replay',
        'batch_no', p_batch_no, 'applied_pos', _oo.native_pos_text(v.applied_pos));
    END IF;
    RAISE EXCEPTION 'batch_conflict' USING ERRCODE = 'OO030';
  END IF;
  IF p_first_pos <> v.applied_pos + 1
     OR p_batch_no <> COALESCE((SELECT max(x.batch_no) FROM _oo.native_c1_baseline_batch x
                                WHERE x.source = p_source AND x.task_id = p_task), 0) + 1
  THEN
    RAISE EXCEPTION 'batch_sequence' USING ERRCODE = 'OO030';
  END IF;

  INSERT INTO _oo.native_c1_baseline_batch (source, task_id, batch_no, first_pos, last_pos, txns,
    row_count, batch_sha256, lease_generation)
  VALUES (p_source, p_task, p_batch_no, p_first_pos, p_last_pos, p_txns, p_row_count,
    p_batch_sha256, p_generation);

  FOR i IN 0 .. p_txns - 1 LOOP
    e := p_txn_records -> i;
    IF jsonb_typeof(e) IS DISTINCT FROM 'object' OR jsonb_typeof(e -> 'applied_pos') IS DISTINCT FROM 'string'
       OR jsonb_typeof(e -> 'row_count') IS DISTINCT FROM 'number'
    THEN
      RAISE EXCEPTION 'invalid_batch' USING ERRCODE = 'OO030';
    END IF;
    pos := (e ->> 'applied_pos')::numeric;
    IF pos <> p_first_pos + i
       OR (e ->> 'txn_id') IS DISTINCT FROM substring(v.measured_id from 7) || ':' || _oo.native_pos_text(pos)
       OR (e ->> 'payload_sha256') IS NULL OR (e ->> 'payload_sha256') !~ '^[0-9a-f]{64}$'
       OR (e ->> 'source_commit_timestamp') IS NULL OR (e ->> 'source_commit_timestamp') !~ v_stamp_re
       OR (e ->> 'row_count')::bigint < 0
    THEN
      RAISE EXCEPTION 'invalid_batch' USING ERRCODE = 'OO030';
    END IF;
    v_txn_rows := COALESCE(v_txn_rows, 0) + (e ->> 'row_count')::bigint;
    stamp := (e ->> 'source_commit_timestamp')::timestamptz;
    INSERT INTO _oo.native_c1_baseline_txn (source, task_id, applied_pos, txn_id, payload_sha256, applied_at)
      VALUES (p_source, p_task, pos, e ->> 'txn_id', e ->> 'payload_sha256', clock_timestamp());
    INSERT INTO _oo.native_c1_baseline_txn_time (source, task_id, applied_pos, source_committed_at)
      VALUES (p_source, p_task, pos, stamp);
  END LOOP;
  IF v_txn_rows IS DISTINCT FROM p_row_count THEN
    RAISE EXCEPTION 'batch_rows' USING ERRCODE = 'OO030';
  END IF;

  FOR t IN SELECT * FROM _oo.native_c1_baseline_v2_table
           WHERE source = p_source AND task_id = p_task ORDER BY schema_ident, table_ident LOOP
    v_cols := _oo.native_c1_baseline_v2_collist(t.physical_columns, '');
    v_scols := _oo.native_c1_baseline_v2_collist(t.physical_columns, 'ev');
    v_set := _oo.native_c1_baseline_v2_setlist(t.physical_columns);

    EXECUTE format('SELECT count(*), (SELECT count(*) FROM (SELECT DISTINCT %1$I, _oo_pos FROM _oo.%2$I WHERE _oo_part_kind = ''cdc'' AND _oo_part_no = $1) d),
        count(*) FILTER (WHERE %1$I IS NULL OR _oo_op IS NULL OR _oo_op NOT IN (''upsert'', ''delete'')
          OR _oo_pos IS NULL OR _oo_pos < $2 OR _oo_pos > $3)
      FROM _oo.%2$I WHERE _oo_part_kind = ''cdc'' AND _oo_part_no = $1', t.key_column, t.stage_name)
      INTO v_rows, v_distinct, v_bad USING p_batch_no, p_first_pos, p_last_pos;
    IF v_distinct <> v_rows OR v_bad <> 0 THEN
      RAISE EXCEPTION 'invalid_batch_events' USING ERRCODE = 'OO030';
    END IF;

    EXECUTE format($q$WITH ev AS (SELECT DISTINCT ON (s.%1$I) s.* FROM _oo.%2$I s
        WHERE s._oo_part_kind = 'cdc' AND s._oo_part_no = $1
        ORDER BY s.%1$I, s._oo_pos DESC),
      del AS (DELETE FROM _oo.%3$I w USING ev
        WHERE ev._oo_op = 'delete' AND w.%1$I = ev.%1$I AND w."_oo_pos" < ev._oo_pos RETURNING 1),
      tomb AS (INSERT INTO _oo.%4$I AS x (k, pos) SELECT ev.%1$I, ev._oo_pos FROM ev
        WHERE ev._oo_op = 'delete'
        ON CONFLICT (k) DO UPDATE SET pos = GREATEST(x.pos, EXCLUDED.pos) RETURNING 1)
      SELECT (SELECT count(*) FROM del), (SELECT count(*) FROM tomb),
        (SELECT count(*) FROM ev WHERE ev._oo_op = 'delete'), (SELECT count(*) FROM ev WHERE ev._oo_op = 'upsert')$q$,
      t.key_column, t.stage_name, t.physical_name, t.tomb_name)
      INTO v_del, v_tomb, v_deletes, v_upserts USING p_batch_no;

    EXECUTE format($q$WITH ev AS (SELECT DISTINCT ON (s.%1$I) s.* FROM _oo.%2$I s
        WHERE s._oo_part_kind = 'cdc' AND s._oo_part_no = $1
        ORDER BY s.%1$I, s._oo_pos DESC),
      ups AS (INSERT INTO _oo.%3$I AS w (%4$s, "_oo_pos")
        SELECT %5$s, ev._oo_pos FROM ev
        WHERE ev._oo_op = 'upsert'
          AND NOT EXISTS (SELECT 1 FROM _oo.%6$I x WHERE x.k = ev.%1$I AND x.pos > ev._oo_pos)
        ON CONFLICT (%1$I) DO UPDATE SET %7$s, "_oo_pos" = EXCLUDED."_oo_pos"
        WHERE w."_oo_pos" < EXCLUDED."_oo_pos" RETURNING 1)
      SELECT count(*) FROM ups$q$,
      t.key_column, t.stage_name, t.physical_name, v_cols, v_scols, t.tomb_name, v_set)
      INTO v_up USING p_batch_no;

    EXECUTE format('DELETE FROM _oo.%I WHERE _oo_part_kind = ''cdc'' AND _oo_part_no = $1', t.stage_name)
      USING p_batch_no;
    EXECUTE format('SELECT count(*) FROM _oo.%I', t.stage_name) INTO v_left;
    IF v_left <> 0 THEN
      RAISE EXCEPTION 'batch_rows' USING ERRCODE = 'OO030';
    END IF;
    v_total := v_total + v_rows;
    v_written := v_written + v_up;
    v_removed := v_removed + v_del;
  END LOOP;

  IF v_total <> p_row_count THEN
    RAISE EXCEPTION 'batch_rows' USING ERRCODE = 'OO030';
  END IF;

  UPDATE _oo.native_c1_baseline_run SET applied_pos = p_last_pos WHERE source = p_source AND task_id = p_task;

  RETURN jsonb_build_object('protocol', 'oo.pipeline.native-c1-baseline-stage/v2', 'outcome', 'applied',
    'batch_no', p_batch_no, 'applied_pos', _oo.native_pos_text(p_last_pos), 'txns', p_txns,
    'row_count', p_row_count, 'upserted', v_written, 'deleted', v_removed);
END
$v2$;

-- Public batch write (R2 first, then the core).
CREATE FUNCTION _oo.native_c1_baseline_v2_txn_batch(
  p_source text, p_task uuid, p_generation bigint, p_epoch integer,
  p_batch_no integer, p_first_pos numeric, p_last_pos numeric, p_txns integer,
  p_row_count bigint, p_batch_sha256 text, p_txn_records jsonb
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
BEGIN
  PERFORM _oo.native_c1_baseline_lock(p_source, p_task);
  RETURN _oo.native_c1_baseline_v2_batch_core(p_source, p_task, p_generation, p_epoch, p_batch_no,
    p_first_pos, p_last_pos, p_txns, p_row_count, p_batch_sha256, p_txn_records);
END
$v2$;

-- Seal (fenced). The caller holds the target fence and has bound its session, and
-- it computed p_tables from T_new in the same transaction after the R2 lock.
-- For each table: count check, tombstone and stage dropped, full_import_table
-- coverage set, one apply receipt (payload = composed digest), and note_table.
-- seal_quality and full_import_seal_run remain the caller's steps (N2).
CREATE FUNCTION _oo.native_c1_baseline_v2_seal(
  p_source text, p_task uuid, p_generation bigint, p_epoch integer,
  p_shard text, p_owner uuid, p_target_generation bigint,
  p_l_end text, p_l_end_pos numeric, p_hook_digest text, p_tables jsonb
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
DECLARE
  v _oo.native_c1_baseline_run%ROWTYPE;
  t _oo.native_c1_baseline_v2_table%ROWTYPE;
  f _oo.native_c1_baseline_v2_format%ROWTYPE;
  e jsonb; r _oo.full_import_run%ROWTYPE;
  v_rows bigint; v_expect bigint; v_digest text; v_receipt text; v_out jsonb := '[]'::jsonb;
  v_batch constant text := 'c1v2.seal';
BEGIN
  PERFORM _oo.native_c1_baseline_lock(p_source, p_task);
  SELECT * INTO f FROM _oo.native_c1_baseline_v2_format WHERE source = p_source AND task_id = p_task;
  IF FOUND AND f.sealed THEN
    SELECT * INTO v FROM _oo.native_c1_baseline_run WHERE source = p_source AND task_id = p_task;
    IF v.l_end IS DISTINCT FROM p_l_end_pos THEN
      RAISE EXCEPTION 'seal_conflict' USING ERRCODE = 'OO030';
    END IF;
    RETURN jsonb_build_object('protocol', 'oo.pipeline.native-c1-baseline-stage/v2', 'outcome', 'replay',
      'l_end', _oo.native_pos_text(p_l_end_pos));
  END IF;
  v := _oo.native_c1_baseline_v2_assert(p_source, p_task, p_generation, p_epoch);
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_target_generation);
  IF p_l_end IS NULL OR p_l_end !~ ('^' || substring(v.measured_id from 7) || ':[1-9][0-9]*-[1-9][0-9]*$')
     OR split_part(p_l_end, ':', 2) !~ ('^[1-9][0-9]*-' || _oo.native_pos_text(p_l_end_pos) || '$')
     OR p_l_end_pos <> v.applied_pos OR p_l_end_pos < v.l0_pos
  THEN
    RAISE EXCEPTION 'final barrier is not applied through L_end' USING ERRCODE = 'OO010';
  END IF;
  IF jsonb_typeof(p_tables) IS DISTINCT FROM 'array'
     OR jsonb_array_length(p_tables) <> (SELECT count(*) FROM _oo.native_c1_baseline_v2_table
                                         WHERE source = p_source AND task_id = p_task)
  THEN
    RAISE EXCEPTION 'seal_tables' USING ERRCODE = 'OO030';
  END IF;
  IF p_hook_digest IS NULL OR p_hook_digest !~ '^[0-9a-f]{64}$' THEN
    RAISE EXCEPTION 'seal_tables' USING ERRCODE = 'OO030';
  END IF;

  FOR t IN SELECT * FROM _oo.native_c1_baseline_v2_table
           WHERE source = p_source AND task_id = p_task ORDER BY schema_ident, table_ident LOOP
    IF NOT EXISTS (SELECT 1 FROM _oo.native_c1_baseline_chunk c
                   WHERE c.source = p_source AND c.task_id = p_task
                     AND c.schema_ident = t.schema_ident AND c.table_ident = t.table_ident AND c.terminal) THEN
      RAISE EXCEPTION 'snapshot_incomplete' USING ERRCODE = 'OO010';
    END IF;
    SELECT x.value INTO e FROM jsonb_array_elements(p_tables) x
     WHERE x.value ->> 'schema' = t.schema_ident AND x.value ->> 'table' = t.table_ident;
    IF NOT FOUND OR jsonb_typeof(e -> 'row_count') IS DISTINCT FROM 'number'
       OR (e ->> 'payload_digest') IS NULL OR (e ->> 'payload_digest') !~ '^[0-9a-f]{64}$'
    THEN
      RAISE EXCEPTION 'seal_tables' USING ERRCODE = 'OO030';
    END IF;
    v_expect := (e ->> 'row_count')::bigint;
    v_digest := e ->> 'payload_digest';
    EXECUTE format('SELECT count(*) FROM _oo.%I', t.physical_name) INTO v_rows;
    IF v_rows <> v_expect THEN
      RAISE EXCEPTION 'composed_row_count' USING ERRCODE = 'OO030';
    END IF;

    SELECT * INTO r FROM _oo.full_import_run WHERE run_id = t.run_id FOR UPDATE;
    EXECUTE format('DROP TABLE _oo.%I', t.tomb_name);
    EXECUTE format('DROP TABLE _oo.%I', t.stage_name);
    UPDATE _oo.full_import_table SET applied_rows = v_rows, applied_pos = p_l_end_pos
      WHERE run_id = t.run_id AND table_index = 0;
    v_receipt := _oo.commit_apply_receipt(p_shard, p_owner, p_target_generation, p_source, t.physical_name,
      v_batch, r.config_digest, r.type_digest, p_hook_digest, r.schema_digest, v_digest,
      'snapshot-zero/v1', 'full-import.snapshot', 0, 0, false, false, true, NULL, NULL, ''::bytea);
    PERFORM _oo.full_import_note_table(p_shard, p_owner, p_target_generation, t.run_id, 0, v_rows, v_digest, p_source);
    v_out := v_out || jsonb_build_array(jsonb_build_object('schema', t.schema_ident, 'table', t.table_ident,
      'run_id', t.run_id, 'physical', t.physical_name, 'row_count', v_rows, 'payload_digest', v_digest,
      'receipt_batch_id', v_batch, 'receipt', v_receipt));
  END LOOP;

  UPDATE _oo.native_c1_baseline_v2_format SET sealed = true WHERE source = p_source AND task_id = p_task;
  UPDATE _oo.native_c1_baseline_run SET l_end = p_l_end_pos, state = 'awaiting_publish', sealed_at = clock_timestamp()
    WHERE source = p_source AND task_id = p_task;

  RETURN jsonb_build_object('protocol', 'oo.pipeline.native-c1-baseline-stage/v2', 'outcome', 'sealed',
    'source', p_source, 'task_id', p_task, 'source_epoch', p_epoch, 'l0', v.l0,
    'l_end', p_l_end, 'applied_pos', _oo.native_pos_text(p_l_end_pos), 'tables', v_out);
END
$v2$;

-- Rebind a v2 run to the current lease generation and fence generation (N1).
-- The v1 rebind adopts the lease. Full-import runs that are still open are then
-- rebound to the caller's fence. The caller has bound its session first.
CREATE FUNCTION _oo.native_c1_baseline_v2_rebind(
  p_source text, p_task uuid, p_generation bigint,
  p_shard text, p_owner uuid, p_target_generation bigint
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
DECLARE
  result jsonb; t _oo.native_c1_baseline_v2_table%ROWTYPE; c _oo.native_c1_baseline_chunk%ROWTYPE;
  b _oo.native_c1_baseline_batch%ROWTYPE; n bigint; arr jsonb := '[]'::jsonb;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(p_source, p_task);
  IF NOT EXISTS (SELECT 1 FROM _oo.native_c1_baseline_v2_format WHERE source = p_source AND task_id = p_task) THEN
    RAISE EXCEPTION 'format_mismatch' USING ERRCODE = 'OO030';
  END IF;
  result := _oo.native_c1_baseline_rebind(p_source, p_task, p_generation);
  IF result IS NULL THEN
    RAISE EXCEPTION 'C1 v2 baseline is not open' USING ERRCODE = 'OO010';
  END IF;
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_target_generation);
  UPDATE _oo.full_import_run r SET shard_id = p_shard, owner_token = p_owner, target_generation = p_target_generation
    FROM _oo.native_c1_baseline_v2_table v
    WHERE v.run_id = r.run_id AND v.source = p_source AND v.task_id = p_task AND r.state = 'open'
      AND (r.shard_id, r.owner_token, r.target_generation) IS DISTINCT FROM (p_shard, p_owner, p_target_generation);

  FOR t IN SELECT * FROM _oo.native_c1_baseline_v2_table
           WHERE source = p_source AND task_id = p_task ORDER BY schema_ident, table_ident LOOP
    SELECT * INTO c FROM _oo.native_c1_baseline_chunk
     WHERE source = p_source AND task_id = p_task AND schema_ident = t.schema_ident AND table_ident = t.table_ident
     ORDER BY chunk_no DESC LIMIT 1;
    SELECT COALESCE(sum(x.row_count), 0) INTO n FROM _oo.native_c1_baseline_chunk x
     WHERE x.source = p_source AND x.task_id = p_task AND x.schema_ident = t.schema_ident AND x.table_ident = t.table_ident;
    arr := arr || jsonb_build_array(jsonb_build_object('schema', t.schema_ident, 'table', t.table_ident,
      'physical', t.physical_name, 'run_id', t.run_id, 'last_chunk_no', COALESCE(c.chunk_no, 0),
      'last_key_hi', _oo.native_pos_text(c.key_hi), 'terminal', COALESCE(c.terminal, false), 'rows', n));
  END LOOP;
  SELECT * INTO b FROM _oo.native_c1_baseline_batch WHERE source = p_source AND task_id = p_task
   ORDER BY batch_no DESC LIMIT 1;

  RETURN result || jsonb_build_object('format', 'chunked-typed/v2', 'tables', arr,
    'batches', jsonb_build_object('last_batch_no', COALESCE(b.batch_no, 0),
      'last_pos', CASE WHEN b.batch_no IS NULL THEN NULL ELSE _oo.native_pos_text(b.last_pos) END));
END
$v2$;

-- Privileges. Each function is owned by oo_target_owner, so the PUBLIC default is
-- revoked explicitly. Only the loader entry points are granted to the runtime.
REVOKE ALL ON FUNCTION _oo.native_c1_baseline_lock(text, uuid),
  _oo.native_c1_baseline_v2_assert(text, uuid, bigint, integer),
  _oo.native_c1_baseline_v2_reg(text, uuid, text, text),
  _oo.native_c1_baseline_v2_collist(text[], text),
  _oo.native_c1_baseline_v2_setlist(text[]),
  _oo.native_c1_baseline_v2_guard_row(),
  _oo.native_c1_baseline_v2_guard_txn(),
  _oo.native_c1_baseline_v2_guard_run(),
  _oo.native_c1_baseline_v2_open(text, uuid, bigint, integer, text, text, text, text, numeric),
  _oo.native_c1_baseline_v2_open_target(text, uuid, bigint, integer, text, uuid, bigint, bigint, text, text, text, text, text, text[], bigint[], integer[], boolean[], text),
  _oo.native_c1_baseline_v2_chunk_core(text, uuid, bigint, integer, text, text, integer, integer, numeric, numeric, boolean, bigint, text),
  _oo.native_c1_baseline_v2_chunk_apply(text, uuid, bigint, integer, text, text, integer, integer, numeric, numeric, boolean, bigint, text),
  _oo.native_c1_baseline_v2_clear_cdc(text, uuid, integer),
  _oo.native_c1_baseline_v2_batch_core(text, uuid, bigint, integer, integer, numeric, numeric, integer, bigint, text, jsonb),
  _oo.native_c1_baseline_v2_txn_batch(text, uuid, bigint, integer, integer, numeric, numeric, integer, bigint, text, jsonb),
  _oo.native_c1_baseline_v2_seal(text, uuid, bigint, integer, text, uuid, bigint, text, numeric, text, jsonb),
  _oo.native_c1_baseline_v2_rebind(text, uuid, bigint, text, uuid, bigint)
  FROM PUBLIC;

GRANT EXECUTE ON FUNCTION _oo.native_c1_baseline_lock(text, uuid),
  _oo.native_c1_baseline_v2_open(text, uuid, bigint, integer, text, text, text, text, numeric),
  _oo.native_c1_baseline_v2_open_target(text, uuid, bigint, integer, text, uuid, bigint, bigint, text, text, text, text, text, text[], bigint[], integer[], boolean[], text),
  _oo.native_c1_baseline_v2_chunk_apply(text, uuid, bigint, integer, text, text, integer, integer, numeric, numeric, boolean, bigint, text),
  _oo.native_c1_baseline_v2_txn_batch(text, uuid, bigint, integer, integer, numeric, numeric, integer, bigint, text, jsonb),
  _oo.native_c1_baseline_v2_seal(text, uuid, bigint, integer, text, uuid, bigint, text, numeric, text, jsonb),
  _oo.native_c1_baseline_v2_rebind(text, uuid, bigint, text, uuid, bigint)
  TO oo_pipeline_runtime;
