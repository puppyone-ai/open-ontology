#!/bin/bash
# Copy stdout, stderr, the exit code, the request, and an audit file when
# query-pg creates one. The bytes are the binary's. This script adds none.
set -u
REAL=/gw-target/debug/query-pg
CAP=/out/capture
mkdir -p "$CAP"
req=""
prev=""
for arg in "$@"; do
  if [ "$prev" = "--request" ]; then
    req=$arg
  fi
  prev=$arg
done
if [ -z "$req" ] || [ ! -f "$req" ]; then
  echo "wrapper: missing request" >&2
  exit 3
fi
id=$(basename "$req" .json)
cp "$req" "$CAP/${id}.request.json"
"$REAL" "$@" > "$CAP/${id}.stdout" 2> "$CAP/${id}.stderr"
rc=$?
printf '%s\n' "$rc" > "$CAP/${id}.exit"
audit=$(sed -n 's/.*"audit_file":"\([^"]*\)".*/\1/p' "$req" | head -n 1)
if [ -n "$audit" ] && [ -f "$audit" ]; then
  cp "$audit" "$CAP/${id}.audit.jsonl"
fi
cat "$CAP/${id}.stdout"
exit "$rc"
