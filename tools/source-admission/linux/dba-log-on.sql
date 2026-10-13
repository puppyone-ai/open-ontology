ALTER SYSTEM SET log_statement = 'all';
SELECT pg_reload_conf();
SELECT pg_rotate_logfile();
