\ir prelude.sql
SET application_name = 'a102-lock-holder';
BEGIN;
SELECT pg_advisory_xact_lock(6401202601);
SELECT pg_sleep(30);
COMMIT;
