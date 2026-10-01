BEGIN;
CREATE SCHEMA jobs_partial;
CREATE TABLE jobs_partial.tasks (id integer);
DO $$
BEGIN
  RAISE EXCEPTION 'forced migration failure' USING ERRCODE = 'OO003';
END
$$;
COMMIT;
