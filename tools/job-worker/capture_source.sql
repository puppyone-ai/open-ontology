-- Source capture handler state owned by job-worker.
-- This file does not create or replace a jobs function and it does not
-- alter oo-jobs or oo-pipeline migrations.
-- The binary path, digest, and command are fixed. secret_ref is a directory
-- or the one approved secret://oo-dl01w1-43/reader reference.
-- The password file is not a column.

CREATE TABLE IF NOT EXISTS jw.capture_sources (
  binding_id uuid PRIMARY KEY REFERENCES jw.capture_bindings (id),
  driver_name text NOT NULL,
  driver_commit text NOT NULL,
  driver_sha256 text NOT NULL,
  binary_rel text NOT NULL,
  secret_ref text NOT NULL,
  source_host text NOT NULL,
  source_port integer NOT NULL,
  evidence_rel text NOT NULL,
  command text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  CONSTRAINT jw_src_name CHECK (driver_name = 'doris-reader'),
  CONSTRAINT jw_src_commit CHECK (driver_commit IN (
    'a5aed1167caa54e68e3a2be7335142465fb2d31a',
    'd9255c6a69a9b534a5f5c792a964c43023764f58'
  )),
  CONSTRAINT jw_src_sha CHECK (
    driver_sha256 = 'd5dc8555ad76ecc9d4550ac5d2abd6b00bb2fee6ca5a17c7c206d294b206010c'
  ),
  CONSTRAINT jw_src_bin CHECK (
    binary_rel = 'evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/capture-live/build/doris-reader'
  ),
  CONSTRAINT jw_src_secret CHECK (
    (
      secret_ref ~ '^/([A-Za-z0-9._-]+/)*[A-Za-z0-9._-]+$'
      OR secret_ref = 'secret://oo-dl01w1-43/reader'
    )
    AND octet_length(secret_ref) BETWEEN 2 AND 256
    AND secret_ref !~ 'reader[.]password'
  ),
  CONSTRAINT jw_src_host CHECK (source_host ~ '^[0-9]{1,3}(\.[0-9]{1,3}){3}$'),
  CONSTRAINT jw_src_port CHECK (source_port BETWEEN 1 AND 65535),
  CONSTRAINT jw_src_evidence CHECK (
    evidence_rel ~ '^linux/run-source-[a-z0-9-]{1,40}/[a-z0-9-]{1,40}$'
  ),
  CONSTRAINT jw_src_command CHECK (command = 'capture-tick')
);

CREATE TABLE IF NOT EXISTS jw.capture_source_install (
  id integer PRIMARY KEY,
  checksum text NOT NULL,
  CONSTRAINT jw_src_install_one CHECK (id = 1),
  CONSTRAINT jw_src_install_checksum CHECK (checksum ~ '^[0-9a-f]{64}$')
);

REVOKE ALL ON jw.capture_sources, jw.capture_source_install FROM PUBLIC;
ALTER TABLE jw.capture_sources DROP CONSTRAINT IF EXISTS jw_src_secret;
ALTER TABLE jw.capture_sources ADD CONSTRAINT jw_src_secret CHECK (
  (
    secret_ref ~ '^/([A-Za-z0-9._-]+/)*[A-Za-z0-9._-]+$'
    OR secret_ref = 'secret://oo-dl01w1-43/reader'
  )
  AND octet_length(secret_ref) BETWEEN 2 AND 256
  AND secret_ref !~ 'reader[.]password'
);

GRANT SELECT, INSERT, DELETE ON jw.capture_sources TO oo_jobs_runtime;
GRANT UPDATE (secret_ref, source_host, source_port, evidence_rel, updated_at)
  ON jw.capture_sources TO oo_jobs_runtime;
GRANT SELECT ON jw.capture_source_install TO oo_jobs_runtime;

-- Schedule API owned by this worker. jobs.schedules stays the published row
-- (no period column, no payload column, no new jobs function). Period and
-- the closed payload live here and are applied through these functions.

CREATE TABLE IF NOT EXISTS jw.schedule_terms (
  schedule_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  period_ms integer NOT NULL,
  payload jsonb NOT NULL,
  enabled boolean NOT NULL,
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  CONSTRAINT jw_term_period CHECK (period_ms BETWEEN 1000 AND 86400000),
  CONSTRAINT jw_term_payload_obj CHECK (jsonb_typeof(payload) = 'object'),
  CONSTRAINT jw_term_payload_len CHECK (octet_length(payload::text) BETWEEN 2 AND 16384),
  CONSTRAINT jw_term_secret CHECK (
    NOT (
      payload ? 'secret'
      OR payload ? 'secret_ref'
      OR payload ? 'password'
      OR payload ? 'argv'
      OR payload ? 'shell'
    )
    AND payload::text NOT LIKE '%secret://%'
    AND payload::text NOT LIKE '%reader.password%'
  )
);

CREATE OR REPLACE FUNCTION jw.schedule_payload_closed(p_payload jsonb)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
SECURITY INVOKER
SET search_path = pg_catalog
AS $body$
  SELECT p_payload IS NOT NULL
    AND jsonb_typeof(p_payload) = 'object'
    AND octet_length(p_payload::text) BETWEEN 2 AND 16384
    AND NOT (
      p_payload ? 'secret'
      OR p_payload ? 'secret_ref'
      OR p_payload ? 'password'
      OR p_payload ? 'argv'
      OR p_payload ? 'shell'
    )
    AND p_payload::text NOT LIKE '%secret://%'
    AND p_payload::text NOT LIKE '%reader.password%';
$body$;

CREATE OR REPLACE FUNCTION jw.schedule_put(
  p_schedule uuid,
  p_project uuid,
  p_period integer,
  p_payload jsonb
) RETURNS void
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog
AS $body$
DECLARE
  v_enabled boolean;
BEGIN
  IF p_period < 1000 OR p_period > 86400000 OR NOT jw.schedule_payload_closed(p_payload) THEN
    RAISE EXCEPTION 'schedule payload rejected' USING ERRCODE = '22023';
  END IF;
  SELECT enabled INTO v_enabled
  FROM jobs.schedules
  WHERE id = p_schedule AND project_id = p_project;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'schedule missing' USING ERRCODE = '22023';
  END IF;
  INSERT INTO jw.schedule_terms (schedule_id, project_id, period_ms, payload, enabled)
  VALUES (p_schedule, p_project, p_period, p_payload, v_enabled);
END;
$body$;

CREATE OR REPLACE FUNCTION jw.schedule_set_period(
  p_binding uuid,
  p_project uuid,
  p_period integer
) RETURNS integer
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog
AS $body$
DECLARE
  v_prior integer;
  v_schedule uuid;
  v_next timestamptz;
BEGIN
  IF p_period < 1000 OR p_period > 86400000 THEN
    RAISE EXCEPTION 'schedule period rejected' USING ERRCODE = '22023';
  END IF;
  SELECT period_ms, schedule_id INTO v_prior, v_schedule
  FROM jw.capture_bindings
  WHERE id = p_binding AND project_id = p_project
  FOR UPDATE;
  IF NOT FOUND OR v_schedule IS NULL THEN
    RAISE EXCEPTION 'schedule missing' USING ERRCODE = '22023';
  END IF;
  SELECT next_run_at INTO v_next
  FROM jobs.schedules
  WHERE id = v_schedule AND project_id = p_project;
  UPDATE jw.schedule_terms
  SET period_ms = p_period, updated_at = clock_timestamp()
  WHERE schedule_id = v_schedule AND project_id = p_project;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'schedule term missing' USING ERRCODE = '22023';
  END IF;
  UPDATE jw.capture_bindings
  SET period_ms = p_period, updated_at = clock_timestamp()
  WHERE id = p_binding AND project_id = p_project;
  IF (SELECT next_run_at FROM jobs.schedules WHERE id = v_schedule) IS DISTINCT FROM v_next THEN
    RAISE EXCEPTION 'schedule next_run moved' USING ERRCODE = '23514';
  END IF;
  RETURN v_prior;
END;
$body$;

CREATE OR REPLACE FUNCTION jw.schedule_set_enabled(
  p_schedule uuid,
  p_project uuid,
  p_kind text,
  p_enabled boolean
) RETURNS boolean
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog
AS $body$
BEGIN
  UPDATE jobs.schedules
  SET enabled = p_enabled
  WHERE id = p_schedule AND project_id = p_project AND kind = p_kind;
  IF NOT FOUND THEN
    RETURN false;
  END IF;
  UPDATE jw.schedule_terms
  SET enabled = p_enabled, updated_at = clock_timestamp()
  WHERE schedule_id = p_schedule AND project_id = p_project;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'schedule term missing' USING ERRCODE = '22023';
  END IF;
  RETURN true;
END;
$body$;

CREATE OR REPLACE FUNCTION jw.schedule_set_pin(
  p_binding uuid,
  p_project uuid,
  p_pin bigint
) RETURNS void
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog
AS $body$
DECLARE
  v_schedule uuid;
  v_binding text;
  v_payload jsonb;
  v_next timestamptz;
BEGIN
  IF p_pin < 1 THEN
    RAISE EXCEPTION 'schedule payload rejected' USING ERRCODE = '22023';
  END IF;
  SELECT schedule_id, id::text INTO v_schedule, v_binding
  FROM jw.capture_bindings
  WHERE id = p_binding AND project_id = p_project;
  IF NOT FOUND OR v_schedule IS NULL THEN
    RAISE EXCEPTION 'schedule missing' USING ERRCODE = '22023';
  END IF;
  SELECT next_run_at INTO v_next FROM jobs.schedules WHERE id = v_schedule;
  v_payload := jsonb_build_object('v', 1, 'binding', v_binding, 'pin', p_pin);
  IF NOT jw.schedule_payload_closed(v_payload) THEN
    RAISE EXCEPTION 'schedule payload rejected' USING ERRCODE = '22023';
  END IF;
  UPDATE jw.schedule_terms
  SET payload = v_payload, updated_at = clock_timestamp()
  WHERE schedule_id = v_schedule AND project_id = p_project;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'schedule term missing' USING ERRCODE = '22023';
  END IF;
  IF (SELECT next_run_at FROM jobs.schedules WHERE id = v_schedule) IS DISTINCT FROM v_next THEN
    RAISE EXCEPTION 'schedule next_run moved' USING ERRCODE = '23514';
  END IF;
END;
$body$;

CREATE OR REPLACE VIEW jw.schedule_api
WITH (security_invoker = true) AS
SELECT
  s.id,
  s.project_id,
  s.kind,
  s.config_version,
  s.artifact_digest,
  s.dedupe_prefix,
  s.next_run_at,
  s.enabled,
  s.created_at,
  s.updated_at,
  t.period_ms,
  t.payload
FROM jobs.schedules AS s
JOIN jw.schedule_terms AS t
  ON t.schedule_id = s.id AND t.project_id = s.project_id;

COMMENT ON VIEW jw.schedule_api IS
  'Published jobs.schedules row plus the job-worker period and payload. This view does not add a jobs function.';

REVOKE ALL ON jw.schedule_terms FROM PUBLIC;
REVOKE ALL ON FUNCTION jw.schedule_payload_closed(jsonb) FROM PUBLIC;
REVOKE ALL ON FUNCTION jw.schedule_put(uuid, uuid, integer, jsonb) FROM PUBLIC;
REVOKE ALL ON FUNCTION jw.schedule_set_period(uuid, uuid, integer) FROM PUBLIC;
REVOKE ALL ON FUNCTION jw.schedule_set_enabled(uuid, uuid, text, boolean) FROM PUBLIC;
REVOKE ALL ON FUNCTION jw.schedule_set_pin(uuid, uuid, bigint) FROM PUBLIC;
REVOKE ALL ON jw.schedule_api FROM PUBLIC;

GRANT SELECT, INSERT, UPDATE ON jw.schedule_terms TO oo_jobs_runtime;
GRANT EXECUTE ON FUNCTION jw.schedule_payload_closed(jsonb) TO oo_jobs_runtime;
GRANT EXECUTE ON FUNCTION jw.schedule_put(uuid, uuid, integer, jsonb) TO oo_jobs_runtime;
GRANT EXECUTE ON FUNCTION jw.schedule_set_period(uuid, uuid, integer) TO oo_jobs_runtime;
GRANT EXECUTE ON FUNCTION jw.schedule_set_enabled(uuid, uuid, text, boolean) TO oo_jobs_runtime;
GRANT EXECUTE ON FUNCTION jw.schedule_set_pin(uuid, uuid, bigint) TO oo_jobs_runtime;
GRANT SELECT ON jw.schedule_api TO oo_jobs_runtime;

INSERT INTO jw.schedule_terms (schedule_id, project_id, period_ms, payload, enabled)
SELECT b.schedule_id,
       b.project_id,
       b.period_ms,
       jsonb_build_object('v', 1, 'binding', b.id::text, 'pin', b.current_pin),
       s.enabled
FROM jw.capture_bindings AS b
JOIN jobs.schedules AS s ON s.id = b.schedule_id AND s.project_id = b.project_id
WHERE b.schedule_id IS NOT NULL
ON CONFLICT (schedule_id) DO NOTHING;
