#!/bin/bash
# DBA identity. Creates the reader and the no-flash user from password files.
set -euo pipefail
reader_pw=$(cat /run/secrets/oo_reader-passwd)
noflash_pw=$(cat /run/secrets/oo_noflash-passwd)
if [[ -z "${ORACLE_HOME:-}" ]]; then
  echo "ORACLE_HOME required" >&2
  exit 2
fi
export PATH="${ORACLE_HOME}/bin:${PATH:-}"
export LD_LIBRARY_PATH="${ORACLE_HOME}/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export NLS_LANG="${NLS_LANG:-AMERICAN_AMERICA.AL32UTF8}"
sqlplus -s / as sysdba <<EOF
set echo off feedback off heading off pagesize 0
whenever sqlerror exit sql.sqlcode
alter session set container = FREEPDB1;
create user oo_reader identified by "${reader_pw}" default tablespace users quota 0 on users account unlock;
create user oo_noflash identified by "${noflash_pw}" default tablespace users quota 0 on users account unlock;
prompt users|created
exit
EOF
