SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name = 'a099drop' AND pid <> pg_backend_pid();
