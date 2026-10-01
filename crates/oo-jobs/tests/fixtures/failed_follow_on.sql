BEGIN;
SET ROLE oo_jobs_owner;
CREATE TABLE jobs._rollback_probe (id integer);
INSERT INTO jobs.schema_migrations (version, checksum, phase, filename)
VALUES (
  'jobs-control/2',
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
