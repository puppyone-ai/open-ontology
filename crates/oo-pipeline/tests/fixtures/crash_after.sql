\set ON_ERROR_STOP on
SET application_name = 'a072-crash-after';
SELECT _oo.adopt_target_generation(
  'shard_crash_after',
  '11111111-1111-4111-8111-111111111111'::uuid
);
BEGIN;
SELECT fixture_synth.commit_bundle(
  'shard_crash_after',
  '11111111-1111-4111-8111-111111111111'::uuid,
  1,
  'batch-crash-after',
  repeat('e', 64)
);
COMMIT;
SELECT pg_catalog.pg_sleep(30);
