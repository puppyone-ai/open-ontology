\set ON_ERROR_STOP on
SET statement_timeout = '10s';
SET lock_timeout = '1s';
BEGIN;
SELECT pg_advisory_xact_lock(6401202601);
SELECT id FROM jobs.migration_lock WHERE id = 1 FOR UPDATE;
COMMIT;
