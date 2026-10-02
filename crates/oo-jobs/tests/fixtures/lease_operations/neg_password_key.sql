\ir prelude.sql
SELECT jobs.enqueue('11111111-1111-4111-8111-111111111111'::uuid, 'alpha', 1, '{"password":"canary-a102-secret-do-not-echo"}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-bounds-password', clock_timestamp(), 3);
