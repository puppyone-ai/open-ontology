#!/bin/bash
# Read one published role through a fixed Gateway `oo` binary.
# The binary must be built from commit 91031011238c39e571a28a90831b012f225862d6.
# Any source uses the same command. This script does not compile and does not print the token.
set -eu
: "${OO_BIN:?}"
: "${TOKEN_FILE:?}"
: "${GATEWAY_ORIGIN:?}"
: "${SQL:?}"
exec "$OO_BIN" query --json --auth external \
  --token-file "$TOKEN_FILE" \
  --gateway "$GATEWAY_ORIGIN" \
  --transport local-development \
  "$SQL"
