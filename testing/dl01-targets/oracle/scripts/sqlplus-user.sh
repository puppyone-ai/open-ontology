#!/bin/bash
# Runs inside the database container. Reads a password file. Does not echo it.
set -euo pipefail
user=${1:?user}
sqlfile=${2:?sql}
arg=${3:-}
pwfile="/run/secrets/${user}-passwd"
if [[ ! -r "$pwfile" ]]; then
  echo "password file unreadable for ${user}" >&2
  exit 2
fi
pw=$(cat "$pwfile")
if [[ -z "${ORACLE_HOME:-}" ]]; then
  echo "ORACLE_HOME required" >&2
  exit 2
fi
export PATH="${ORACLE_HOME}/bin:${PATH:-}"
export LD_LIBRARY_PATH="${ORACLE_HOME}/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export NLS_LANG="${NLS_LANG:-AMERICAN_AMERICA.AL32UTF8}"
if [[ -n "$arg" ]]; then
  script_line="@${sqlfile} ${arg}"
else
  script_line="@${sqlfile}"
fi
run_sqlplus() {
  sqlplus -s /nolog <<EOF
set echo off
set trimout on
set tab off
set linesize 500
set pagesize 0
set heading off
set feedback off
whenever sqlerror exit sql.sqlcode
connect ${user}/"${pw}"@//127.0.0.1/FREEPDB1
${script_line}
exit
EOF
}
if [[ -n "${SPOOL:-}" ]]; then
  run_sqlplus >"${SPOOL}" 2>&1
else
  run_sqlplus
fi
