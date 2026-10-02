SELECT _oo.adopt_target_generation('a099replay', '11111111-1111-4111-8111-111111111111'::uuid) AS gen \gset
BEGIN;
INSERT INTO public.side_note VALUES (1, 'side');
SELECT 'RESULT|' || _oo.commit_apply_receipt('a099replay', '11111111-1111-4111-8111-111111111111'::uuid, :gen, 'warehouse', 'orders_g1', 'replay-batch', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('f', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, 0, 1, '\x01'::bytea);
COMMIT;
