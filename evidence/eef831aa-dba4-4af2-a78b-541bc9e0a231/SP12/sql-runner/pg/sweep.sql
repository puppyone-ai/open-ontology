SELECT pg_terminate_backend(pid) AS terminated, pid, state, left(query, 48) AS query_prefix
FROM pg_stat_activity
WHERE pid <> pg_backend_pid()
  AND datname = 'oo_a198'
  AND state <> 'idle';
