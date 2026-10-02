\ir prelude.sql
SELECT count(*)::text || '|' || coalesce(min(state), '')
FROM jobs.tasks
WHERE dedupe_key = 'dedupe-dead';
