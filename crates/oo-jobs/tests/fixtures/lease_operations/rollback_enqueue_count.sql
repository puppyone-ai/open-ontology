\ir prelude.sql
SELECT count(*)::text
FROM jobs.tasks
WHERE dedupe_key = 'dedupe-rollback';
