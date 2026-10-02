\ir prelude.sql
SELECT jobs.claim('11111111-1111-4111-8111-111111111111'::uuid, (SELECT array_agg('k' || g::text) FROM generate_series(1, 65) AS g), 'worker-b');
