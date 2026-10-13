#!/bin/bash
# Verify Cargo.lock pins without updating the lock. Optional previous lock detects drift.
set -euo pipefail

new="${1:?lock}"
previous="${2:-}"
out="${3:?out}"

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

has_pin() {
  local name="$1" version="$2" checksum="$3" file="$4"
  awk -F '\t' -v name="$name" -v version="$version" -v checksum="$checksum" '
    $1 == name && $2 == version && $3 == checksum { found = 1 }
    END { exit found ? 0 : 1 }
  ' "$file"
}

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
triples "$new" >"$tmp"

failed=0
{
  while IFS=' ' read -r name version checksum; do
    if has_pin "$name" "$version" "$checksum" "$tmp"; then
      printf '%s expect=%s:%s match=true\n' "$name" "$version" "$checksum"
    else
      found="$(awk -F '\t' -v name="$name" '$1 == name { printf "%s:%s,", $2, $3 }' "$tmp")"
      printf '%s expect=%s:%s found=%s match=false\n' "$name" "$version" "$checksum" "${found:-missing}"
      failed=1
    fi
  done <<'PINS'
mongodb 3.3.0 622f272c59e54a3c85f5902c6b8e7b1653a6b6681f45e4c42d6581301119a4b8
mongodb-internal-macros 3.3.0 63981427a0f26b89632fd2574280e069d09fb2912a3138da15de0174d11dd077
serde_json 1.0.151 c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14
serde 1.0.229 4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba
arrow-array 60.0.0 51dff6e4b9f158864a0aeb6a131ad53858ecbae7a2fc8989307b9b3c6b2f122e
arrow-ipc 60.0.0 86c523472d22e31f984df7a44fc36e6323d3c9b05ffe53d267ee4c3aa7c128d2
arrow-schema 60.0.0 f85a9a58cd03560526b0066487574aacb9008e413504fa0ee23d61141e96c338
arrow-buffer 60.0.0 c7235b863533e9db3ab86905b4521251de11226275b436ff3cb2d0010558d1c5
arrow-data 60.0.0 5aa06d5e22786c4966ffc4d2f171d0cb65d95f6bdea108a14e8c335e9faa437f
tokio 1.53.2 e95f91fcc7a621e8b030f6aa23c71fe9838ae2fb4d8118b75602a328f5144044
bson 2.15.0 7969a9ba84b0ff843813e7249eed1678d9b6607ce5a3b8f0a47af3fcf7978e6e
mongocrypt 0.3.2 8da0cd419a51a5fb44819e290fbdb0665a54f21dead8923446a799c7f4d26ad9
mongocrypt-sys 0.1.6+1.18.2 851fac73f7fe22f6a3ab87f720ce509cae7c9fd08e7dd27866cc232dee07ccf4
PINS
  if [[ -n "$previous" ]]; then
    prev="$(mktemp)"
    triples "$previous" >"$prev"
    while IFS=$'\t' read -r name version checksum; do
      if ! has_pin "$name" "$version" "$checksum" "$tmp"; then
        printf 'drift %s %s %s\n' "$name" "$version" "$checksum"
        failed=1
      fi
    done <"$prev"
    rm -f "$prev"
  fi
} >"$out"
cat "$out"
if [[ "$failed" -ne 0 ]]; then
  exit 2
fi
