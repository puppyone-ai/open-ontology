#!/bin/sh
set -eu
command -v pg_isready >/dev/null || exit 1
command -v psql >/dev/null || exit 1
pg_isready -U oo_fixture_admin -d oo_control || exit 1
out=$(psql -v ON_ERROR_STOP=1 -U oo_fixture_admin -d oo_control -tAc "SELECT count(*) FROM oo_fixture.ownership") || exit 1
[ "$out" = "1" ] || exit 1
