\ir prelude.sql
CREATE FUNCTION public.clock_timestamp()
RETURNS timestamptz
LANGUAGE sql
AS $$ SELECT pg_catalog.clock_timestamp() + interval '24 hours' $$;
