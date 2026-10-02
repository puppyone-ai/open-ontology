SELECT project_id::text
FROM jobs.tasks
WHERE id = :'task_id';
