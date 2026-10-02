\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
SELECT (to_regclass('_oo._rollback_probe') IS NULL);
SELECT version FROM _oo.schema_migrations ORDER BY version;
