\ir prelude.sql
BEGIN;
SET ROLE oo_jobs_owner;
CREATE FUNCTION jobs.claim()
RETURNS integer
LANGUAGE sql
AS $$ SELECT 7 $$;
RESET ROLE;
COMMIT;
