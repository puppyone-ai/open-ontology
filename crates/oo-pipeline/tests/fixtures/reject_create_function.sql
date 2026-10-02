\set ON_ERROR_STOP on
CREATE FUNCTION public.oo_a072_should_fail()
RETURNS integer
LANGUAGE sql
AS $$ SELECT 1 $$;
