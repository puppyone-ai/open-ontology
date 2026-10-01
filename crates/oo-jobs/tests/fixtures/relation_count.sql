SELECT count(*)
FROM pg_class AS class
JOIN pg_namespace AS namespace ON namespace.oid = class.relnamespace
WHERE namespace.nspname = 'jobs'
  AND class.relkind IN ('r', 'v', 'i', 'S', 'I');
