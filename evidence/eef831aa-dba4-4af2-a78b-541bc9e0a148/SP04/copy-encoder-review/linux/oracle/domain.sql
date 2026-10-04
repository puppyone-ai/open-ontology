\set ON_ERROR_STOP on
\set VERBOSITY verbose
BEGIN;
CREATE SCHEMA oo_copy_a148_chk;
CREATE DOMAIN oo_copy_a148_chk.pos_int AS integer CHECK (VALUE > 0);
CREATE TABLE oo_copy_a148_chk.checked (
  pos numeric,
  n oo_copy_a148_chk.pos_int
);
\copy oo_copy_a148_chk.checked (pos, n) FROM '/tmp/domain-violation.copy' WITH (FORMAT binary)
