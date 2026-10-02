\set ON_ERROR_STOP on
DO $check$
DECLARE
  fk_def text;
  unique_def text;
  fk_deferrable boolean;
  unique_deferrable boolean;
  update_action "char";
  delete_action "char";
  event_cols text;
  task_cols text;
  readiness_def text;
  service_def text;
BEGIN
  SELECT pg_get_constraintdef(oid), condeferrable, confupdtype, confdeltype
    INTO fk_def, fk_deferrable, update_action, delete_action
  FROM pg_constraint
  WHERE conname = 'task_events_task_project_fkey'
    AND conrelid = 'jobs.task_events'::regclass
    AND contype = 'f';
  IF fk_def NOT LIKE 'FOREIGN KEY (task_id, project_id) REFERENCES %tasks(id, project_id) ON UPDATE RESTRICT ON DELETE RESTRICT'
     OR fk_deferrable
     OR update_action <> 'r'
     OR delete_action <> 'r'
  THEN
    RAISE EXCEPTION 'event project foreign key drifted: %', fk_def USING ERRCODE = 'OO004';
  END IF;

  SELECT pg_get_constraintdef(oid), condeferrable
    INTO unique_def, unique_deferrable
  FROM pg_constraint
  WHERE conname = 'tasks_id_project_unique'
    AND conrelid = 'jobs.tasks'::regclass
    AND contype = 'u';
  IF unique_def IS DISTINCT FROM 'UNIQUE (id, project_id)' OR unique_deferrable THEN
    RAISE EXCEPTION 'task id project unique drifted: %', unique_def USING ERRCODE = 'OO004';
  END IF;

  SELECT string_agg(att.attname, ',' ORDER BY cols.ord)
    INTO event_cols
  FROM pg_constraint AS constraint_row
  JOIN unnest(constraint_row.conkey) WITH ORDINALITY AS cols(attnum, ord) ON true
  JOIN pg_attribute AS att
    ON att.attrelid = constraint_row.conrelid
   AND att.attnum = cols.attnum
  WHERE constraint_row.conname = 'task_events_task_project_fkey';
  SELECT string_agg(att.attname, ',' ORDER BY cols.ord)
    INTO task_cols
  FROM pg_constraint AS constraint_row
  JOIN unnest(constraint_row.confkey) WITH ORDINALITY AS cols(attnum, ord) ON true
  JOIN pg_attribute AS att
    ON att.attrelid = constraint_row.confrelid
   AND att.attnum = cols.attnum
  WHERE constraint_row.conname = 'task_events_task_project_fkey';
  IF event_cols IS DISTINCT FROM 'task_id,project_id'
     OR task_cols IS DISTINCT FROM 'id,project_id'
  THEN
    RAISE EXCEPTION 'foreign key column order drifted: % -> %', event_cols, task_cols
      USING ERRCODE = 'OO004';
  END IF;

  SELECT pg_get_viewdef('jobs.event_project_readiness'::regclass, true)
    INTO readiness_def;
  IF readiness_def NOT LIKE '%jobs-event-project/1%' THEN
    RAISE EXCEPTION 'event project readiness view drifted' USING ERRCODE = 'OO004';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM pg_class
    WHERE oid = 'jobs.event_project_readiness'::regclass
      AND reloptions @> ARRAY['security_invoker=true']
  ) THEN
    RAISE EXCEPTION 'event project readiness is not security invoker' USING ERRCODE = 'OO004';
  END IF;
  SELECT pg_get_viewdef('jobs.service_readiness'::regclass, true)
    INTO service_def;
  IF service_def NOT LIKE '%jobs-control/1%'
     OR service_def LIKE '%jobs-event-project/1%'
  THEN
    RAISE EXCEPTION 'jobs-control/1 readiness view changed' USING ERRCODE = 'OO004';
  END IF;

  IF (
    SELECT count(*)
    FROM pg_trigger
    WHERE tgname IN (
      'tasks_enforce_update',
      'tasks_audit_insert',
      'tasks_audit_update',
      'task_events_reject_update',
      'task_events_reject_delete'
    )
      AND tgenabled = 'O'
      AND NOT tgisinternal
  ) <> 5 THEN
    RAISE EXCEPTION 'append-only or audit trigger is missing' USING ERRCODE = 'OO004';
  END IF;
END
$check$;
