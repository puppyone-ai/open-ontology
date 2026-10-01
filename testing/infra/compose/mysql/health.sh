#!/bin/sh
# Healthy only when the client exists, the synthetic table is present, and
# statement logging is already on. A missing mysql binary fails the check.
set -eu
command -v mysql >/dev/null || exit 1
test -r /run/secrets/mysql_root_password || exit 1
umask 077
cnf=/tmp/oo-hc.cnf
trap 'rm -f "$cnf"' EXIT
{
  echo '[client]'
  echo 'user=root'
  echo "password=$(tr -d '\n' < /run/secrets/mysql_root_password)"
  echo 'protocol=socket'
} > "$cnf"
chmod 600 "$cnf"
count=$(mysql --defaults-extra-file="$cnf" -N -e "SELECT COUNT(*) FROM oo_local_test.shipment_leg")
[ "$count" = "2" ]
on=$(mysql --defaults-extra-file="$cnf" -N -e "SELECT @@global.general_log")
[ "$on" = "1" ]
output=$(mysql --defaults-extra-file="$cnf" -N -e "SELECT @@global.log_output")
[ "$output" = "TABLE" ]
