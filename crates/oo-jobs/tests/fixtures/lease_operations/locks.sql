\ir prelude.sql
SELECT
  (SELECT count(*) FROM pg_stat_activity
    WHERE usename = 'oo_a102_runtime_login'
      AND state = 'idle in transaction')::text
  || '|' ||
  (SELECT count(*) FROM pg_locks AS lock_row
    JOIN pg_stat_activity AS activity
      ON activity.pid = lock_row.pid
    JOIN pg_class AS class_row
      ON class_row.oid = lock_row.relation
    JOIN pg_namespace AS namespace_row
      ON namespace_row.oid = class_row.relnamespace
    WHERE activity.usename = 'oo_a102_runtime_login'
      AND namespace_row.nspname = 'jobs'
      AND class_row.relname = 'tasks'
      AND lock_row.granted)::text;
