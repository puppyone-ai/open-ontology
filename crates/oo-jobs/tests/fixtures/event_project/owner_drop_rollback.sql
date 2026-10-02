\set ON_ERROR_STOP on
BEGIN;
ALTER TABLE jobs.task_events DROP CONSTRAINT task_events_task_project_fkey;
ROLLBACK;
