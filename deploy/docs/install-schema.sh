#!/bin/sh
# One-shot schema install (10 §3.1 rule 3): the engine installs the schema when
# OO_DOCS_ADMIN_DATABASE_URL is set, so run it, wait until it reports
# `listening` (the schema is in by then), and stop it. Idempotent: the
# installer compares checksums under an advisory lock.
set -eu
out=/tmp/install.out
: > "$out"
/usr/local/bin/oo-docs-engine > "$out" 2>/tmp/install.err &
pid=$!
i=0
while [ $i -lt 240 ]; do
  if grep -q '"listening"' "$out"; then
    kill -TERM "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    echo "schema installed"
    exit 0
  fi
  if ! kill -0 "$pid" 2>/dev/null; then
    cat /tmp/install.err >&2
    echo "engine exited during schema install" >&2
    exit 3
  fi
  i=$((i + 1))
  sleep 0.5
done
kill -KILL "$pid" 2>/dev/null || true
echo "schema install timed out" >&2
exit 4
