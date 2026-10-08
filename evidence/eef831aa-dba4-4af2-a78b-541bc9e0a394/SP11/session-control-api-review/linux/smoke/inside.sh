#!/bin/bash
# Pre-oracle smoke. Version and archive hashes only. No cargo, no edits.
set -u
EV=/evidence
rustc --version > "$EV/version.txt" 2>&1
printf '%s\n' $? > "$EV/version.exit"
rustc -vV > "$EV/rustc-verbose.txt" 2>&1
uname -m > "$EV/uname.txt"
if [ -x /usr/local/bin/runsc ] || [ -x /usr/bin/runsc ] || [ -f /usr/local/bin/runsc ] || [ -f /usr/bin/runsc ]; then
  printf '%s\n' present > "$EV/runsc.txt"
else
  printf '%s\n' absent > "$EV/runsc.txt"
fi
bad=0
rows=0
: > "$EV/hash-check.txt"
while read -r sha nbytes path; do
  case "$sha" in
    ""|\#*) continue ;;
  esac
  full="/archive/tools/session-control-api/${path}"
  got=$(sha256sum "$full" | awk '{print $1}')
  size=$(wc -c < "$full" | tr -d ' ')
  rows=$((rows + 1))
  if [ "$got" != "$sha" ] || [ "$size" != "$nbytes" ]; then
    bad=$((bad + 1))
    printf 'MISMATCH %s\n' "$path" >> "$EV/hash-check.txt"
  fi
done < /inventory/SOURCE-INVENTORY.txt
printf 'files=%s mismatches=%s\n' "$rows" "$bad" >> "$EV/hash-check.txt"
if [ "$bad" -eq 0 ] && [ "$rows" -eq 18 ]; then
  printf '%s\n' 0 > "$EV/hash-check.exit"
else
  printf '%s\n' 1 > "$EV/hash-check.exit"
fi
if [ "$(cat "$EV/version.exit")" != 0 ] || [ "$(cat "$EV/hash-check.exit")" != 0 ]; then
  exit 1
fi
grep -q 'rustc 1.94.0' "$EV/version.txt" || exit 1
grep -q 'aarch64-unknown-linux-gnu' "$EV/rustc-verbose.txt" || exit 1
grep -q '^aarch64$' "$EV/uname.txt" || exit 1
exit 0
