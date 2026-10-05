SELECT count(*) AS oo_a198_backends
FROM pg_stat_activity
WHERE datname = 'oo_a198';
SELECT pid, state, wait_event_type, backend_type, left(query, 48) AS query_prefix
FROM pg_stat_activity
WHERE datname = 'oo_a198'
ORDER BY pid;
