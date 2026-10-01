#!/bin/bash
# The recipe is present and is not a running CDC pipeline.
set -euo pipefail

here=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
props=$here/application.properties
recipe=$here/debezium.recipe.yaml
test -s "$props"
test -s "$recipe"
grep -q 'debezium.source.connector.class=io.debezium.connector.postgresql.PostgresConnector' "$props"
grep -q 'publication.autocreate.mode=disabled' "$props"
grep -q 'sha256:842a0aa66dfab8a8cbe40634d53df5d1e5b4b5e9c7c8ea968d68f7cbc947c175' "$recipe"
echo "debezium_cdc=not_started"
echo "debezium_recipe=present"
echo "p3_p11=not_accepted"
echo "status=pending"
exit 2
