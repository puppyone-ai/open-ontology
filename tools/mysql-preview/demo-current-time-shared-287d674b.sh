#!/bin/sh
# Actual unpublished-preview OO CLI against the current sealed MySQL TIME vector.
set -eu
PRIVATE_ROOT=/tmp/oo-dl01w1-73-time-preview
FIXED_ROOT=/Users/redux/.cache/oo-dl01w1-73/fixed-elf
TOKEN_NAME=${OO_DEMO_TOKEN_NAME:-client-demo-20261013-0143.token}
case "$TOKEN_NAME" in *[!A-Za-z0-9._-]*|.|..) echo 'Invalid token filename' >&2; exit 2;; esac
test -f "$PRIVATE_ROOT/$TOKEN_NAME"
test -f "$PRIVATE_ROOT/listen.addr"
test -x "$FIXED_ROOT/oo-fb8073fd6e01822b6d42ded74ae3673cf202e778b54bdca0ca044c909bb56703"
ADDR=$(cat "$PRIVATE_ROOT/listen.addr")
CLI=/fixed/oo-fb8073fd6e01822b6d42ded74ae3673cf202e778b54bdca0ca044c909bb56703
TOKEN=/run-current/$TOKEN_NAME
for ACTION in describe query freshness; do
  case "$ACTION" in
    describe) set -- describe --json --auth external --token-file "$TOKEN" --gateway "http://$ADDR" --transport local-development native_mysql_20261012.value_probe ;;
    query) set -- query --json --auth external --token-file "$TOKEN" --gateway "http://$ADDR" --transport local-development 'SELECT probe_id,clock_at FROM native_mysql_20261012.value_probe ORDER BY probe_id' ;;
    freshness) set -- freshness --json --auth external --token-file "$TOKEN" --gateway "http://$ADDR" --transport local-development ;;
  esac
  printf '\n%s\n' "$ACTION"
  docker run --rm --network container:oo-dl01w1-73-time-preview \
    -v "$FIXED_ROOT:/fixed:ro" -v "$PRIVATE_ROOT:/run-current:ro" \
    docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 \
    "$CLI" "$@"
done
