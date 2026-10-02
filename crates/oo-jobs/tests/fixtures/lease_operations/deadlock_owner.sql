\ir prelude.sql
SET application_name = 'a102-deadlock-owner';
BEGIN;
LOCK TABLE jobs.task_events IN SHARE ROW EXCLUSIVE MODE;
SELECT pg_sleep(8);
LOCK TABLE jobs.tasks IN SHARE ROW EXCLUSIVE MODE;
COMMIT;
