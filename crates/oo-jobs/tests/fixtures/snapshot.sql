SELECT 'task', id::text, project_id::text, dedupe_key, state, attempt::text, max_attempts::text,
       generation::text, lease_owner, coalesce(lease_until::text, ''), coalesce(last_error, ''),
       artifact_digest, payload::text, payload_version::text, config_version
FROM jobs.tasks
ORDER BY id;

SELECT 'schedule', id::text, project_id::text, dedupe_prefix, artifact_digest, enabled::text, next_run_at::text
FROM jobs.schedules
ORDER BY id;

SELECT 'occurrence', schedule_id::text, scheduled_at::text, project_id::text, task_id::text
FROM jobs.schedule_occurrences
ORDER BY schedule_id, scheduled_at;

SELECT 'lease', id::text, project_id::text, target_key, coalesce(owner, ''), generation::text,
       exclusivity_domain, target_fence_certified::text, coalesce(holder_task_id::text, '')
FROM jobs.leases
ORDER BY id;

SELECT 'migration', version, checksum, phase, filename
FROM jobs.schema_migrations
ORDER BY version;
