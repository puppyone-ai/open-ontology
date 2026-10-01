DO $drop$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'oo_jobs_nobody') THEN
    EXECUTE 'DROP OWNED BY oo_jobs_nobody';
    DROP ROLE oo_jobs_nobody;
  END IF;
END
$drop$;
