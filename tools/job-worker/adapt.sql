-- Driver-owned side tables. jobs-lease-operations/1 has no checkpoint
-- column, no effect table, and no payload column on jobs.schedules.
-- This file does not create or replace a jobs.* function.

CREATE SCHEMA IF NOT EXISTS jw AUTHORIZATION oo_jobs_owner;

REVOKE ALL ON SCHEMA jw FROM PUBLIC;
GRANT USAGE ON SCHEMA jw TO oo_jobs_runtime;

CREATE TABLE IF NOT EXISTS jw.install (
  id integer PRIMARY KEY,
  checksum text NOT NULL,
  CONSTRAINT jw_install_singleton CHECK (id = 1),
  CONSTRAINT jw_install_checksum CHECK (checksum ~ '^[0-9a-f]{64}$')
);

CREATE TABLE IF NOT EXISTS jw.checkpoints (
  task_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  generation bigint NOT NULL,
  owner text NOT NULL,
  applied integer NOT NULL,
  target integer NOT NULL,
  updated_at timestamptz NOT NULL,
  CONSTRAINT jw_checkpoints_applied CHECK (applied >= 0 AND applied <= target),
  CONSTRAINT jw_checkpoints_target CHECK (target >= 1 AND target <= 32),
  CONSTRAINT jw_checkpoints_owner CHECK (owner ~ '^[A-Za-z0-9_.:-]{1,128}$')
);

CREATE TABLE IF NOT EXISTS jw.effects (
  task_id uuid NOT NULL,
  seq integer NOT NULL,
  project_id uuid NOT NULL,
  generation bigint NOT NULL,
  owner text NOT NULL,
  txid text NOT NULL,
  applied_at timestamptz NOT NULL,
  PRIMARY KEY (task_id, seq),
  CONSTRAINT jw_effects_seq CHECK (seq >= 1 AND seq <= 32),
  CONSTRAINT jw_effects_txid CHECK (octet_length(txid) BETWEEN 1 AND 64),
  CONSTRAINT jw_effects_owner CHECK (owner ~ '^[A-Za-z0-9_.:-]{1,128}$')
);

CREATE TABLE IF NOT EXISTS jw.claims (
  task_id uuid NOT NULL,
  generation bigint NOT NULL,
  owner text NOT NULL,
  claimed_at timestamptz NOT NULL,
  lease_until timestamptz NOT NULL,
  PRIMARY KEY (task_id, generation),
  CONSTRAINT jw_claims_owner CHECK (owner ~ '^[A-Za-z0-9_.:-]{1,128}$')
);

CREATE TABLE IF NOT EXISTS jw.beats (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  task_id uuid NOT NULL,
  generation bigint NOT NULL,
  owner text NOT NULL,
  observed_at timestamptz NOT NULL,
  lease_until timestamptz NOT NULL,
  CONSTRAINT jw_beats_owner CHECK (owner ~ '^[A-Za-z0-9_.:-]{1,128}$')
);

CREATE TABLE IF NOT EXISTS jw.schedule_payloads (
  schedule_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  payload text NOT NULL,
  payload_version integer NOT NULL,
  config_version text NOT NULL,
  artifact_digest text NOT NULL,
  max_attempts integer NOT NULL,
  CONSTRAINT jw_sched_payload_len CHECK (octet_length(payload) BETWEEN 1 AND 16384),
  CONSTRAINT jw_sched_digest CHECK (artifact_digest ~ '^[0-9a-f]{64}$'),
  CONSTRAINT jw_sched_attempts CHECK (max_attempts BETWEEN 1 AND 32),
  CONSTRAINT jw_sched_version CHECK (payload_version = 1)
);

CREATE TABLE IF NOT EXISTS jw.intents (
  dedupe_key text PRIMARY KEY,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  CONSTRAINT jw_intents_key CHECK (octet_length(dedupe_key) BETWEEN 1 AND 256)
);

CREATE TABLE IF NOT EXISTS jw.observations (
  name text PRIMARY KEY,
  value bigint NOT NULL,
  observed_at timestamptz NOT NULL,
  CONSTRAINT jw_obs_name CHECK (name ~ '^[a-z0-9_-]{1,64}$')
);

REVOKE ALL ON ALL TABLES IN SCHEMA jw FROM PUBLIC;
GRANT SELECT, INSERT, UPDATE ON jw.checkpoints TO oo_jobs_runtime;
GRANT SELECT, INSERT ON jw.effects TO oo_jobs_runtime;
GRANT SELECT, INSERT ON jw.claims TO oo_jobs_runtime;
GRANT SELECT, INSERT ON jw.beats TO oo_jobs_runtime;
GRANT SELECT, INSERT ON jw.schedule_payloads TO oo_jobs_runtime;
GRANT SELECT, INSERT ON jw.intents TO oo_jobs_runtime;
GRANT SELECT, INSERT, UPDATE ON jw.observations TO oo_jobs_runtime;
GRANT SELECT ON jw.install TO oo_jobs_runtime;
GRANT USAGE, SELECT ON SEQUENCE jw.beats_id_seq TO oo_jobs_runtime;

-- Capture window state. jobs.schedules has no period and no payload column.
-- These tables do not add a jobs.* function. Pins are insert-only.
-- schedule_payloads stays insert-only; the next fire reads current_pin.

CREATE TABLE IF NOT EXISTS jw.capture_bindings (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  schedule_id uuid UNIQUE,
  source text NOT NULL,
  driver_sha text NOT NULL,
  artifact_rel text NOT NULL,
  secret_ref text NOT NULL,
  table_name text NOT NULL,
  shard text NOT NULL,
  fence_owner uuid NOT NULL,
  period_ms integer NOT NULL,
  row_budget integer NOT NULL,
  byte_budget integer NOT NULL,
  current_pin bigint NOT NULL,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  CONSTRAINT jw_cap_source CHECK (source IN ('doris', 'clickhouse', 'mongodb', 'starrocks')),
  CONSTRAINT jw_cap_driver CHECK (driver_sha ~ '^[0-9a-f]{40}$'),
  CONSTRAINT jw_cap_artifact CHECK (
    octet_length(artifact_rel) BETWEEN 1 AND 512
    AND artifact_rel !~ '\.\.'
    AND left(artifact_rel, 1) <> '/'
  ),
  CONSTRAINT jw_cap_secret CHECK (
    octet_length(secret_ref) BETWEEN 1 AND 256
    AND secret_ref !~ '\.\.'
  ),
  CONSTRAINT jw_cap_table CHECK (table_name ~ '^[a-z][a-z0-9_]{0,62}$'),
  CONSTRAINT jw_cap_shard CHECK (shard ~ '^[a-z][a-z0-9_.:-]{0,127}$'),
  CONSTRAINT jw_cap_shard_unique UNIQUE (shard),
  CONSTRAINT jw_cap_period CHECK (period_ms BETWEEN 1000 AND 86400000),
  CONSTRAINT jw_cap_rows CHECK (row_budget BETWEEN 1 AND 64),
  CONSTRAINT jw_cap_bytes CHECK (byte_budget BETWEEN 1 AND 2000000),
  CONSTRAINT jw_cap_pin CHECK (current_pin >= 1)
);

CREATE TABLE IF NOT EXISTS jw.capture_pins (
  binding_id uuid NOT NULL REFERENCES jw.capture_bindings (id),
  pin bigint NOT NULL,
  artifact_sha text NOT NULL,
  source_generation bigint NOT NULL,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (binding_id, pin),
  CONSTRAINT jw_pin_sha CHECK (artifact_sha ~ '^[0-9a-f]{64}$'),
  CONSTRAINT jw_pin_pin CHECK (pin >= 1),
  CONSTRAINT jw_pin_gen CHECK (source_generation >= 1)
);

CREATE TABLE IF NOT EXISTS jw.capture_watermarks (
  binding_id uuid PRIMARY KEY REFERENCES jw.capture_bindings (id),
  project_id uuid NOT NULL,
  source_generation bigint NOT NULL,
  artifact_sha text NOT NULL,
  receipt_key text NOT NULL,
  payload_digest text NOT NULL,
  applied_pos numeric NOT NULL,
  copied boolean NOT NULL,
  seal_state text NOT NULL,
  task_id uuid NOT NULL,
  generation bigint NOT NULL,
  advanced_at timestamptz NOT NULL,
  CONSTRAINT jw_wm_sha CHECK (artifact_sha ~ '^[0-9a-f]{64}$'),
  CONSTRAINT jw_wm_digest CHECK (payload_digest ~ '^[0-9a-f]{64}$'),
  CONSTRAINT jw_wm_key CHECK (octet_length(receipt_key) BETWEEN 1 AND 256),
  CONSTRAINT jw_wm_seal CHECK (seal_state IN ('awaiting_publish', 'replay')),
  CONSTRAINT jw_wm_gen CHECK (source_generation >= 1 AND generation >= 1)
);

CREATE TABLE IF NOT EXISTS jw.capture_faults (
  binding_id uuid PRIMARY KEY REFERENCES jw.capture_bindings (id),
  kind text NOT NULL,
  armed boolean NOT NULL,
  CONSTRAINT jw_fault_kind CHECK (kind IN ('apply_once', 'mirror_once'))
);

CREATE TABLE IF NOT EXISTS jw.capture_refusals (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  binding_id uuid NOT NULL REFERENCES jw.capture_bindings (id),
  pin bigint NOT NULL,
  reason text NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  CONSTRAINT jw_refusal_reason CHECK (reason ~ '^[a-z0-9_]{1,64}$'),
  CONSTRAINT jw_refusal_pin CHECK (pin >= 1)
);

REVOKE ALL ON jw.capture_bindings, jw.capture_pins, jw.capture_watermarks, jw.capture_faults, jw.capture_refusals FROM PUBLIC;
GRANT SELECT, INSERT ON jw.capture_bindings TO oo_jobs_runtime;
GRANT UPDATE (schedule_id, period_ms, current_pin, artifact_rel, updated_at) ON jw.capture_bindings TO oo_jobs_runtime;
GRANT SELECT, INSERT ON jw.capture_pins TO oo_jobs_runtime;
GRANT SELECT, INSERT, UPDATE ON jw.capture_watermarks TO oo_jobs_runtime;
GRANT SELECT, INSERT ON jw.capture_faults TO oo_jobs_runtime;
GRANT UPDATE (kind, armed) ON jw.capture_faults TO oo_jobs_runtime;
GRANT SELECT, INSERT ON jw.capture_refusals TO oo_jobs_runtime;
GRANT USAGE, SELECT ON SEQUENCE jw.capture_refusals_id_seq TO oo_jobs_runtime;
