SELECT tgname || '|' || tgenabled::text
FROM pg_trigger
WHERE tgname = 'tasks_enforce_update'
  AND NOT tgisinternal;
