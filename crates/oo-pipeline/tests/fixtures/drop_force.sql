\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
DROP FUNCTION IF EXISTS fixture_synth.force_generation(text, bigint);
DROP FUNCTION IF EXISTS fixture_synth.force_lock_key(text, integer, integer);
SELECT count(*)
FROM pg_proc AS pg_proc
JOIN pg_namespace AS pg_namespace ON pg_namespace.oid = pg_proc.pronamespace
WHERE pg_namespace.nspname = 'fixture_synth'
  AND pg_proc.proname LIKE 'force%';
