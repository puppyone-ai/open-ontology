\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
SELECT current_database();
SELECT count(*)::text
FROM pg_namespace
WHERE nspname IN ('_oo', 'fixture_synth');
