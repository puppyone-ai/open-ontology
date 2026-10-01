\set ON_ERROR_STOP on
-- SET ROLE checks the session user. A superuser session can still SET ROLE
-- to the owner after SET ROLE runtime. This switches the session user.
SET SESSION AUTHORIZATION oo_jobs_runtime;
SET ROLE oo_jobs_owner;
