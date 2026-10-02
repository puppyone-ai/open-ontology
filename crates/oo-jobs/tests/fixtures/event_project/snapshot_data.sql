SELECT 'task', id::text, project_id::text, kind, payload_version::text, payload::text,
       config_version, artifact_digest, dedupe_key, state, run_at::text, attempt::text,
       max_attempts::text, coalesce(lease_owner, ''), coalesce(lease_until::text, ''),
       generation::text, coalesce(last_error, ''), created_at::text, updated_at::text,
       ctid::text, xmin::text
FROM jobs.tasks
ORDER BY id;

SELECT 'event', event_id::text, project_id::text, task_id::text, coalesce(from_state, ''),
       to_state, generation::text, attempt::text, coalesce(lease_owner, ''),
       occurred_at::text, coalesce(note, ''), ctid::text, xmin::text
FROM jobs.task_events
ORDER BY event_id;

SELECT 'schedule', id::text, project_id::text, kind, config_version, artifact_digest,
       dedupe_prefix, next_run_at::text, enabled::text, created_at::text, updated_at::text,
       ctid::text
FROM jobs.schedules
ORDER BY id;

SELECT 'occurrence', schedule_id::text, scheduled_at::text, project_id::text, task_id::text,
       created_at::text, ctid::text
FROM jobs.schedule_occurrences
ORDER BY schedule_id, scheduled_at;

SELECT 'lease', id::text, project_id::text, target_key, coalesce(owner, ''),
       coalesce(lease_until::text, ''), generation::text, coalesce(holder_task_id::text, ''),
       exclusivity_domain, target_fence_certified::text, created_at::text, updated_at::text,
       ctid::text
FROM jobs.leases
ORDER BY id;

SELECT 'lock', id::text FROM jobs.migration_lock ORDER BY id;
