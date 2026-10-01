#!/bin/sh
set -eu
command -v pg_isready >/dev/null || exit 1
command -v psql >/dev/null || exit 1
pg_isready -U oo_fixture_admin -d oo_agent || exit 1
out=$(psql -v ON_ERROR_STOP=1 -U oo_fixture_admin -d oo_agent -tAc "SELECT pg_is_in_recovery()") || exit 1
[ "$out" = "t" ] || exit 1
out=$(psql -v ON_ERROR_STOP=1 -U oo_fixture_admin -d oo_agent -tAc "SELECT status FROM pg_stat_wal_receiver") || exit 1
[ "$out" = "streaming" ] || exit 1
