-- jobs-event-project/1 phase expand
-- Run as oo_jobs_owner after jobs-control/1, inside the applicator transaction
-- that already holds pg_advisory_xact_lock(6401202601). This file adds a
-- non-deferrable foreign key so task_events.project_id equals the task's
-- project_id. It does not modify stored task or event rows. A preexisting
-- mismatch aborts the transaction. Repair of that history is a later audited
-- plan and is not invented here.
--
-- Lock the referencing table first, then the referenced table. That is the
-- same order as an event insert (ROW EXCLUSIVE on task_events, then FOR KEY
-- SHARE on tasks) and as ADD FOREIGN KEY. The unique constraint has to exist
-- before the foreign key, and its column order is (id, project_id).

LOCK TABLE jobs.task_events IN SHARE ROW EXCLUSIVE MODE;
LOCK TABLE jobs.tasks IN SHARE ROW EXCLUSIVE MODE;

DO $bind$
DECLARE
  bad_rows bigint;
BEGIN
  SELECT count(*) INTO bad_rows
  FROM jobs.task_events AS event
  WHERE NOT EXISTS (
    SELECT 1
    FROM jobs.tasks AS task
    WHERE task.id = event.task_id
      AND task.project_id = event.project_id
  );
  IF bad_rows > 0 THEN
    RAISE EXCEPTION
      'refusing event project binding: % preexisting task_events row(s) do not match tasks.project_id; this migration does not rewrite, delete, or forge a correction',
      bad_rows
      USING ERRCODE = 'OO004';
  END IF;
END
$bind$;

ALTER TABLE jobs.tasks
  ADD CONSTRAINT tasks_id_project_unique
  UNIQUE (id, project_id)
  NOT DEFERRABLE;

COMMENT ON CONSTRAINT tasks_id_project_unique ON jobs.tasks IS
  'Unique (id, project_id) so task_events can reference both columns. id remains the primary key. Not deferrable.';

ALTER TABLE jobs.task_events
  ADD CONSTRAINT task_events_task_project_fkey
  FOREIGN KEY (task_id, project_id)
  REFERENCES jobs.tasks (id, project_id)
  ON DELETE RESTRICT
  ON UPDATE RESTRICT
  NOT DEFERRABLE;

COMMENT ON CONSTRAINT task_events_task_project_fkey ON jobs.task_events IS
  'project_id on an event must equal tasks.project_id for that task_id. Note, generation, lease_owner, and states on a direct insert stay caller-supplied. This constraint does not authenticate a worker.';

CREATE VIEW jobs.event_project_readiness
WITH (security_invoker = true) AS
SELECT
  version,
  checksum,
  phase,
  filename,
  applied_at,
  (version = 'jobs-event-project/1' AND phase = 'expand') AS schema_version_present
FROM jobs.schema_migrations
WHERE version = 'jobs-event-project/1';

COMMENT ON VIEW jobs.event_project_readiness IS
  'Version marker for jobs-event-project/1 expand. schema_version_present does not prove these file bytes match checksum. Project-scoped use of task_events also needs the jobs-control/1 checksum to match its release file. This view does not repair old events.';

REVOKE ALL ON jobs.event_project_readiness FROM PUBLIC;
GRANT SELECT ON jobs.event_project_readiness TO oo_jobs_runtime;
