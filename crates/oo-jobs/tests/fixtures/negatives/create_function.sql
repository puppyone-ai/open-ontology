\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
CREATE FUNCTION jobs.evil() RETURNS integer LANGUAGE sql AS 'SELECT 1';
