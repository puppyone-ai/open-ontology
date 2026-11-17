-- docs-sources-jobs/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-bootstrap/1,
-- inside the installer transaction (advisory lock 6413202611). No BEGIN or
-- COMMIT here; the installer inserts the schema_migrations row.
--
-- Source, file, and job tables plus the job and event functions (10 §3.2,
-- §4.1, §8.5). The functions are owner-owned SECURITY DEFINER with a fixed
-- search_path and are the only way the runtime role changes job, event, and
-- event_head rows: it has SELECT on job and event and nothing else there.
--
-- The functions never COMMIT. A stage may call complete() or retry() inside
-- the same transaction as its own writes (4.1.3). Lease length is 30 s from
-- clock_timestamp(); heartbeat extends 30 s and takes no TTL. Every lease
-- operation compares (owner, generation); a worker that lost its lease gets
-- lost_lease and must discard its result.

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs sources/jobs migration must run as the owner'
      USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (
    SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-bootstrap/1'
  ) THEN
    RAISE EXCEPTION 'docs-bootstrap/1 is not installed'
      USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

CREATE TABLE _oo_docs.drive (
  id uuid PRIMARY KEY,
  name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 128),
  backend text NOT NULL CHECK (backend IN ('fs', 's3', 'oss', 'cos', 'r2')),
  config jsonb NOT NULL,
  model_enabled boolean NOT NULL DEFAULT false,
  llm_daily_calls integer NOT NULL DEFAULT 2000 CHECK (llm_daily_calls BETWEEN 0 AND 1000000),
  max_files integer NOT NULL DEFAULT 10000 CHECK (max_files BETWEEN 1 AND 10000),
  sync_generation bigint NOT NULL DEFAULT 0,
  status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'paused', 'deleting', 'deleted')),
  status_code text,
  revision bigint NOT NULL DEFAULT 1 CHECK (revision >= 1),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  CONSTRAINT drive_config_no_inline_secret CHECK (
    NOT (config::text ~* '"(secret|password|access_key_secret|secret_access_key)"\s*:\s*"')
  )
);
CREATE UNIQUE INDEX drive_name_live ON _oo_docs.drive (lower(name)) WHERE status <> 'deleted';

CREATE TABLE _oo_docs.drive_sync (
  drive_id uuid NOT NULL REFERENCES _oo_docs.drive(id),
  generation bigint NOT NULL,
  trigger text NOT NULL CHECK (trigger IN ('poll', 'manual', 'event', 'startup', 'audit')),
  status text NOT NULL CHECK (status IN ('running', 'complete', 'failed', 'held')),
  listed bigint NOT NULL DEFAULT 0,
  pages integer NOT NULL DEFAULT 0,
  added integer NOT NULL DEFAULT 0,
  changed integer NOT NULL DEFAULT 0,
  removed integer NOT NULL DEFAULT 0,
  renamed integer NOT NULL DEFAULT 0,
  error_code text,
  started_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  finished_at timestamptz,
  PRIMARY KEY (drive_id, generation)
);

CREATE TABLE _oo_docs.blob (
  b3 text PRIMARY KEY CHECK (b3 ~ '^[0-9a-f]{64}$'),
  sha256 text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
  size bigint NOT NULL CHECK (size >= 0),
  format text CHECK (format IN ('md', 'txt', 'csv', 'docx', 'xlsx', 'pptx', 'pdf', 'pdf_scanned',
                                'image', 'legacy_office', 'zip', 'unknown')),
  encrypted boolean NOT NULL DEFAULT false,
  has_macros boolean NOT NULL DEFAULT false,
  cached boolean NOT NULL DEFAULT false,
  first_seen timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE _oo_docs.file (
  id uuid PRIMARY KEY,
  drive_id uuid NOT NULL REFERENCES _oo_docs.drive(id),
  path text NOT NULL CHECK (
    octet_length(path) BETWEEN 1 AND 1024 AND path !~ '(^/|//|(^|/)\.\.?(/|$))'
  ),
  raw_key bytea NOT NULL,
  path_fold text NOT NULL,
  state text NOT NULL CHECK (state IN ('stored', 'queued', 'parsing', 'parsed', 'extracting', 'indexed',
         'published', 'parse_failed', 'extract_partial', 'unsupported', 'too_large', 'encrypted',
         'duplicate', 'superseded', 'deleted', 'outdated')),
  state_since timestamptz NOT NULL DEFAULT clock_timestamp(),
  state_code text,
  retryable boolean NOT NULL DEFAULT false,
  blocked_reason text CHECK (blocked_reason IN ('parser_unavailable', 'disk_low', 'db_degraded',
         'llm_limit', 'drive_paused')),
  progress jsonb,
  current_version_id uuid,
  pending_review integer NOT NULL DEFAULT 0 CHECK (pending_review >= 0),
  fp_size bigint,
  fp_etag text,
  fp_mtime_ns bigint,
  fp_version_id text,
  fp_ino bigint,
  last_seen_generation bigint NOT NULL,
  deleted_at timestamptz,
  revision bigint NOT NULL DEFAULT 1,
  UNIQUE (drive_id, raw_key)
);
CREATE UNIQUE INDEX file_live_path ON _oo_docs.file (drive_id, path) WHERE deleted_at IS NULL;
CREATE INDEX file_drive_state ON _oo_docs.file (drive_id, state, path);
CREATE INDEX file_seen ON _oo_docs.file (drive_id, last_seen_generation) WHERE deleted_at IS NULL;
CREATE INDEX file_path_prefix ON _oo_docs.file (drive_id, path text_pattern_ops);

CREATE TABLE _oo_docs.file_version (
  id uuid PRIMARY KEY,
  file_id uuid NOT NULL REFERENCES _oo_docs.file(id),
  b3 text NOT NULL REFERENCES _oo_docs.blob(b3),
  path_at_version text NOT NULL,
  fp jsonb NOT NULL,
  duplicate_of uuid REFERENCES _oo_docs.file_version(id),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  superseded_at timestamptz,
  deleted_at timestamptz
);
CREATE INDEX file_version_b3 ON _oo_docs.file_version (b3);
CREATE UNIQUE INDEX file_version_live ON _oo_docs.file_version (file_id)
  WHERE superseded_at IS NULL AND deleted_at IS NULL;

CREATE TABLE _oo_docs.parse_result (
  b3 text NOT NULL REFERENCES _oo_docs.blob(b3),
  parser_id text NOT NULL,
  parser_version text NOT NULL,
  profile_sha256 text NOT NULL,
  status text NOT NULL CHECK (status IN ('running', 'complete', 'failed')),
  ir_b3 text,
  block_count integer,
  page_count integer,
  windows_done int4range[],
  error_code text,
  crash_count integer NOT NULL DEFAULT 0,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  finished_at timestamptz,
  PRIMARY KEY (b3, parser_id, parser_version, profile_sha256)
);

CREATE TABLE _oo_docs.file_state_history (
  file_id uuid NOT NULL REFERENCES _oo_docs.file(id),
  seq bigint NOT NULL,
  version_id uuid,
  from_state text,
  to_state text NOT NULL,
  code text,
  attempt integer,
  detail jsonb,
  at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (file_id, seq)
);
CREATE TRIGGER file_state_history_insert_only
  BEFORE UPDATE OR DELETE ON _oo_docs.file_state_history
  FOR EACH ROW EXECUTE FUNCTION _oo_docs.reject_mutation();
CREATE TRIGGER file_state_history_no_truncate
  BEFORE TRUNCATE ON _oo_docs.file_state_history
  FOR EACH STATEMENT EXECUTE FUNCTION _oo_docs.reject_mutation();

CREATE TABLE _oo_docs.job (
  id uuid PRIMARY KEY,
  kind text NOT NULL CHECK (kind IN ('sync', 'hash', 'parse', 'normalize', 'extract_rules', 'extract_llm',
        'resolve', 'publish', 'compile', 'gc', 'expire', 'drift', 'reprocess')),
  drive_id uuid REFERENCES _oo_docs.drive(id),
  file_id uuid REFERENCES _oo_docs.file(id),
  version_id uuid REFERENCES _oo_docs.file_version(id),
  dedupe_key text NOT NULL CHECK (octet_length(dedupe_key) <= 512),
  priority smallint NOT NULL DEFAULT 100,
  state text NOT NULL CHECK (state IN ('queued', 'running', 'succeeded', 'failed', 'cancelled', 'quarantined')),
  run_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  attempt integer NOT NULL DEFAULT 0,
  max_attempts integer NOT NULL CHECK (max_attempts BETWEEN 1 AND 32),
  crash_count integer NOT NULL DEFAULT 0,
  lease_owner text,
  lease_until timestamptz,
  generation bigint NOT NULL DEFAULT 0,
  cancel_requested boolean NOT NULL DEFAULT false,
  blocked_reason text,
  last_error text CHECK (octet_length(last_error) <= 2048),
  checkpoint jsonb,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  CONSTRAINT job_lease_shape CHECK (
    (state = 'running') = (lease_owner IS NOT NULL AND lease_until IS NOT NULL)
  ),
  UNIQUE (dedupe_key)
);
CREATE INDEX job_due ON _oo_docs.job (kind, priority, run_at, id) WHERE state = 'queued';
CREATE INDEX job_expired ON _oo_docs.job (lease_until) WHERE state = 'running';

CREATE TABLE _oo_docs.event_head (
  singleton boolean PRIMARY KEY CHECK (singleton),
  seq bigint NOT NULL CHECK (seq >= 0)
);
INSERT INTO _oo_docs.event_head VALUES (true, 0);

CREATE TABLE _oo_docs.event (
  seq bigint PRIMARY KEY CHECK (seq >= 1),
  type text NOT NULL CHECK (type ~ '^[a-z_]+\.[a-z_]+$'),
  drive_id uuid,
  subject text,
  payload jsonb NOT NULL CHECK (octet_length(payload::text) <= 16384),
  at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX event_at ON _oo_docs.event (at);
CREATE TRIGGER event_insert_only
  BEFORE UPDATE OR DELETE ON _oo_docs.event
  FOR EACH ROW EXECUTE FUNCTION _oo_docs.reject_mutation();
CREATE TRIGGER event_no_truncate
  BEFORE TRUNCATE ON _oo_docs.event
  FOR EACH STATEMENT EXECUTE FUNCTION _oo_docs.reject_mutation();

CREATE TABLE _oo_docs.api_idempotency (
  key text NOT NULL CHECK (key ~ '^[A-Za-z0-9_-]{16,128}$'),
  route text NOT NULL,
  body_sha256 text NOT NULL CHECK (body_sha256 ~ '^[0-9a-f]{64}$'),
  status smallint NOT NULL,
  response jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (route, key)
);

-- Runtime grants. job, event, event_head: no DML. Everything that changes
-- them goes through the functions below.
GRANT SELECT, INSERT, UPDATE ON _oo_docs.drive, _oo_docs.drive_sync, _oo_docs.blob,
  _oo_docs.file, _oo_docs.file_version, _oo_docs.parse_result TO oo_docs_runtime;
GRANT SELECT, INSERT ON _oo_docs.file_state_history TO oo_docs_runtime;
GRANT SELECT, INSERT, DELETE ON _oo_docs.api_idempotency TO oo_docs_runtime;
GRANT SELECT ON _oo_docs.job, _oo_docs.event TO oo_docs_runtime;

-- ---------------------------------------------------------------------------
-- Job and event functions (10 §3.2). SECURITY DEFINER, fixed search_path.
-- Errors use static text. Input faults raise OO010.
-- ---------------------------------------------------------------------------

CREATE TYPE _oo_docs.job_result AS (
  status text,
  job_id uuid,
  kind text,
  state text,
  attempt integer,
  generation bigint,
  lease_owner text,
  lease_until timestamptz,
  run_at timestamptz,
  crash_count integer,
  cancel_requested boolean,
  last_error text,
  checkpoint jsonb,
  drive_id uuid,
  file_id uuid,
  version_id uuid,
  dedupe_key text,
  priority smallint,
  max_attempts integer
);

CREATE FUNCTION _oo_docs._job_result(p_status text, p_job _oo_docs.job)
RETURNS _oo_docs.job_result
LANGUAGE sql
IMMUTABLE
SECURITY INVOKER
SET search_path = pg_catalog, _oo_docs
AS $$
  SELECT ROW(
    p_status, p_job.id, p_job.kind, p_job.state, p_job.attempt, p_job.generation,
    p_job.lease_owner, p_job.lease_until, p_job.run_at, p_job.crash_count,
    p_job.cancel_requested, p_job.last_error, p_job.checkpoint, p_job.drive_id,
    p_job.file_id, p_job.version_id, p_job.dedupe_key, p_job.priority, p_job.max_attempts
  )::_oo_docs.job_result
$$;

-- Server-side cap on a caller supplied retry delay (4.1.4 table).
CREATE FUNCTION _oo_docs._retry_delay_cap_ms(p_kind text)
RETURNS bigint
LANGUAGE sql
IMMUTABLE
SECURITY INVOKER
SET search_path = pg_catalog
AS $$
  SELECT CASE p_kind
    WHEN 'sync' THEN 300000
    WHEN 'hash' THEN 600000
    WHEN 'parse' THEN 1800000
    WHEN 'normalize' THEN 600000
    WHEN 'extract_rules' THEN 600000
    WHEN 'extract_llm' THEN 300000
    WHEN 'publish' THEN 30000
    WHEN 'compile' THEN 30000
    ELSE 600000
  END::bigint
$$;

CREATE FUNCTION _oo_docs._check_lease_args(p_owner text, p_generation bigint)
RETURNS void
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog
AS $$
BEGIN
  IF p_owner IS NULL OR p_generation IS NULL OR p_generation < 1
     OR p_owner !~ '^[A-Za-z0-9_.:/@-]{1,128}$'
  THEN
    RAISE EXCEPTION 'invalid job operation input' USING ERRCODE = 'OO010';
  END IF;
END
$$;

CREATE FUNCTION _oo_docs._check_code(p_code text)
RETURNS void
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog
AS $$
BEGIN
  IF p_code IS NULL OR p_code !~ '^[A-Za-z0-9_.:-]{1,96}$' THEN
    RAISE EXCEPTION 'invalid job operation input' USING ERRCODE = 'OO010';
  END IF;
END
$$;

-- enqueue: same dedupe_key, non terminal -> replayed; succeeded -> already_done;
-- failed, cancelled, quarantined -> terminal (caller adds retry#n to the key).
CREATE FUNCTION _oo_docs.enqueue(
  p_id uuid,
  p_kind text,
  p_drive_id uuid,
  p_file_id uuid,
  p_version_id uuid,
  p_dedupe_key text,
  p_priority smallint,
  p_max_attempts integer,
  p_run_at timestamptz
)
RETURNS _oo_docs.job_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo_docs
AS $$
DECLARE
  v_row _oo_docs.job%ROWTYPE;
BEGIN
  IF p_kind IS NULL OR p_dedupe_key IS NULL OR p_max_attempts IS NULL
     OR octet_length(p_dedupe_key) < 1 OR octet_length(p_dedupe_key) > 512
     OR p_max_attempts < 1 OR p_max_attempts > 32
     OR p_run_at IN ('infinity'::timestamptz, '-infinity'::timestamptz)
  THEN
    RAISE EXCEPTION 'invalid job operation input' USING ERRCODE = 'OO010';
  END IF;

  BEGIN
    INSERT INTO _oo_docs.job (id, kind, drive_id, file_id, version_id, dedupe_key,
                              priority, state, run_at, max_attempts)
    VALUES (COALESCE(p_id, gen_random_uuid()), p_kind, p_drive_id, p_file_id, p_version_id,
            p_dedupe_key, COALESCE(p_priority, 100::smallint), 'queued',
            COALESCE(p_run_at, clock_timestamp()), p_max_attempts)
    ON CONFLICT (dedupe_key) DO NOTHING
    RETURNING * INTO v_row;
  EXCEPTION
    WHEN check_violation OR not_null_violation OR foreign_key_violation THEN
      RAISE EXCEPTION 'invalid job operation input' USING ERRCODE = 'OO010';
    WHEN unique_violation THEN
      RAISE EXCEPTION 'job operation failed' USING ERRCODE = 'OO016';
  END;
  IF FOUND THEN
    RETURN _oo_docs._job_result('enqueued', v_row);
  END IF;

  SELECT * INTO v_row FROM _oo_docs.job WHERE dedupe_key = p_dedupe_key;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'job operation failed' USING ERRCODE = 'OO016';
  END IF;
  IF v_row.state IN ('queued', 'running') THEN
    RETURN _oo_docs._job_result('replayed', v_row);
  ELSIF v_row.state = 'succeeded' THEN
    RETURN _oo_docs._job_result('already_done', v_row);
  END IF;
  RETURN _oo_docs._job_result('terminal', v_row);
END
$$;

-- claim: expired leases go back first (at most 8 rows per call); then up to
-- p_limit due queued rows are leased in priority, run_at, id order.
CREATE FUNCTION _oo_docs.claim(p_kinds text[], p_owner text, p_limit integer)
RETURNS SETOF _oo_docs.job_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo_docs
AS $$
DECLARE
  v_row _oo_docs.job%ROWTYPE;
  v_upd _oo_docs.job%ROWTYPE;
  v_step integer;
BEGIN
  IF p_kinds IS NULL OR p_owner IS NULL OR p_limit IS NULL
     OR cardinality(p_kinds) < 1 OR cardinality(p_kinds) > 64
     OR p_limit < 1 OR p_limit > 64
     OR p_owner !~ '^[A-Za-z0-9_.:/@-]{1,128}$'
     OR EXISTS (SELECT 1 FROM unnest(p_kinds) AS k WHERE k IS NULL)
  THEN
    RAISE EXCEPTION 'invalid job operation input' USING ERRCODE = 'OO010';
  END IF;

  FOR v_step IN 1..8 LOOP
    SELECT * INTO v_row
    FROM _oo_docs.job
    WHERE state = 'running' AND lease_until <= clock_timestamp() AND kind = ANY (p_kinds)
    ORDER BY lease_until, id
    FOR UPDATE SKIP LOCKED
    LIMIT 1;
    EXIT WHEN NOT FOUND;

    IF v_row.cancel_requested THEN
      UPDATE _oo_docs.job
      SET state = 'cancelled', lease_owner = NULL, lease_until = NULL,
          last_error = 'lease_expired_cancelled', updated_at = clock_timestamp()
      WHERE id = v_row.id;
    ELSIF v_row.attempt >= v_row.max_attempts THEN
      UPDATE _oo_docs.job
      SET state = 'failed', lease_owner = NULL, lease_until = NULL,
          last_error = 'lease_expired_attempt_cap', updated_at = clock_timestamp()
      WHERE id = v_row.id;
    ELSE
      UPDATE _oo_docs.job
      SET state = 'queued', lease_owner = NULL, lease_until = NULL,
          last_error = 'lease_expired', updated_at = clock_timestamp()
      WHERE id = v_row.id;
    END IF;
  END LOOP;

  FOR v_row IN
    SELECT * FROM _oo_docs.job
    WHERE state = 'queued' AND kind = ANY (p_kinds) AND run_at <= clock_timestamp()
    ORDER BY priority, run_at, id
    FOR UPDATE SKIP LOCKED
    LIMIT p_limit
  LOOP
    IF v_row.generation >= 9223372036854775807 THEN
      RAISE EXCEPTION 'generation overflow' USING ERRCODE = 'OO012';
    END IF;
    UPDATE _oo_docs.job
    SET state = 'running', attempt = attempt + 1, generation = generation + 1,
        lease_owner = p_owner, lease_until = clock_timestamp() + interval '30 seconds',
        last_error = NULL, updated_at = clock_timestamp()
    WHERE id = v_row.id
    RETURNING * INTO v_upd;
    RETURN NEXT _oo_docs._job_result('claimed', v_upd);
  END LOOP;
  RETURN;
END
$$;

-- heartbeat: extends only a live matching lease. Returns cancel_requested.
CREATE FUNCTION _oo_docs.heartbeat(p_id uuid, p_owner text, p_generation bigint)
RETURNS _oo_docs.job_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo_docs
AS $$
DECLARE
  v_row _oo_docs.job%ROWTYPE;
BEGIN
  IF p_id IS NULL THEN
    RAISE EXCEPTION 'invalid job operation input' USING ERRCODE = 'OO010';
  END IF;
  PERFORM _oo_docs._check_lease_args(p_owner, p_generation);

  SELECT * INTO v_row FROM _oo_docs.job WHERE id = p_id FOR UPDATE;
  IF NOT FOUND THEN
    RETURN _oo_docs._job_result('lost_lease', NULL::_oo_docs.job);
  END IF;
  IF v_row.state = 'running' AND v_row.lease_owner = p_owner
     AND v_row.generation = p_generation AND v_row.lease_until > clock_timestamp()
  THEN
    UPDATE _oo_docs.job
    SET lease_until = clock_timestamp() + interval '30 seconds', updated_at = clock_timestamp()
    WHERE id = p_id
    RETURNING * INTO v_row;
    RETURN _oo_docs._job_result('extended', v_row);
  END IF;
  IF v_row.state IN ('succeeded', 'failed', 'cancelled', 'quarantined') THEN
    RETURN _oo_docs._job_result('terminal', v_row);
  END IF;
  RETURN _oo_docs._job_result('lost_lease', v_row);
END
$$;

-- Shared tail of complete/fail/quarantine: CAS on a live lease.
CREATE FUNCTION _oo_docs._finish(
  p_id uuid, p_owner text, p_generation bigint, p_state text, p_error text
)
RETURNS _oo_docs.job_result
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog, _oo_docs
AS $$
DECLARE
  v_row _oo_docs.job%ROWTYPE;
BEGIN
  IF p_id IS NULL THEN
    RAISE EXCEPTION 'invalid job operation input' USING ERRCODE = 'OO010';
  END IF;
  PERFORM _oo_docs._check_lease_args(p_owner, p_generation);

  UPDATE _oo_docs.job
  SET state = p_state, lease_owner = NULL, lease_until = NULL,
      last_error = p_error, updated_at = clock_timestamp()
  WHERE id = p_id AND state = 'running' AND lease_owner = p_owner
    AND generation = p_generation AND lease_until > clock_timestamp()
  RETURNING * INTO v_row;
  IF FOUND THEN
    RETURN _oo_docs._job_result(p_state, v_row);
  END IF;

  SELECT * INTO v_row FROM _oo_docs.job WHERE id = p_id;
  IF NOT FOUND THEN
    RETURN _oo_docs._job_result('lost_lease', NULL::_oo_docs.job);
  END IF;
  IF v_row.state IN ('succeeded', 'failed', 'cancelled', 'quarantined') THEN
    RETURN _oo_docs._job_result('terminal', v_row);
  END IF;
  RETURN _oo_docs._job_result('lost_lease', v_row);
END
$$;

CREATE FUNCTION _oo_docs.complete(p_id uuid, p_owner text, p_generation bigint)
RETURNS _oo_docs.job_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo_docs
AS $$
BEGIN
  RETURN _oo_docs._finish(p_id, p_owner, p_generation, 'succeeded', NULL);
END
$$;

CREATE FUNCTION _oo_docs.fail(p_id uuid, p_owner text, p_generation bigint, p_code text)
RETURNS _oo_docs.job_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo_docs
AS $$
BEGIN
  PERFORM _oo_docs._check_code(p_code);
  RETURN _oo_docs._finish(p_id, p_owner, p_generation, 'failed', p_code);
END
$$;

CREATE FUNCTION _oo_docs.quarantine(p_id uuid, p_owner text, p_generation bigint, p_code text)
RETURNS _oo_docs.job_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo_docs
AS $$
BEGIN
  PERFORM _oo_docs._check_code(p_code);
  RETURN _oo_docs._finish(p_id, p_owner, p_generation, 'quarantined', p_code);
END
$$;

-- retry: back to queued after a delay clamped to the per kind cap.
--   p_count_attempt false: the attempt taken by claim is given back
--                          (PARSER_BUSY and sidecar unavailable do not count).
--   p_crash true:          crash_count + 1; at p_crash_limit, or at the attempt
--                          cap, the job is quarantined instead (4.1.6).
--   otherwise at the attempt cap the job becomes failed.
CREATE FUNCTION _oo_docs.retry(
  p_id uuid,
  p_owner text,
  p_generation bigint,
  p_code text,
  p_delay_ms bigint,
  p_count_attempt boolean DEFAULT true,
  p_crash boolean DEFAULT false,
  p_crash_limit integer DEFAULT 2
)
RETURNS _oo_docs.job_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo_docs
AS $$
DECLARE
  v_row _oo_docs.job%ROWTYPE;
  v_delay bigint;
  v_crash integer;
  v_attempt integer;
BEGIN
  IF p_id IS NULL OR p_delay_ms IS NULL OR p_count_attempt IS NULL OR p_crash IS NULL
     OR p_crash_limit IS NULL OR p_crash_limit < 1 OR p_crash_limit > 32
  THEN
    RAISE EXCEPTION 'invalid job operation input' USING ERRCODE = 'OO010';
  END IF;
  PERFORM _oo_docs._check_lease_args(p_owner, p_generation);
  PERFORM _oo_docs._check_code(p_code);

  SELECT * INTO v_row
  FROM _oo_docs.job
  WHERE id = p_id AND state = 'running' AND lease_owner = p_owner
    AND generation = p_generation AND lease_until > clock_timestamp()
  FOR UPDATE;
  IF NOT FOUND THEN
    SELECT * INTO v_row FROM _oo_docs.job WHERE id = p_id;
    IF NOT FOUND THEN
      RETURN _oo_docs._job_result('lost_lease', NULL::_oo_docs.job);
    END IF;
    IF v_row.state IN ('succeeded', 'failed', 'cancelled', 'quarantined') THEN
      RETURN _oo_docs._job_result('terminal', v_row);
    END IF;
    RETURN _oo_docs._job_result('lost_lease', v_row);
  END IF;

  v_crash := v_row.crash_count + CASE WHEN p_crash THEN 1 ELSE 0 END;
  v_attempt := CASE WHEN p_count_attempt THEN v_row.attempt ELSE greatest(v_row.attempt - 1, 0) END;

  IF p_crash AND (v_crash >= p_crash_limit OR v_row.attempt >= v_row.max_attempts) THEN
    UPDATE _oo_docs.job
    SET state = 'quarantined', crash_count = v_crash, lease_owner = NULL, lease_until = NULL,
        last_error = p_code, updated_at = clock_timestamp()
    WHERE id = p_id RETURNING * INTO v_row;
    RETURN _oo_docs._job_result('quarantined', v_row);
  END IF;
  IF p_count_attempt AND v_row.attempt >= v_row.max_attempts THEN
    UPDATE _oo_docs.job
    SET state = 'failed', crash_count = v_crash, lease_owner = NULL, lease_until = NULL,
        last_error = p_code, updated_at = clock_timestamp()
    WHERE id = p_id RETURNING * INTO v_row;
    RETURN _oo_docs._job_result('exhausted', v_row);
  END IF;

  v_delay := least(greatest(p_delay_ms, 0), _oo_docs._retry_delay_cap_ms(v_row.kind));
  UPDATE _oo_docs.job
  SET state = 'queued', attempt = v_attempt, crash_count = v_crash,
      lease_owner = NULL, lease_until = NULL, last_error = p_code,
      run_at = clock_timestamp() + v_delay * interval '1 millisecond',
      updated_at = clock_timestamp()
  WHERE id = p_id RETURNING * INTO v_row;
  RETURN _oo_docs._job_result('retry_scheduled', v_row);
END
$$;

-- cancel: queued -> cancelled; running -> cancel_requested; terminal unchanged.
CREATE FUNCTION _oo_docs.cancel(p_id uuid)
RETURNS _oo_docs.job_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo_docs
AS $$
DECLARE
  v_row _oo_docs.job%ROWTYPE;
BEGIN
  IF p_id IS NULL THEN
    RAISE EXCEPTION 'invalid job operation input' USING ERRCODE = 'OO010';
  END IF;
  SELECT * INTO v_row FROM _oo_docs.job WHERE id = p_id FOR UPDATE;
  IF NOT FOUND THEN
    RETURN _oo_docs._job_result('not_found', NULL::_oo_docs.job);
  END IF;
  IF v_row.state = 'queued' THEN
    UPDATE _oo_docs.job SET state = 'cancelled', last_error = 'cancelled',
      updated_at = clock_timestamp() WHERE id = p_id RETURNING * INTO v_row;
    RETURN _oo_docs._job_result('cancelled', v_row);
  ELSIF v_row.state = 'running' THEN
    UPDATE _oo_docs.job SET cancel_requested = true,
      updated_at = clock_timestamp() WHERE id = p_id RETURNING * INTO v_row;
    RETURN _oo_docs._job_result('cancel_requested', v_row);
  END IF;
  RETURN _oo_docs._job_result('terminal', v_row);
END
$$;

-- ack_cancel: the worker saw cancel_requested and stops. CAS on a live lease;
-- only valid after cancel() flagged the job. Partial products stay (4.1.5).
CREATE FUNCTION _oo_docs.ack_cancel(p_id uuid, p_owner text, p_generation bigint)
RETURNS _oo_docs.job_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo_docs
AS $$
DECLARE
  v_row _oo_docs.job%ROWTYPE;
BEGIN
  IF p_id IS NULL THEN
    RAISE EXCEPTION 'invalid job operation input' USING ERRCODE = 'OO010';
  END IF;
  PERFORM _oo_docs._check_lease_args(p_owner, p_generation);
  SELECT * INTO v_row FROM _oo_docs.job
  WHERE id = p_id AND state = 'running' AND lease_owner = p_owner
    AND generation = p_generation AND lease_until > clock_timestamp()
  FOR UPDATE;
  IF FOUND THEN
    IF NOT v_row.cancel_requested THEN
      RETURN _oo_docs._job_result('not_requested', v_row);
    END IF;
    RETURN _oo_docs._finish(p_id, p_owner, p_generation, 'cancelled', 'cancelled');
  END IF;
  RETURN _oo_docs._finish(p_id, p_owner, p_generation, 'cancelled', 'cancelled');
END
$$;

-- save_checkpoint: resume state (4.1.8), CAS on the live lease.
CREATE FUNCTION _oo_docs.save_checkpoint(
  p_id uuid, p_owner text, p_generation bigint, p_checkpoint jsonb
)
RETURNS _oo_docs.job_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo_docs
AS $$
DECLARE
  v_row _oo_docs.job%ROWTYPE;
BEGIN
  IF p_id IS NULL OR p_checkpoint IS NULL OR octet_length(p_checkpoint::text) > 65536 THEN
    RAISE EXCEPTION 'invalid job operation input' USING ERRCODE = 'OO010';
  END IF;
  PERFORM _oo_docs._check_lease_args(p_owner, p_generation);
  UPDATE _oo_docs.job
  SET checkpoint = p_checkpoint, updated_at = clock_timestamp()
  WHERE id = p_id AND state = 'running' AND lease_owner = p_owner
    AND generation = p_generation AND lease_until > clock_timestamp()
  RETURNING * INTO v_row;
  IF FOUND THEN
    RETURN _oo_docs._job_result('saved', v_row);
  END IF;
  RETURN _oo_docs._job_result('lost_lease', NULL::_oo_docs.job);
END
$$;

-- append_event: seq is assigned under the event_head row lock, held to
-- commit, so seq order is commit order and a `since` cursor cannot skip a
-- visible event. Call it last in a transaction (4.1.3).
CREATE FUNCTION _oo_docs.append_event(
  p_type text, p_drive_id uuid, p_subject text, p_payload jsonb
)
RETURNS bigint
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo_docs
AS $$
DECLARE
  v_seq bigint;
BEGIN
  IF p_type IS NULL OR p_type !~ '^[a-z_]+\.[a-z_]+$'
     OR p_payload IS NULL OR jsonb_typeof(p_payload) IS DISTINCT FROM 'object'
     OR octet_length(p_payload::text) > 16384
     OR (p_subject IS NOT NULL AND octet_length(p_subject) > 256)
     OR EXISTS (
       SELECT 1 FROM jsonb_object_keys(p_payload) AS k
       WHERE k = ANY (ARRAY['password', 'passwd', 'secret', 'credential', 'credentials',
                            'api_key', 'access_token', 'private_key', 'authorization']::text[])
     )
  THEN
    RAISE EXCEPTION 'invalid event input' USING ERRCODE = 'OO010';
  END IF;
  UPDATE _oo_docs.event_head SET seq = seq + 1 WHERE singleton RETURNING seq INTO v_seq;
  INSERT INTO _oo_docs.event (seq, type, drive_id, subject, payload)
  VALUES (v_seq, p_type, p_drive_id, p_subject, p_payload);
  PERFORM pg_catalog.pg_notify('oo_docs_events', v_seq::text);
  RETURN v_seq;
END
$$;

-- Only the public entry points are executable by the runtime role.
DO $grants$
DECLARE
  v_fn regprocedure;
BEGIN
  FOR v_fn IN
    SELECT p.oid::regprocedure
    FROM pg_catalog.pg_proc AS p
    WHERE p.pronamespace = '_oo_docs'::regnamespace
  LOOP
    EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC', v_fn);
  END LOOP;
  FOR v_fn IN
    SELECT p.oid::regprocedure
    FROM pg_catalog.pg_proc AS p
    WHERE p.pronamespace = '_oo_docs'::regnamespace
      AND p.proname = ANY (ARRAY['enqueue', 'claim', 'heartbeat', 'complete', 'fail', 'retry',
                                 'quarantine', 'cancel', 'ack_cancel', 'save_checkpoint',
                                 'append_event']::text[])
  LOOP
    EXECUTE format('GRANT EXECUTE ON FUNCTION %s TO oo_docs_runtime', v_fn);
  END LOOP;
END
$grants$;
