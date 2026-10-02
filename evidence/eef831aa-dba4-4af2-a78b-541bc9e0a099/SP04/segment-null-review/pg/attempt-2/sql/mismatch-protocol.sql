BEGIN;
SET LOCAL lock_timeout = '3s';
SELECT pg_catalog.pg_advisory_xact_lock(6404202605);
SET ROLE oo_target_owner;
SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;
DO $check$ DECLARE recorded text; BEGIN SELECT checksum INTO recorded FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/2'; IF recorded IS DISTINCT FROM 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee' THEN RAISE EXCEPTION 'migration checksum mismatch' USING ERRCODE = 'OO001'; END IF; END $check$;
RESET ROLE;
COMMIT;
