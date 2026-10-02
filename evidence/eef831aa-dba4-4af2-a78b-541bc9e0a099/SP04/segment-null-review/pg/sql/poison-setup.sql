CREATE SCHEMA IF NOT EXISTS a099_evil;
CREATE OR REPLACE FUNCTION a099_evil.clock_timestamp() RETURNS timestamptz LANGUAGE sql AS $$ SELECT '1999-01-01 00:00:00+00'::timestamptz $$;
CREATE OR REPLACE FUNCTION _oo.clock_timestamp() RETURNS timestamptz LANGUAGE sql AS $$ SELECT '1998-01-01 00:00:00+00'::timestamptz $$;
CREATE OR REPLACE FUNCTION _oo.octet_length(segment_bitmap bytea) RETURNS integer LANGUAGE sql AS $$ SELECT 0 $$;
