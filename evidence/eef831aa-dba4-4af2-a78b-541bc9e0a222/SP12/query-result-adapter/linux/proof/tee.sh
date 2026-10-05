#!/bin/bash
# Pass query-pg stdout through and keep a copy of each non-empty stream.
# The real binary stays on the target volume. /tmp is noexec.
set -u
real=/target/workspace/query-pg.real
dir=/evidence/live/captures
mkdir -p "$dir"
tmp=$(mktemp "$dir/capture.XXXXXX")
"$real" "$@" | tee "$tmp"
code=${PIPESTATUS[0]}
if [ ! -s "$tmp" ]; then
  rm -f "$tmp"
else
  mv "$tmp" "${tmp}.stdout"
fi
exit "$code"
