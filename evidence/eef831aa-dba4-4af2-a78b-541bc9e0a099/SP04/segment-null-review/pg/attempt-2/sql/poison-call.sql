SET search_path = a099_evil, public, pg_catalog;
SELECT 'ADOPT|' || _oo.adopt_target_generation('a099poison', '11111111-1111-4111-8111-111111111111'::uuid)::text;
SELECT 'RESULT|' || _oo.commit_apply_receipt('a099poison', '11111111-1111-4111-8111-111111111111'::uuid, 1, 'warehouse', 'orders_g1', 'poison-batch', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, 0, 1, '\x01'::bytea);
