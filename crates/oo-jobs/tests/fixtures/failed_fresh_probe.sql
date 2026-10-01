SELECT
  to_regnamespace('jobs_partial') IS NULL AS schema_absent,
  to_regclass('jobs.schema_migrations') IS NULL AS bookkeeping_absent;
