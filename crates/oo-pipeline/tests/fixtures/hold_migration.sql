\set ON_ERROR_STOP on
SET application_name = 'a072-migration';
SET statement_timeout = '15s';
BEGIN;
SELECT pg_catalog.pg_advisory_xact_lock(6404202605);
SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;
SELECT pg_catalog.pg_sleep(5);
COMMIT;
