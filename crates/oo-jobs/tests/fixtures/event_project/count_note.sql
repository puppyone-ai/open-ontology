SELECT count(*)::text
FROM jobs.task_events
WHERE note = :'note';
