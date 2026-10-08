\set ON_ERROR_STOP off
\set VERBOSITY verbose
SET lock_timeout = '2s';
DO $$
BEGIN
  COMMENT ON VIEW oo_dict.enums IS 'a377-during-cut';
  RAISE NOTICE 'step comment ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step comment %', SQLSTATE;
END $$;
DO $$
BEGIN
  CREATE SCHEMA oo_a377_probe;
  RAISE NOTICE 'step schema ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step schema %', SQLSTATE;
END $$;
DO $$
BEGIN
  CREATE TABLE oo_a377_probe.added (id integer primary key);
  RAISE NOTICE 'step table ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step table %', SQLSTATE;
END $$;
DO $$
BEGIN
  INSERT INTO oo_a377_probe.added (id) VALUES (7);
  RAISE NOTICE 'step insert ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step insert %', SQLSTATE;
END $$;
DO $$
BEGIN
  CREATE SEQUENCE oo_a377_probe.seq;
  RAISE NOTICE 'step sequence ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step sequence %', SQLSTATE;
END $$;
DO $$
BEGIN
  PERFORM nextval('oo_a377_probe.seq');
  RAISE NOTICE 'step nextval ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step nextval %', SQLSTATE;
END $$;
DO $$
BEGIN
  CREATE VIEW oo_a377_probe.v AS SELECT 7 AS n;
  RAISE NOTICE 'step view ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step view %', SQLSTATE;
END $$;
