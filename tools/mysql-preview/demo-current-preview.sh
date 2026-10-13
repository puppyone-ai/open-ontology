#!/bin/sh
# Reproduce the fixed, unpublished MySQL native preview with the existing OO CLI.
set -eu
OWN=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
DEMO="$OWN/demo-current-time-shared-287d674b.sh"
EXPECTED=287d674bab07e3ff67488c8aa06f709dcb8ebb8ec25a5ebd93bca141118a60f9
ACTUAL=$(shasum -a 256 "$DEMO" | awk '{print $1}')
if [ "$ACTUAL" != "$EXPECTED" ]; then
  echo 'Fixed MySQL demo snapshot changed; verify its identity before using it.' >&2
  exit 2
fi
if [ "${OO_DEMO_TOKEN_FILE:-}" ]; then
  ROOT=/tmp/oo-dl01w1-73-time-preview
  case "$OO_DEMO_TOKEN_FILE" in
    "$ROOT"/*) OO_DEMO_TOKEN_NAME=${OO_DEMO_TOKEN_FILE#"$ROOT"/}; export OO_DEMO_TOKEN_NAME ;;
    *) echo 'Token file must be under the configured private runtime directory.' >&2; exit 2 ;;
  esac
  case "$OO_DEMO_TOKEN_NAME" in
    *[!A-Za-z0-9._-]*|.|..) echo 'Invalid token filename.' >&2; exit 2 ;;
  esac
fi
exec sh "$DEMO"
