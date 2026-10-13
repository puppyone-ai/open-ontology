CREATE SCHEMA IF NOT EXISTS jw_native;
CREATE TABLE IF NOT EXISTS jw_native.bindings (
 id uuid PRIMARY KEY, project_id uuid NOT NULL, source_id text NOT NULL,
 method text NOT NULL CHECK(method IN ('C1','C2','C3','C4','C5')),
 cycle text NOT NULL CHECK(cycle IN ('detect','pull','reconcile')),
 period_ms bigint NOT NULL CHECK(period_ms BETWEEN 1000 AND 86400000),
 enabled boolean NOT NULL DEFAULT true, manifest_sha text NOT NULL CHECK(manifest_sha ~ '^[0-9a-f]{64}$'),
 next_run_at timestamptz NOT NULL DEFAULT clock_timestamp(), pin bigint NOT NULL DEFAULT 0,
 manifest jsonb NOT NULL, target_database text);
ALTER TABLE jw_native.bindings ADD COLUMN IF NOT EXISTS target_database text;
ALTER TABLE jw_native.bindings DROP CONSTRAINT IF EXISTS bindings_project_id_source_id_cycle_key;
CREATE TABLE IF NOT EXISTS jw_native.occurrences (
 binding_id uuid NOT NULL REFERENCES jw_native.bindings(id), pin bigint NOT NULL,
 task_id uuid NOT NULL UNIQUE REFERENCES jobs.tasks(id), manifest_sha text NOT NULL,
 scheduled_at timestamptz NOT NULL, manifest jsonb NOT NULL, producer_input jsonb, request jsonb, result jsonb,
 PRIMARY KEY(binding_id,pin));
ALTER TABLE jw_native.occurrences ADD COLUMN IF NOT EXISTS producer_input jsonb;
CREATE TABLE IF NOT EXISTS jw_native.watermarks (
 binding_id uuid PRIMARY KEY REFERENCES jw_native.bindings(id), pin bigint NOT NULL,
 task_id uuid NOT NULL REFERENCES jobs.tasks(id), generation bigint NOT NULL,
 source_generation bigint NOT NULL, applied_pos text NOT NULL,
 load_state text NOT NULL, result jsonb NOT NULL, advanced_at timestamptz NOT NULL DEFAULT clock_timestamp());
CREATE TABLE IF NOT EXISTS jw_native.source_progress (
 source_id text NOT NULL, target_database text NOT NULL,
 source_generation bigint CHECK(source_generation >= 1),
 task_id uuid REFERENCES jobs.tasks(id), binding_id uuid REFERENCES jw_native.bindings(id),
 active_task_id uuid REFERENCES jobs.tasks(id),
 confirmed_at timestamptz,
 CONSTRAINT source_progress_confirmed CHECK ((source_generation IS NULL) = (task_id IS NULL)
   AND (source_generation IS NULL) = (binding_id IS NULL)),
 PRIMARY KEY(source_id,target_database));
ALTER TABLE jw_native.source_progress ADD COLUMN IF NOT EXISTS active_task_id uuid REFERENCES jobs.tasks(id);
GRANT USAGE ON SCHEMA jw_native TO oo_jobs_runtime;
GRANT SELECT,INSERT,UPDATE ON ALL TABLES IN SCHEMA jw_native TO oo_jobs_runtime;
CREATE TABLE IF NOT EXISTS jw_native.process_attempts (
 task_id uuid NOT NULL REFERENCES jobs.tasks(id), generation bigint NOT NULL,
 stage text NOT NULL CHECK(stage IN ('producer','loader','ack')),
 executable_sha256 text NOT NULL, exit_code integer NOT NULL,
 stdout bytea NOT NULL, stderr bytea NOT NULL,
 observed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(task_id,generation,stage));
REVOKE ALL ON jw_native.process_attempts FROM PUBLIC;
GRANT SELECT,INSERT ON jw_native.process_attempts TO oo_jobs_runtime;
REVOKE UPDATE ON jw_native.process_attempts FROM oo_jobs_runtime;
GRANT UPDATE(exit_code,stdout,stderr,observed_at) ON jw_native.process_attempts TO oo_jobs_runtime;
ALTER TABLE jw_native.process_attempts DROP CONSTRAINT IF EXISTS process_attempts_stage_check;
ALTER TABLE jw_native.process_attempts ADD CONSTRAINT process_attempts_stage_check CHECK(stage IN ('producer','loader','ack'));
CREATE TABLE IF NOT EXISTS jw_native.pending_acks (
 task_id uuid PRIMARY KEY REFERENCES jobs.tasks(id), binding_id uuid NOT NULL REFERENCES jw_native.bindings(id),
 pin bigint NOT NULL, generation bigint NOT NULL, ack_request jsonb NOT NULL,
 loader_path text NOT NULL, loader_sha256 text NOT NULL, acknowledged_at timestamptz);
REVOKE ALL ON jw_native.pending_acks FROM PUBLIC;
GRANT SELECT,INSERT,UPDATE ON jw_native.pending_acks TO oo_jobs_runtime;
CREATE TABLE IF NOT EXISTS jw_native.loader_overrides (
 binding_id uuid PRIMARY KEY REFERENCES jw_native.bindings(id),
 operator_manifest jsonb NOT NULL, operator_manifest_sha text NOT NULL,
 approved_by text NOT NULL DEFAULT current_user,
 approved_at timestamptz NOT NULL DEFAULT clock_timestamp());
REVOKE ALL ON jw_native.loader_overrides FROM PUBLIC;
GRANT SELECT,INSERT,UPDATE ON jw_native.loader_overrides TO oo_jobs_runtime;
