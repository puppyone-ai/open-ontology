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
