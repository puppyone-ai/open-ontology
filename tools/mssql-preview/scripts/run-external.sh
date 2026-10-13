#!/bin/sh
# Run the customer catalog against a config path the operator supplies.
# This script does not search the home directory, does not accept an EULA,
# and does not start or pull SQL Server.
set -eu
if [ "${1:-}" = "" ]; then
  echo "mssql-preview: usage: run-external.sh /absolute/path/to/mssql-preview" >&2
  exit 2
fi
CONFIG=${OO_MSSQL_CUSTOMER_CONFIG:-}
if [ -z "$CONFIG" ]; then
  echo "mssql-preview: OO_MSSQL_CUSTOMER_CONFIG is unset; native TDS read was not run" >&2
  exit 3
fi
case $CONFIG in
  /*) ;;
  *)
    echo "mssql-preview: OO_MSSQL_CUSTOMER_CONFIG must be an absolute path" >&2
    exit 2
    ;;
esac
OUT=${OO_MSSQL_CATALOG_OUT:-}
if [ -z "$OUT" ]; then
  echo "mssql-preview: OO_MSSQL_CATALOG_OUT is unset; native TDS read was not run" >&2
  exit 3
fi
case $OUT in
  /*) ;;
  *)
    echo "mssql-preview: OO_MSSQL_CATALOG_OUT must be an absolute path" >&2
    exit 2
    ;;
esac
exec "$1" catalog --config "$CONFIG" --out "$OUT"
