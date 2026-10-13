#!/bin/bash
# Pre-edit smoke. No cargo fetch, no tests, no PostgreSQL.
set -u
export HOME=/tmp
EV=/evidence
mkdir -p "$EV/phases"
record() {
  printf '%s\n' "$2" > "$EV/phases/$1.exit"
  echo "PHASE $1=$2"
}
got=$(rustc --version)
printf '%s\n' "$got" > "$EV/rustc-version.txt"
uname -m > "$EV/uname.txt"
case "$got" in
  "rustc 1.94.0 "*) record toolchain 0 ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2; exit 2 ;;
esac
if ! grep -q aarch64 "$EV/uname.txt"; then
  echo "not arm64" >&2
  record arch 2
  exit 2
fi
record arch 0
if [ -e /src/testing/minimal-loop-acceptance/src ]; then
  echo "implementation source exists before smoke" >&2
  printf '%s\n' present > "$EV/overlay.txt"
  record overlay 2
  exit 2
fi
printf '%s\n' absent > "$EV/overlay.txt"
record overlay 0
printf '%s\n' 0 > "$EV/smoke.exit"
echo "smoke=ok"
exit 0
