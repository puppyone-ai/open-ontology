\ir prelude.sql
SELECT jobs.enqueue('11111111-1111-4111-8111-111111111111'::uuid, 'alpha', 1, '{"a":1}', 'cfg-1', 'abcd', 'dedupe-bounds-digest', clock_timestamp(), 3);
