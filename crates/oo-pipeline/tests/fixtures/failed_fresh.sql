\set ON_ERROR_STOP on
BEGIN;
CREATE SCHEMA partial_target;
CREATE TABLE partial_target.applied_row (id integer);
DO $$
BEGIN
  RAISE EXCEPTION 'forced migration failure' USING ERRCODE = 'OO003';
END
$$;
COMMIT;
