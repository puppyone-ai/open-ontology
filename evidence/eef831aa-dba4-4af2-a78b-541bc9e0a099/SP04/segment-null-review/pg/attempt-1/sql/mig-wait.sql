BEGIN;
SET LOCAL lock_timeout = '2s';
SELECT pg_catalog.pg_advisory_xact_lock(6404202605);
COMMIT;
