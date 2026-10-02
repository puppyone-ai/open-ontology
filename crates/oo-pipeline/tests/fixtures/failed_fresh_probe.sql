\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
SELECT (to_regnamespace('partial_target') IS NULL),
       (to_regnamespace('_oo') IS NULL);
