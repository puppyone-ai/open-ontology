\set ON_ERROR_STOP on
BEGIN;
SET ROLE oo_target_owner;
CREATE TABLE _oo._rollback_probe (id integer);
INSERT INTO _oo.schema_migrations (version, checksum, phase, filename)
VALUES (
  'target-fence-receipts/2',
  'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
  'migrate',
  'failed_follow_on.sql'
);
DO $$
BEGIN
  RAISE EXCEPTION 'forced migration failure' USING ERRCODE = 'OO003';
END
$$;
COMMIT;
