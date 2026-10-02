\set ON_ERROR_STOP on
CREATE OR REPLACE FUNCTION public.clock_timestamp()
RETURNS timestamptz
LANGUAGE sql
AS $$ SELECT '2000-01-01 00:00:00+00'::timestamptz $$;
