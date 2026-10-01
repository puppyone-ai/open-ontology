-- jobs-control/1 phase expand
-- Run as oo_jobs_owner after jobs-bootstrap/1. This file does not create roles,
-- install extensions, or define a claim/heartbeat driver. Lease duration,
-- worker identity, NOTIFY, and target-database fencing are not enforced here.

CREATE SCHEMA jobs AUTHORIZATION oo_jobs_owner;

REVOKE ALL ON SCHEMA jobs FROM PUBLIC;
GRANT USAGE ON SCHEMA jobs TO oo_jobs_runtime;

-- Tables and sequences have no hard-wired PUBLIC grant. The per-schema
-- revoke is the recorded default for objects this role creates in jobs.
-- PUBLIC EXECUTE on functions is hard-wired with no schema. A per-schema
-- entry can only add grants, so that revoke has to be global for this
-- role. Runtime EXECUTE is added back only inside jobs.
ALTER DEFAULT PRIVILEGES FOR ROLE oo_jobs_owner IN SCHEMA jobs
  REVOKE ALL ON TABLES FROM PUBLIC;
ALTER DEFAULT PRIVILEGES FOR ROLE oo_jobs_owner IN SCHEMA jobs
  REVOKE ALL ON SEQUENCES FROM PUBLIC;
ALTER DEFAULT PRIVILEGES FOR ROLE oo_jobs_owner
  REVOKE ALL ON FUNCTIONS FROM PUBLIC;
ALTER DEFAULT PRIVILEGES FOR ROLE oo_jobs_owner
  REVOKE ALL ON ROUTINES FROM PUBLIC;
ALTER DEFAULT PRIVILEGES FOR ROLE oo_jobs_owner IN SCHEMA jobs
  GRANT EXECUTE ON FUNCTIONS TO oo_jobs_runtime;
ALTER DEFAULT PRIVILEGES FOR ROLE oo_jobs_owner IN SCHEMA jobs
  GRANT EXECUTE ON ROUTINES TO oo_jobs_runtime;

CREATE TABLE jobs.schema_migrations (
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

CREATE TABLE jobs.migration_lock (
  id integer PRIMARY KEY,
  CONSTRAINT migration_lock_singleton_check CHECK (id = 1)
);

INSERT INTO jobs.migration_lock (id) VALUES (1);

COMMENT ON TABLE jobs.migration_lock IS
  'Single-executor migration lock row. Applicators take pg_advisory_xact_lock(6401202601) and then lock this row FOR UPDATE. The advisory key is not an Agent DB fence.';

CREATE TABLE jobs.tasks (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id uuid NOT NULL,
  kind text NOT NULL,
  payload_version integer NOT NULL,
  payload jsonb NOT NULL,
  config_version text NOT NULL,
  artifact_digest text NOT NULL,
  dedupe_key text NOT NULL,
  state text NOT NULL DEFAULT 'queued',
  run_at timestamptz NOT NULL,
  attempt integer NOT NULL DEFAULT 0,
  max_attempts integer NOT NULL,
  lease_owner text,
  lease_until timestamptz,
  generation bigint NOT NULL DEFAULT 0,
  last_error text,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  CONSTRAINT tasks_kind_check CHECK (kind ~ '^[a-z][a-z0-9_]{0,63}$'),
  CONSTRAINT tasks_payload_version_check CHECK (payload_version >= 1 AND payload_version <= 1000000),
  CONSTRAINT tasks_payload_bound_check CHECK (octet_length(payload::text) <= 16384),
  CONSTRAINT tasks_config_version_check CHECK (octet_length(config_version) BETWEEN 1 AND 128),
  CONSTRAINT tasks_artifact_digest_check CHECK (artifact_digest ~ '^[0-9a-f]{64}$'),
  CONSTRAINT tasks_dedupe_key_check CHECK (octet_length(dedupe_key) BETWEEN 1 AND 256),
  CONSTRAINT tasks_state_check CHECK (state IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
  CONSTRAINT tasks_attempt_bound_check CHECK (attempt >= 0 AND attempt <= max_attempts),
  CONSTRAINT tasks_max_attempts_check CHECK (max_attempts >= 1 AND max_attempts <= 32),
  CONSTRAINT tasks_generation_check CHECK (generation >= 0),
  CONSTRAINT tasks_lease_owner_check CHECK (
    lease_owner IS NULL
    OR (octet_length(lease_owner) BETWEEN 1 AND 128 AND lease_owner ~ '^[A-Za-z0-9_.:-]{1,128}$')
  ),
  CONSTRAINT tasks_last_error_bound_check CHECK (
    last_error IS NULL OR octet_length(last_error) <= 2048
  ),
  CONSTRAINT tasks_state_lease_check CHECK (
    (
      state = 'queued'
      AND lease_owner IS NULL
      AND lease_until IS NULL
    )
    OR (
      state = 'running'
      AND lease_owner IS NOT NULL
      AND lease_until IS NOT NULL
      AND generation >= 1
      AND attempt >= 1
    )
    OR (
      state = 'succeeded'
      AND lease_owner IS NULL
      AND lease_until IS NULL
      AND last_error IS NULL
      AND attempt >= 1
      AND generation >= 1
    )
    OR (
      state = 'failed'
      AND lease_owner IS NULL
      AND lease_until IS NULL
      AND last_error IS NOT NULL
      AND attempt >= 1
      AND generation >= 1
    )
    OR (
      state = 'cancelled'
      AND lease_owner IS NULL
      AND lease_until IS NULL
      AND last_error IS NULL
    )
  ),
  CONSTRAINT tasks_project_dedupe_unique UNIQUE (project_id, dedupe_key)
);

COMMENT ON TABLE jobs.tasks IS
  'Durable control-plane task. payload, payload_version, config_version, and artifact_digest stay on the row for every retry. A second enqueue with the same project and dedupe key conflicts; it does not overwrite.';

CREATE INDEX tasks_claim_idx ON jobs.tasks (run_at, id) WHERE state = 'queued';

CREATE TABLE jobs.schedules (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id uuid NOT NULL,
  kind text NOT NULL,
  config_version text NOT NULL,
  artifact_digest text NOT NULL,
  dedupe_prefix text NOT NULL,
  next_run_at timestamptz NOT NULL,
  enabled boolean NOT NULL DEFAULT true,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  CONSTRAINT schedules_kind_check CHECK (kind ~ '^[a-z][a-z0-9_]{0,63}$'),
  CONSTRAINT schedules_config_version_check CHECK (octet_length(config_version) BETWEEN 1 AND 128),
  CONSTRAINT schedules_artifact_digest_check CHECK (artifact_digest ~ '^[0-9a-f]{64}$'),
  CONSTRAINT schedules_dedupe_prefix_check CHECK (octet_length(dedupe_prefix) BETWEEN 1 AND 200),
  CONSTRAINT schedules_project_prefix_unique UNIQUE (project_id, dedupe_prefix)
);

CREATE INDEX schedules_due_idx ON jobs.schedules (next_run_at, id) WHERE enabled;

CREATE TABLE jobs.schedule_occurrences (
  schedule_id uuid NOT NULL REFERENCES jobs.schedules (id) ON DELETE RESTRICT,
  scheduled_at timestamptz NOT NULL,
  project_id uuid NOT NULL,
  task_id uuid NOT NULL UNIQUE REFERENCES jobs.tasks (id) ON DELETE RESTRICT,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (schedule_id, scheduled_at)
);

COMMENT ON TABLE jobs.schedule_occurrences IS
  '(schedule_id, scheduled_at) is the occurrence key. A timer restart inserts the same pair again and conflicts instead of enqueueing a second side effect.';

CREATE TABLE jobs.leases (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id uuid NOT NULL,
  target_key text NOT NULL,
  owner text,
  lease_until timestamptz,
  generation bigint NOT NULL DEFAULT 0,
  holder_task_id uuid REFERENCES jobs.tasks (id) ON DELETE RESTRICT,
  exclusivity_domain text NOT NULL DEFAULT 'control_jobs',
  target_fence_certified boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  CONSTRAINT leases_target_key_check CHECK (octet_length(target_key) BETWEEN 1 AND 256),
  CONSTRAINT leases_domain_check CHECK (exclusivity_domain = 'control_jobs'),
  CONSTRAINT leases_fence_check CHECK (target_fence_certified = false),
  CONSTRAINT leases_generation_check CHECK (generation >= 0),
  CONSTRAINT leases_owner_check CHECK (
    owner IS NULL
    OR (octet_length(owner) BETWEEN 1 AND 128 AND owner ~ '^[A-Za-z0-9_.:-]{1,128}$')
  ),
  CONSTRAINT leases_coherence_check CHECK (
    (
      owner IS NULL
      AND lease_until IS NULL
      AND holder_task_id IS NULL
    )
    OR (
      owner IS NOT NULL
      AND lease_until IS NOT NULL
      AND generation >= 1
    )
  ),
  CONSTRAINT leases_project_target_unique UNIQUE (project_id, target_key)
);

COMMENT ON TABLE jobs.leases IS
  'Control-plane mutex for a jobs target key. An active row is not an Agent DB advisory lock and does not certify target-side writer exclusivity.';

COMMENT ON COLUMN jobs.leases.target_fence_certified IS
  'Constrained false. This schema cannot certify target-database fencing.';

CREATE TABLE jobs.task_events (
  event_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  project_id uuid NOT NULL,
  task_id uuid NOT NULL REFERENCES jobs.tasks (id) ON DELETE RESTRICT,
  from_state text,
  to_state text NOT NULL,
  generation bigint NOT NULL,
  attempt integer NOT NULL,
  lease_owner text,
  occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  note text,
  CONSTRAINT task_events_state_check CHECK (
    (from_state IS NULL OR from_state IN ('queued', 'running', 'succeeded', 'failed', 'cancelled'))
    AND to_state IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')
  ),
  CONSTRAINT task_events_generation_check CHECK (generation >= 0),
  CONSTRAINT task_events_attempt_check CHECK (attempt >= 0 AND attempt <= 32),
  CONSTRAINT task_events_note_bound_check CHECK (note IS NULL OR octet_length(note) <= 512),
  CONSTRAINT task_events_lease_owner_check CHECK (
    lease_owner IS NULL
    OR (octet_length(lease_owner) BETWEEN 1 AND 128 AND lease_owner ~ '^[A-Za-z0-9_.:-]{1,128}$')
  )
);

CREATE INDEX task_events_task_idx ON jobs.task_events (task_id, event_id);

CREATE FUNCTION jobs.enforce_task_update()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog, jobs
AS $$
BEGIN
  IF NEW.id IS DISTINCT FROM OLD.id
     OR NEW.project_id IS DISTINCT FROM OLD.project_id
     OR NEW.kind IS DISTINCT FROM OLD.kind
     OR NEW.payload_version IS DISTINCT FROM OLD.payload_version
     OR NEW.payload IS DISTINCT FROM OLD.payload
     OR NEW.config_version IS DISTINCT FROM OLD.config_version
     OR NEW.artifact_digest IS DISTINCT FROM OLD.artifact_digest
     OR NEW.dedupe_key IS DISTINCT FROM OLD.dedupe_key
     OR NEW.max_attempts IS DISTINCT FROM OLD.max_attempts
     OR NEW.created_at IS DISTINCT FROM OLD.created_at
  THEN
    RAISE EXCEPTION 'immutable task columns' USING ERRCODE = '23514';
  END IF;

  IF NEW.generation < OLD.generation THEN
    RAISE EXCEPTION 'generation decreased' USING ERRCODE = '23514';
  END IF;

  IF NEW.attempt < OLD.attempt THEN
    RAISE EXCEPTION 'attempt decreased' USING ERRCODE = '23514';
  END IF;

  IF NEW.lease_owner IS DISTINCT FROM OLD.lease_owner
     AND NEW.lease_owner IS NOT NULL
     AND NEW.generation <= OLD.generation THEN
    RAISE EXCEPTION 'lease owner change requires a new generation' USING ERRCODE = '23514';
  END IF;

  IF OLD.state IN ('succeeded', 'failed', 'cancelled')
     AND NEW.state IS DISTINCT FROM OLD.state THEN
    RAISE EXCEPTION 'terminal task state is final' USING ERRCODE = '23514';
  END IF;

  IF OLD.state = 'queued' AND NEW.state = 'running' THEN
    IF NEW.generation <= OLD.generation OR NEW.attempt <> OLD.attempt + 1 THEN
      RAISE EXCEPTION 'claim requires a new generation and the next attempt' USING ERRCODE = '23514';
    END IF;
  ELSIF OLD.state = 'queued' AND NEW.state = 'queued' THEN
    IF NEW.attempt <> OLD.attempt THEN
      RAISE EXCEPTION 'queued update keeps the attempt' USING ERRCODE = '23514';
    END IF;
  ELSIF OLD.state = 'queued' AND NEW.state = 'cancelled' THEN
    IF NEW.attempt <> OLD.attempt OR NEW.last_error IS NOT NULL THEN
      RAISE EXCEPTION 'cancel from queued keeps the attempt' USING ERRCODE = '23514';
    END IF;
  ELSIF OLD.state = 'queued' AND NEW.state IN ('succeeded', 'failed') THEN
    RAISE EXCEPTION 'queued task cannot complete without running' USING ERRCODE = '23514';
  ELSIF OLD.state = 'running' AND NEW.state = 'running' THEN
    IF NEW.lease_owner IS NOT DISTINCT FROM OLD.lease_owner
       AND NEW.generation IS NOT DISTINCT FROM OLD.generation THEN
      IF NEW.attempt <> OLD.attempt THEN
        RAISE EXCEPTION 'heartbeat keeps the attempt' USING ERRCODE = '23514';
      END IF;
    ELSIF NEW.lease_owner IS DISTINCT FROM OLD.lease_owner THEN
      IF NEW.generation <= OLD.generation OR NEW.attempt <> OLD.attempt THEN
        RAISE EXCEPTION 'reclaim keeps the attempt and requires a new generation' USING ERRCODE = '23514';
      END IF;
    ELSE
      RAISE EXCEPTION 'generation change while running keeps the owner' USING ERRCODE = '23514';
    END IF;
  ELSIF OLD.state = 'running' AND NEW.state = 'queued' THEN
    IF OLD.attempt >= OLD.max_attempts THEN
      RAISE EXCEPTION 'attempts are exhausted' USING ERRCODE = '23514';
    END IF;
    IF NEW.attempt <> OLD.attempt OR NEW.generation < OLD.generation THEN
      RAISE EXCEPTION 'retry keeps the attempt' USING ERRCODE = '23514';
    END IF;
  ELSIF OLD.state = 'running' AND NEW.state IN ('succeeded', 'failed', 'cancelled') THEN
    IF NEW.attempt <> OLD.attempt OR NEW.generation < OLD.generation THEN
      RAISE EXCEPTION 'completion keeps the attempt' USING ERRCODE = '23514';
    END IF;
  ELSIF NEW.state IS DISTINCT FROM OLD.state THEN
    RAISE EXCEPTION 'task transition is not legal' USING ERRCODE = '23514';
  END IF;

  NEW.updated_at := clock_timestamp();
  RETURN NEW;
END;
$$;

CREATE TRIGGER tasks_enforce_update
  BEFORE UPDATE ON jobs.tasks
  FOR EACH ROW
  EXECUTE FUNCTION jobs.enforce_task_update();

CREATE FUNCTION jobs.audit_task_change()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog, jobs
AS $$
BEGIN
  IF TG_OP = 'INSERT'
     OR NEW.state IS DISTINCT FROM OLD.state
     OR NEW.generation IS DISTINCT FROM OLD.generation
     OR NEW.lease_owner IS DISTINCT FROM OLD.lease_owner
     OR NEW.attempt IS DISTINCT FROM OLD.attempt
     OR NEW.last_error IS DISTINCT FROM OLD.last_error
  THEN
    INSERT INTO jobs.task_events (
      project_id, task_id, from_state, to_state, generation, attempt, lease_owner, occurred_at
    ) VALUES (
      NEW.project_id,
      NEW.id,
      CASE WHEN TG_OP = 'INSERT' THEN NULL ELSE OLD.state END,
      NEW.state,
      NEW.generation,
      NEW.attempt,
      NEW.lease_owner,
      clock_timestamp()
    );
  END IF;
  RETURN NULL;
END;
$$;

CREATE TRIGGER tasks_audit_insert
  AFTER INSERT ON jobs.tasks
  FOR EACH ROW
  EXECUTE FUNCTION jobs.audit_task_change();

CREATE TRIGGER tasks_audit_update
  AFTER UPDATE ON jobs.tasks
  FOR EACH ROW
  EXECUTE FUNCTION jobs.audit_task_change();

CREATE FUNCTION jobs.enforce_schedule_update()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog, jobs
AS $$
BEGIN
  IF NEW.id IS DISTINCT FROM OLD.id
     OR NEW.project_id IS DISTINCT FROM OLD.project_id
     OR NEW.kind IS DISTINCT FROM OLD.kind
     OR NEW.config_version IS DISTINCT FROM OLD.config_version
     OR NEW.artifact_digest IS DISTINCT FROM OLD.artifact_digest
     OR NEW.dedupe_prefix IS DISTINCT FROM OLD.dedupe_prefix
     OR NEW.created_at IS DISTINCT FROM OLD.created_at
  THEN
    RAISE EXCEPTION 'immutable schedule columns' USING ERRCODE = '23514';
  END IF;
  NEW.updated_at := clock_timestamp();
  RETURN NEW;
END;
$$;

CREATE TRIGGER schedules_enforce_update
  BEFORE UPDATE ON jobs.schedules
  FOR EACH ROW
  EXECUTE FUNCTION jobs.enforce_schedule_update();

CREATE FUNCTION jobs.enforce_occurrence_insert()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog, jobs
AS $$
BEGIN
  IF NOT EXISTS (
    SELECT 1
    FROM jobs.schedules
    WHERE id = NEW.schedule_id
      AND project_id = NEW.project_id
  ) THEN
    RAISE EXCEPTION 'occurrence project does not match the schedule' USING ERRCODE = '23514';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM jobs.tasks
    WHERE id = NEW.task_id
      AND project_id = NEW.project_id
  ) THEN
    RAISE EXCEPTION 'occurrence project does not match the task' USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

CREATE TRIGGER schedule_occurrences_enforce_insert
  BEFORE INSERT ON jobs.schedule_occurrences
  FOR EACH ROW
  EXECUTE FUNCTION jobs.enforce_occurrence_insert();

CREATE FUNCTION jobs.reject_occurrence_mutation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog, jobs
AS $$
BEGIN
  RAISE EXCEPTION 'schedule occurrence is immutable' USING ERRCODE = '23514';
END;
$$;

CREATE TRIGGER schedule_occurrences_reject_update
  BEFORE UPDATE ON jobs.schedule_occurrences
  FOR EACH ROW
  EXECUTE FUNCTION jobs.reject_occurrence_mutation();

CREATE TRIGGER schedule_occurrences_reject_delete
  BEFORE DELETE ON jobs.schedule_occurrences
  FOR EACH ROW
  EXECUTE FUNCTION jobs.reject_occurrence_mutation();

CREATE FUNCTION jobs.enforce_lease_update()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog, jobs
AS $$
BEGIN
  IF NEW.id IS DISTINCT FROM OLD.id
     OR NEW.project_id IS DISTINCT FROM OLD.project_id
     OR NEW.target_key IS DISTINCT FROM OLD.target_key
     OR NEW.exclusivity_domain IS DISTINCT FROM OLD.exclusivity_domain
     OR NEW.target_fence_certified IS DISTINCT FROM OLD.target_fence_certified
     OR NEW.created_at IS DISTINCT FROM OLD.created_at
  THEN
    RAISE EXCEPTION 'immutable lease identity' USING ERRCODE = '23514';
  END IF;
  IF NEW.generation < OLD.generation THEN
    RAISE EXCEPTION 'lease generation decreased' USING ERRCODE = '23514';
  END IF;
  IF NEW.owner IS DISTINCT FROM OLD.owner
     AND NEW.owner IS NOT NULL
     AND NEW.generation <= OLD.generation THEN
    RAISE EXCEPTION 'lease owner change requires a new generation' USING ERRCODE = '23514';
  END IF;
  IF NEW.holder_task_id IS NOT NULL AND NOT EXISTS (
    SELECT 1
    FROM jobs.tasks
    WHERE id = NEW.holder_task_id
      AND project_id = NEW.project_id
  ) THEN
    RAISE EXCEPTION 'lease holder is not a task in this project' USING ERRCODE = '23514';
  END IF;
  NEW.updated_at := clock_timestamp();
  RETURN NEW;
END;
$$;

CREATE TRIGGER leases_enforce_update
  BEFORE UPDATE ON jobs.leases
  FOR EACH ROW
  EXECUTE FUNCTION jobs.enforce_lease_update();

CREATE FUNCTION jobs.reject_book_mutation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog, jobs
AS $$
BEGIN
  RAISE EXCEPTION 'migration bookkeeping is append-only' USING ERRCODE = '23514';
END;
$$;

CREATE TRIGGER schema_migrations_reject_update
  BEFORE UPDATE ON jobs.schema_migrations
  FOR EACH ROW
  EXECUTE FUNCTION jobs.reject_book_mutation();

CREATE TRIGGER schema_migrations_reject_delete
  BEFORE DELETE ON jobs.schema_migrations
  FOR EACH ROW
  EXECUTE FUNCTION jobs.reject_book_mutation();

CREATE FUNCTION jobs.reject_event_mutation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog, jobs
AS $$
BEGIN
  RAISE EXCEPTION 'task event is immutable' USING ERRCODE = '23514';
END;
$$;

CREATE TRIGGER task_events_reject_update
  BEFORE UPDATE ON jobs.task_events
  FOR EACH ROW
  EXECUTE FUNCTION jobs.reject_event_mutation();

CREATE TRIGGER task_events_reject_delete
  BEFORE DELETE ON jobs.task_events
  FOR EACH ROW
  EXECUTE FUNCTION jobs.reject_event_mutation();

CREATE VIEW jobs.dead_letters
WITH (security_invoker = true) AS
SELECT
  id,
  project_id,
  kind,
  payload_version,
  config_version,
  artifact_digest,
  dedupe_key,
  state,
  attempt,
  max_attempts,
  generation,
  last_error,
  created_at,
  updated_at
FROM jobs.tasks
WHERE state = 'failed';

COMMENT ON VIEW jobs.dead_letters IS
  'Terminal failed tasks. Failed is not an automatic retry. security_invoker is on, so the view does not bypass table grants.';

CREATE VIEW jobs.task_audit
WITH (security_invoker = true) AS
SELECT
  event_id,
  project_id,
  task_id,
  from_state,
  to_state,
  generation,
  attempt,
  lease_owner,
  occurred_at,
  note
FROM jobs.task_events;

COMMENT ON VIEW jobs.task_audit IS
  'Append-only task transition log. The runtime role can insert an extra event; it cannot update or delete one. Automatic insert on task change does not authenticate the worker.';

CREATE VIEW jobs.service_readiness
WITH (security_invoker = true) AS
SELECT
  version,
  checksum,
  phase,
  filename,
  applied_at,
  (version = 'jobs-control/1' AND phase = 'expand') AS schema_version_present
FROM jobs.schema_migrations
WHERE version = 'jobs-control/1';

COMMENT ON VIEW jobs.service_readiness IS
  'Version marker for backup and ready checks. schema_version_present does not by itself prove the migration file bytes match checksum. A dependent service stays not ready until that comparison succeeds.';

REVOKE ALL ON ALL TABLES IN SCHEMA jobs FROM PUBLIC;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA jobs FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA jobs FROM PUBLIC;

GRANT SELECT ON jobs.tasks TO oo_jobs_runtime;
GRANT INSERT (
  id, project_id, kind, payload_version, payload, config_version, artifact_digest, dedupe_key, max_attempts, run_at
) ON jobs.tasks TO oo_jobs_runtime;
GRANT UPDATE (
  state, run_at, attempt, lease_owner, lease_until, generation, last_error, updated_at
) ON jobs.tasks TO oo_jobs_runtime;

GRANT SELECT ON jobs.schedules TO oo_jobs_runtime;
GRANT INSERT (
  id, project_id, kind, config_version, artifact_digest, dedupe_prefix, next_run_at
) ON jobs.schedules TO oo_jobs_runtime;
GRANT UPDATE (next_run_at, enabled, updated_at) ON jobs.schedules TO oo_jobs_runtime;

GRANT SELECT ON jobs.schedule_occurrences TO oo_jobs_runtime;
GRANT INSERT (schedule_id, scheduled_at, project_id, task_id) ON jobs.schedule_occurrences TO oo_jobs_runtime;

GRANT SELECT ON jobs.leases TO oo_jobs_runtime;
GRANT INSERT (id, project_id, target_key) ON jobs.leases TO oo_jobs_runtime;
GRANT UPDATE (
  owner, lease_until, generation, holder_task_id, updated_at
) ON jobs.leases TO oo_jobs_runtime;

GRANT SELECT, INSERT ON jobs.task_events TO oo_jobs_runtime;
GRANT USAGE, SELECT ON SEQUENCE jobs.task_events_event_id_seq TO oo_jobs_runtime;

GRANT SELECT ON jobs.schema_migrations TO oo_jobs_runtime;
GRANT SELECT ON jobs.dead_letters TO oo_jobs_runtime;
GRANT SELECT ON jobs.task_audit TO oo_jobs_runtime;
GRANT SELECT ON jobs.service_readiness TO oo_jobs_runtime;

GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA jobs TO oo_jobs_runtime;
