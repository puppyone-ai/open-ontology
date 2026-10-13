#!/bin/sh
# Argument-boundary test only: the spy records argv; no source is represented.
set -eu
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SCRATCH=$(mktemp -d)
trap 'rm -rf "$SCRATCH"' EXIT HUP INT TERM
export OO_MSSQL_TEST_ARGV="$SCRATCH/argv"
cat > "$SCRATCH/spy" <<'EOF'
#!/bin/sh
printf '%s\n' "$@" > "$OO_MSSQL_TEST_ARGV"
EOF
chmod 700 "$SCRATCH/spy"
export OO_MSSQL_CUSTOMER_CONFIG=/private/operator/customer.json
export OO_MSSQL_CATALOG_OUT=relative.json
set +e
sh "$SCRIPT_DIR/run-external.sh" "$SCRATCH/spy" 2> "$SCRATCH/error"
STATUS=$?
set -e
test "$STATUS" = 2
test ! -e "$OO_MSSQL_TEST_ARGV"
export OO_MSSQL_CATALOG_OUT="$SCRATCH/catalog with spaces.json"
sh "$SCRIPT_DIR/run-external.sh" "$SCRATCH/spy"
printf '%s\n' catalog --config "$OO_MSSQL_CUSTOMER_CONFIG" --out "$OO_MSSQL_CATALOG_OUT" > "$SCRATCH/expected"
cmp "$SCRATCH/expected" "$OO_MSSQL_TEST_ARGV"
echo 'external path boundary: relative output refused before invocation; absolute paths forwarded intact'
