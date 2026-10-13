#!/bin/bash
# Compare /src/Cargo.lock with the pinned baseline. New packages are recorded.
# Drift exits 2. This script does not update a lock.
set -euo pipefail
name=""
ver=""
chk=""
: > /evidence/new-packages.tsv
: > /evidence/drift.tsv
flush() {
  if [[ -z "$name" || -z "$ver" ]]; then
    return
  fi
  local known
  known="$(awk -F '\t' -v n="$name" -v v="$ver" '$1==n && $2==v {print $3; exit}' /baseline/baseline-checksums.tsv || true)"
  if [[ -n "$known" ]]; then
    if [[ -n "$chk" && "$known" != "$chk" ]]; then
      printf 'checksum\t%s\t%s\t%s\t%s\n' "$name" "$ver" "$chk" "$known" >> /evidence/drift.tsv
    fi
  else
    local allowed
    allowed="$(awk -F '\t' -v n="$name" '$1==n {print $2; exit}' /baseline/baseline-names.tsv || true)"
    if [[ -n "$allowed" ]]; then
      case ",$allowed," in
        *,"$ver",*) ;;
        *) printf 'version\t%s\t%s\tallowed=%s\n' "$name" "$ver" "$allowed" >> /evidence/drift.tsv ;;
      esac
    else
      printf '%s\t%s\t%s\n' "$name" "$ver" "$chk" >> /evidence/new-packages.tsv
    fi
  fi
}
while IFS= read -r line; do
  case "$line" in
    '[[package]]') flush; name=""; ver=""; chk="" ;;
    'name = "'*) name="${line#name = \"}"; name="${name%\"}" ;;
    'version = "'*) ver="${line#version = \"}"; ver="${ver%\"}" ;;
    'checksum = "'*) chk="${line#checksum = \"}"; chk="${chk%\"}" ;;
  esac
done < /src/Cargo.lock
flush
echo "new=$(wc -l < /evidence/new-packages.tsv | tr -d ' ') drift=$(wc -l < /evidence/drift.tsv | tr -d ' ')"
if [[ -s /evidence/drift.tsv ]]; then
  exit 2
fi
