\set ON_ERROR_STOP on
UPDATE jobs.task_events
SET project_id = '22222222-2222-4222-8222-222222222222',
    task_id = 'a0750001-0001-4000-8000-000000000013'
WHERE task_id = 'a0750001-0001-4000-8000-000000000012'
  AND note = 'same-project direct';
