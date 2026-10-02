\ir prelude.sql
SELECT state || '|' || generation::text
FROM jobs.tasks
WHERE dedupe_key = 'dedupe-over';
