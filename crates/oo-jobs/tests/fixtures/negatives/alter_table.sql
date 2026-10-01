\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
ALTER TABLE jobs.tasks ADD COLUMN injected text;
