#!/bin/bash
# Create the two login roles from hex password files. The SQL text is not written to the evidence directory.
# Database name is oo_a345. This card does not create oo_a326 or oo_a340.
# Origin: a326 pg/init-roles.sh, with the database name changed.
set -eu
hex48() {
  value=$(tr -d '\n' < "$1")
  case "$value" in
    *[!0-9a-fA-F]*|"") echo "password file is not hex" >&2; exit 1 ;;
  esac
  if [ "${#value}" -ne 48 ]; then
    echo "password file is not 48 hex characters" >&2
    exit 1
  fi
  printf '%s' "$value"
}
admin=$(hex48 /secrets/admin.pass)
token_admin=$(hex48 /secrets/token-admin.pass)
token_runtime=$(hex48 /secrets/token-runtime.pass)
export PGPASSWORD="$admin"
psql -v ON_ERROR_STOP=1 -U oo_fixture_admin -d oo_a345 \
  -c "CREATE ROLE oo_token_admin LOGIN PASSWORD '${token_admin}'" \
  -c "CREATE ROLE oo_token_runtime LOGIN PASSWORD '${token_runtime}'" \
  -c "REVOKE CREATE ON DATABASE oo_a345 FROM PUBLIC" \
  -c "GRANT CREATE ON DATABASE oo_a345 TO oo_token_admin"
unset PGPASSWORD
unset admin token_admin token_runtime
echo "roles_ready"
