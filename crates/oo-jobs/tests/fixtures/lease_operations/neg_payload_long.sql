\ir prelude.sql
SELECT jobs.enqueue('11111111-1111-4111-8111-111111111111'::uuid, 'alpha', 1, repeat('a', 16385), 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-bounds-long', clock_timestamp(), 3);
