SELECT pg_terminate_backend(pid)::text FROM pg_stat_activity WHERE application_name = 'a099post' AND pid <> pg_backend_pid();
