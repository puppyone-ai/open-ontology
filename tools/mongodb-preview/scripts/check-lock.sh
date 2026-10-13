#!/bin/bash
# Fail when a checksummed triple from the previous lock disappeared.
set -euo pipefail
new="${1:?lock}"
previous="${2:?previous}"

triples() {
  awk '
    function flush() {
      if (name != "" && checksum != "") print name "\t" version "\t" checksum
      name = ""
      version = ""
      checksum = ""
    }
    /^\[\[package\]\]/ { flush(); next }
    /^name = / { name = $3; gsub(/"/, "", name); next }
    /^version = / && version == "" { version = $3; gsub(/"/, "", version); next }
    /^checksum = / { checksum = $3; gsub(/"/, "", checksum); next }
    END { flush() }
  ' "$1" | sort
}

prev="$(mktemp)"
cur="$(mktemp)"
trap 'rm -f "$prev" "$cur"' EXIT
triples "$previous" >"$prev"
triples "$new" >"$cur"
missing="$(comm -23 "$prev" "$cur")"
if [[ -n "$missing" ]]; then
  printf '%s\n' "$missing" | awk -F '\t' '{ printf "drift %s %s %s\n", $1, $2, $3 }'
  exit 2
fi
printf 'lock triples kept %s\n' "$(wc -l <"$prev" | tr -d ' ')"
