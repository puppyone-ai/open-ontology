#!/bin/bash
# Checksum-matched crate bodies and crates.io publication times. Not a cargo build.
set -euo pipefail
umask 022
EV=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a110/SP03/lossless-types-review
LOCK="$EV/snapshot/Cargo.lock"
OUT="$EV/licenses"
ADOPT=2026-10-02T13:30:04Z
mkdir -p "$OUT/crates" "$OUT/bodies"
adopt_epoch=$(date -u -j -f '%Y-%m-%dT%H:%M:%SZ' "$ADOPT" +%s)
{
  echo "adopt_commit_utc=$ADOPT"
  echo "adopt_epoch=$adopt_epoch"
  echo "observed_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "threshold_seconds=604800"
} > "$OUT/provenance-header.txt"

checksum_of() {
  name=$1
  ver=$2
  awk -v name="$name" -v ver="$ver" '
    $1 == "name" && $3 == "\"" name "\"" { want = 1; version = ""; checksum = "" }
    want && $1 == "version" { gsub(/"/, "", $3); version = $3 }
    want && $1 == "checksum" { gsub(/"/, "", $3); checksum = $3 }
    want && version == ver && checksum != "" { print checksum; exit }
  ' "$LOCK"
}

: > "$OUT/provenance.tsv"
while read -r name ver; do
  [ -n "$name" ] || continue
  sum=$(checksum_of "$name" "$ver")
  file="$OUT/crates/${name}-${ver}.crate"
  curl -fsSL --retry 3 -o "$file" "https://static.crates.io/crates/${name}/${name}-${ver}.crate"
  got=$(shasum -a 256 "$file" | awk '{print $1}')
  if [ "$sum" = "$got" ]; then
    match=yes
  else
    match=no
  fi
  dest="$OUT/bodies/${name}-${ver}"
  rm -rf "$dest"
  mkdir -p "$dest"
  tar -xzf "$file" -C "$dest"
  api="$OUT/bodies/${name}-${ver}.api.json"
  curl -fsSL --retry 3 -A "oo-a110-license-review" -o "$api" "https://crates.io/api/v1/crates/${name}/${ver}"
  created=$(sed -n 's/.*"created_at":"\([^"]*\)".*/\1/p' "$api" | head -1)
  yanked=$(sed -n 's/.*"yanked":\(true\|false\).*/\1/p' "$api" | head -1)
  rust_field=$(sed -n 's/.*"rust_version":\("\([^"]*\)"\|null\).*/\1/p' "$api" | head -1)
  license_field=$(sed -n 's/.*"license":"\([^"]*\)".*/\1/p' "$api" | head -1)
  base=$(printf '%s\n' "$created" | sed -E 's/\.[0-9]+Z$/Z/')
  created_epoch=$(date -u -j -f '%Y-%m-%dT%H:%M:%SZ' "$base" +%s)
  elapsed=$((adopt_epoch - created_epoch))
  if [ "$elapsed" -ge 604800 ]; then
    age=yes
  else
    age=no
  fi
  crate_root=$(find "$dest" -mindepth 1 -maxdepth 1 -type d | head -1)
  msrv=$(awk -F'"' '/^rust-version/ { print $2; exit }' "$crate_root/Cargo.toml" || true)
  license_files=$(find "$crate_root" -maxdepth 2 -type f \( -iname 'LICENSE*' -o -iname 'NOTICE*' -o -iname 'COPYING*' -o -iname 'UNLICENSE*' -o -iname 'COPYRIGHT*' \) | sed "s|^$crate_root/||" | sort | paste -sd, -)
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$name" "$ver" "$match" "$sum" "$created" "$elapsed" "$age" "$yanked" "$license_field" "${msrv:-}" "$rust_field" "$license_files" \
    >> "$OUT/provenance.tsv"
  echo "$name $ver match=$match age=$age elapsed=$elapsed created=$created"
done <<'LIST'
android_system_properties 0.1.6
arrow-array 60.0.0
arrow-buffer 60.0.0
arrow-data 60.0.0
arrow-schema 60.0.0
const-random 0.1.18
const-random-macro 0.1.16
crunchy 0.2.4
half 2.7.1
iana-time-zone 0.1.65
iana-time-zone-haiku 0.1.2
libm 0.2.16
num-bigint 0.4.8
num-bigint 0.5.1
tiny-keccak 2.0.2
windows-core 0.62.2
windows-implement 0.60.2
windows-interface 0.59.3
windows-result 0.4.1
windows-strings 0.5.1
LIST
echo license_fetch_done
