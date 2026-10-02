\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
SELECT current_user, session_user, rolsuper
FROM pg_roles
WHERE rolname = current_user;
