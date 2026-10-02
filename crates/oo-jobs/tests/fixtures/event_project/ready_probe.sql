SELECT set_config('jobs.checksum_bootstrap', :'checksum_bootstrap', false);
SELECT set_config('jobs.checksum_expand', :'checksum_expand', false);
SELECT set_config('jobs.checksum_event_project', :'checksum_event_project', false);
SELECT (
  (SELECT schema_version_present AND checksum = current_setting('jobs.checksum_expand')
   FROM jobs.service_readiness)
  AND (SELECT checksum = current_setting('jobs.checksum_bootstrap')
       FROM jobs.schema_migrations
       WHERE version = 'jobs-bootstrap/1')
  AND (SELECT COALESCE(bool_and(
         schema_version_present
         AND checksum = current_setting('jobs.checksum_event_project')
       ), false)
       FROM jobs.event_project_readiness)
) AS ready;
