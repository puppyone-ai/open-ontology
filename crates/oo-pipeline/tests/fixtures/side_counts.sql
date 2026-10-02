\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
\pset fieldsep '|'
SELECT
  (SELECT count(*) FROM fixture_synth.applied_row WHERE batch_id = 'rollback_temp'),
  (SELECT count(*) FROM fixture_synth.quality_row WHERE batch_id = 'rollback_temp');
