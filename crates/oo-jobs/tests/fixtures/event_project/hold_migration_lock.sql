\set ON_ERROR_STOP on
SET statement_timeout = '15s';
BEGIN;
SELECT pg_advisory_xact_lock(6401202601);
SELECT id FROM jobs.migration_lock WHERE id = 1 FOR UPDATE;
SELECT pg_sleep(6);
COMMIT;
