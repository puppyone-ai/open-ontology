//! Statements the driver sends. Task state changes go through jobs.enqueue,
//! jobs.claim, jobs.heartbeat, jobs.complete_success, jobs.complete_failure,
//! jobs.retry_failure, and jobs.cancel. There is no UPDATE of jobs.tasks.

pub const ENQUEUE: &str = "\
SELECT status, task_id::text, project_id::text, kind, state, attempt, generation, lease_owner, run_at::text
FROM jobs.enqueue($1::text::uuid, $2, $3, $4, $5, $6, $7, clock_timestamp() + make_interval(secs => $8::float8), $9)";

pub const CLAIM: &str = "\
SELECT status, task_id::text, project_id::text, kind, state, attempt, generation, lease_owner, run_at::text
FROM jobs.claim($1::text::uuid, $2::text[], $3)";

pub const CLAIM_SELECTED: &str = "\
SELECT status, task_id::text, project_id::text, kind, state, attempt, generation, lease_owner, run_at::text
FROM jobs.claim_selected($1::text::uuid, $2::text::uuid, $3::text[], $4)";

pub const HEARTBEAT: &str = "\
SELECT status, task_id::text, project_id::text, kind, state, attempt, generation, lease_owner, run_at::text
FROM jobs.heartbeat($1::text::uuid, $2::text::uuid, $3, $4)";

pub const COMPLETE_SUCCESS: &str = "\
SELECT status, task_id::text, project_id::text, kind, state, attempt, generation, lease_owner, run_at::text
FROM jobs.complete_success($1::text::uuid, $2::text::uuid, $3, $4)";

pub const COMPLETE_FAILURE: &str = "\
SELECT status, task_id::text, project_id::text, kind, state, attempt, generation, lease_owner, run_at::text
FROM jobs.complete_failure($1::text::uuid, $2::text::uuid, $3, $4, $5)";

pub const RETRY_FAILURE: &str = "\
SELECT status, task_id::text, project_id::text, kind, state, attempt, generation, lease_owner, run_at::text
FROM jobs.retry_failure($1::text::uuid, $2::text::uuid, $3, $4, $5)";

pub const CANCEL: &str = "\
SELECT status, task_id::text, project_id::text, kind, state, attempt, generation, lease_owner, run_at::text
FROM jobs.cancel($1::text::uuid, $2::text::uuid, $3, $4)";

pub const NOTIFY: &str = "SELECT pg_notify('oo_jw_wake', $1)";

pub const INSERT_CLAIM: &str = "\
INSERT INTO jw.claims (task_id, generation, owner, claimed_at, lease_until)
SELECT id, generation, lease_owner, clock_timestamp(), lease_until
FROM jobs.tasks
WHERE id = $1::text::uuid
  AND project_id = $2::text::uuid
  AND state = 'running'
  AND lease_owner = $3
  AND generation = $4
RETURNING task_id::text";

pub const HEARTBEAT_DUE: &str = "\
SELECT COALESCE(clock_timestamp() >= COALESCE(
  (SELECT max(observed_at) FROM jw.beats WHERE task_id = $1::text::uuid AND generation = $2),
  (SELECT claimed_at FROM jw.claims WHERE task_id = $1::text::uuid AND generation = $2)
) + make_interval(secs => $3::float8), false)";

pub const INSERT_BEAT: &str = "\
INSERT INTO jw.beats (task_id, generation, owner, observed_at, lease_until)
SELECT id, generation, lease_owner, clock_timestamp(), lease_until
FROM jobs.tasks
WHERE id = $1::text::uuid
  AND project_id = $2::text::uuid
  AND state = 'running'
  AND lease_owner = $3
  AND generation = $4
RETURNING id";

pub const READ_WORK: &str = "\
SELECT payload::text, attempt, max_attempts
FROM jobs.tasks
WHERE id = $1::text::uuid
  AND project_id = $2::text::uuid
  AND state = 'running'
  AND lease_owner = $3
  AND generation = $4
  AND lease_until > clock_timestamp()";

pub const READ_APPLIED: &str = "\
SELECT COALESCE((SELECT applied FROM jw.checkpoints WHERE task_id = $1::text::uuid AND project_id = $2::text::uuid), 0)";

pub const APPLY_EFFECT: &str = "\
WITH gate AS (
  SELECT id
  FROM jobs.tasks
  WHERE id = $1::text::uuid
    AND project_id = $2::text::uuid
    AND state = 'running'
    AND lease_owner = $3
    AND generation = $4
    AND lease_until > clock_timestamp()
),
inserted AS (
  INSERT INTO jw.effects (task_id, seq, project_id, generation, owner, txid, applied_at)
  SELECT $1::text::uuid, $5::integer, $2::text::uuid, $4::bigint, $3, pg_current_xact_id()::text, clock_timestamp()
  WHERE EXISTS (SELECT 1 FROM gate)
  ON CONFLICT (task_id, seq) DO NOTHING
  RETURNING seq
),
upsert AS (
  INSERT INTO jw.checkpoints (task_id, project_id, generation, owner, applied, target, updated_at)
  SELECT $1::text::uuid, $2::text::uuid, $4::bigint, $3, $5::integer, $6::integer, clock_timestamp()
  WHERE EXISTS (SELECT 1 FROM inserted)
     OR (
       EXISTS (SELECT 1 FROM gate)
       AND EXISTS (
         SELECT 1 FROM jw.effects
         WHERE task_id = $1::text::uuid AND seq = $5::integer
       )
     )
  ON CONFLICT (task_id) DO UPDATE
    SET applied = EXCLUDED.applied,
        generation = EXCLUDED.generation,
        owner = EXCLUDED.owner,
        updated_at = clock_timestamp()
    WHERE jw.checkpoints.applied < EXCLUDED.applied
  RETURNING applied
)
SELECT
  (SELECT count(*)::bigint FROM gate) AS leased,
  (SELECT count(*)::bigint FROM jw.effects WHERE task_id = $1::text::uuid AND seq = $5::integer AND EXISTS (SELECT 1 FROM gate)) AS have_seq,
  COALESCE(
    (SELECT applied FROM upsert),
    (SELECT applied FROM jw.checkpoints WHERE task_id = $1::text::uuid),
    0
  )::bigint AS applied";

pub const READ_DEDUPE: &str = "\
SELECT dedupe_key FROM jobs.tasks WHERE id = $1::text::uuid AND project_id = $2::text::uuid";

pub const FENCE_EFFECT: &str = "\
WITH gate AS (
  SELECT id
  FROM jobs.tasks
  WHERE id = $1::text::uuid
    AND project_id = $2::text::uuid
    AND state = 'running'
    AND lease_owner = $3
    AND generation = $4
    AND lease_until > clock_timestamp()
),
inserted AS (
  INSERT INTO jw.effects (task_id, seq, project_id, generation, owner, txid, applied_at)
  SELECT $1::text::uuid, 31, $2::text::uuid, $4::bigint, $3, pg_current_xact_id()::text, clock_timestamp()
  WHERE EXISTS (SELECT 1 FROM gate)
  ON CONFLICT (task_id, seq) DO NOTHING
  RETURNING seq
)
SELECT
  (SELECT count(*)::bigint FROM gate) AS leased,
  (SELECT count(*)::bigint FROM inserted) AS inserted";

pub const XACT_COMMIT: &str = "\
SELECT pg_stat_get_db_xact_commit(oid)::bigint
FROM pg_database
WHERE datname = current_database()";

pub const OBSERVE: &str = "\
INSERT INTO jw.observations (name, value, observed_at)
VALUES ($1, $2, clock_timestamp())
ON CONFLICT (name) DO UPDATE
SET value = EXCLUDED.value, observed_at = clock_timestamp()";
