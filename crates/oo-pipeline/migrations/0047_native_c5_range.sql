ALTER TABLE _oo.table_quality ADD COLUMN c5_mode text,
  ADD COLUMN last_full_digest_scan_completed_at timestamptz,
  ADD COLUMN recent_window_watermark timestamptz;

CREATE TABLE _oo.c5_table (
  source text NOT NULL, schema_name text NOT NULL, table_name text NOT NULL,
  schema_fingerprint text NOT NULL, topology_revision bigint NOT NULL CHECK(topology_revision>0),
  source_generation bigint NOT NULL CHECK(source_generation>=0), input_digest text,
  recent_window_watermark timestamptz, last_full_digest_scan_completed_at timestamptz,
  PRIMARY KEY(source,schema_name,table_name)
);
CREATE TABLE _oo.c5_scan (
  source text NOT NULL, schema_name text NOT NULL, table_name text NOT NULL,
  scan_id text NOT NULL, status text NOT NULL CHECK(status IN ('running','completed')),
  next_block bigint NOT NULL DEFAULT 0, started_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  completed_at timestamptz, expected_chunks jsonb NOT NULL,
  PRIMARY KEY(source,schema_name,table_name,scan_id),
  FOREIGN KEY(source,schema_name,table_name) REFERENCES _oo.c5_table
);
CREATE UNIQUE INDEX c5_one_running_scan ON _oo.c5_scan(source,schema_name,table_name) WHERE status='running';
CREATE TABLE _oo.c5_chunk (
  source text NOT NULL, schema_name text NOT NULL, table_name text NOT NULL, chunk_id text NOT NULL,
  topology_revision bigint NOT NULL, lo jsonb NOT NULL, hi jsonb NOT NULL,
  membership_revision bigint NOT NULL DEFAULT 0, row_count bigint NOT NULL DEFAULT 0,
  previous_digest_hi bit(64), previous_digest_lo bit(64),
  current_digest_hi bit(64), current_digest_lo bit(64), scanned_at timestamptz,
  arrow_sha256 text, last_scope jsonb, last_scan_id text,
  PRIMARY KEY(source,schema_name,table_name,chunk_id),
  FOREIGN KEY(source,schema_name,table_name) REFERENCES _oo.c5_table
);
CREATE TABLE _oo.c5_member (
  source text NOT NULL, schema_name text NOT NULL, table_name text NOT NULL, chunk_id text NOT NULL,
  key_identity bytea NOT NULL, typed_key jsonb NOT NULL, loader_key text NOT NULL,
  PRIMARY KEY(source,schema_name,table_name,key_identity),
  FOREIGN KEY(source,schema_name,table_name,chunk_id) REFERENCES _oo.c5_chunk
);
CREATE TABLE _oo.c5_block_receipt (
  source text NOT NULL, schema_name text NOT NULL, table_name text NOT NULL, chunk_id text NOT NULL,
  source_generation bigint NOT NULL, scope jsonb NOT NULL, run_id uuid NOT NULL,
  batch_id text NOT NULL, payload_digest text NOT NULL, target_generation bigint NOT NULL,
  sealed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(source,schema_name,table_name,chunk_id,source_generation),
  FOREIGN KEY(source,schema_name,table_name,chunk_id) REFERENCES _oo.c5_chunk,
  FOREIGN KEY(run_id) REFERENCES _oo.full_import_run(run_id)
);
REVOKE ALL ON _oo.c5_table,_oo.c5_scan,_oo.c5_chunk,_oo.c5_member,_oo.c5_block_receipt FROM PUBLIC,oo_pipeline_runtime;
