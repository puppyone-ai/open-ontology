#!/bin/bash
# Network phase. Generates the helper lock, fetches crates, and records supply facts.
# Tests run later with the network disconnected.
set -u
OUT=/out
exec > >(tee -a "$OUT/fetch-console.log") 2>&1
printf 'fetch-start %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"

record() {
  local name="$1"
  shift
  "$@" >"$OUT/${name}.log" 2>&1
  local code=$?
  printf '%s\n' "$code" >"$OUT/${name}.exit"
  printf '%s exit %s\n' "$name" "$code"
  return 0
}

rustc -vV >"$OUT/rustc-version.txt" 2>&1 || true
cargo -V >"$OUT/cargo-version.txt" 2>&1 || true
command -v curl >"$OUT/curl-path.txt" 2>&1 || true
command -v python3 >"$OUT/python3-path.txt" 2>&1 || true
command -v perl >"$OUT/perl-path.txt" 2>&1 || true
command -v git >"$OUT/git-path.txt" 2>&1 || true
printf '%s\n' "$(id)" >"$OUT/id.txt"
grep -E 'Cap(Inh|Prm|Eff|Bnd|Amb)' /proc/self/status >"$OUT/caps.txt" 2>&1 || true
record clippy-component rustup component add clippy --toolchain 1.94.0

record helper-lock cargo generate-lockfile --manifest-path /src/testing/harness/containment-supervisor/Cargo.toml
record helper-fetch cargo fetch --locked --manifest-path /src/testing/harness/containment-supervisor/Cargo.toml
record workspace-fetch cargo fetch --locked --manifest-path /src/Cargo.toml

# Bash + curl. The image is not assumed to have python3, and apt is not used.
supply_fail=0
: >"$OUT/supply-check.log"
rows=/tmp/a082-supply-rows.jsonl
: >"$rows"
lock=/src/testing/harness/containment-supervisor/Cargo.lock
if [ ! -f "$lock" ]; then
  echo 'helper lock missing' >>"$OUT/supply-check.log"
  supply_fail=1
else
  awk '
    /^\[\[package\]\]/ {
      if (name != "" && checksum != "") print name "\t" version "\t" checksum
      name = version = checksum = ""
      next
    }
    /^name = "/ { gsub(/"/, "", $3); name = $3; next }
    /^version = "/ { gsub(/"/, "", $3); version = $3; next }
    /^checksum = "/ { gsub(/"/, "", $3); checksum = $3; next }
    END { if (name != "" && checksum != "") print name "\t" version "\t" checksum }
  ' "$lock" > /tmp/a082-supply-pkgs.tsv
  floor=$(date -u -d '168 hours ago' +%s)
  while IFS="$(printf '\t')" read -r name version checksum; do
    [ -n "$name" ] || continue
    url="https://crates.io/api/v1/crates/${name}/${version}"
    if ! body=$(curl -fsS --max-time 60 -A 'open-ontology-a082 (supply-check)' "$url"); then
      echo "FETCH_FAIL $name $version" >>"$OUT/supply-check.log"
      supply_fail=1
      continue
    fi
    yanked=$(printf '%s' "$body" | sed -n 's/.*"yanked":\(true\|false\).*/\1/p' | head -1)
    created=$(printf '%s' "$body" | sed -n 's/.*"created_at":"\([^"]*\)".*/\1/p' | head -1)
    api_sum=$(printf '%s' "$body" | sed -n 's/.*"checksum":"\([^"]*\)".*/\1/p' | head -1)
    api_num=$(printf '%s' "$body" | sed -n 's/.*"num":"\([^"]*\)".*/\1/p' | head -1)
    license=$(printf '%s' "$body" | sed -n 's/.*"license":"\([^"]*\)".*/\1/p' | head -1)
    msrv=$(printf '%s' "$body" | sed -n 's/.*"rust_version":"\([^"]*\)".*/\1/p' | head -1)
    created_s=$(date -u -d "$created" +%s 2>/dev/null || echo 0)
    age_ok=1
    match=1
    if [ "$yanked" != false ]; then supply_fail=1; fi
    if [ "$api_sum" != "$checksum" ]; then match=0; supply_fail=1; fi
    if [ "$api_num" != "$version" ]; then supply_fail=1; fi
    if [ "$created_s" -gt "$floor" ] || [ "$created_s" -eq 0 ]; then age_ok=0; supply_fail=1; fi
    if [ "$name" = bitflags ] && [ "$version" = 2.12.0 ]; then supply_fail=1; fi
    if [ "$name" = cfg-if ] && [ "$version" = 1.0.2 ]; then supply_fail=1; fi
    printf '%s\n' "{\"name\":\"$name\",\"version\":\"$version\",\"checksum\":\"$checksum\",\"checksum_match\":$match,\"yanked\":\"$yanked\",\"created_at\":\"$created\",\"age_ok\":$age_ok,\"license\":\"$license\",\"rust_version\":\"$msrv\"}" | tee -a "$rows" >>"$OUT/supply-check.log"
  done < /tmp/a082-supply-pkgs.tsv
fi
if [ -s "$rows" ]; then
  {
    printf '[\n'
    sed '$!s/$/,/' "$rows"
    printf ']\n'
  } >"$OUT/supply-resolved.json"
else
  printf '[]\n' >"$OUT/supply-resolved.json"
fi
printf '%s\n' "$supply_fail" >"$OUT/supply-check.exit"
printf 'supply-check exit %s\n' "$supply_fail"

record advisory-nix curl -fsS -A 'open-ontology-a082 (supply-check)' -H 'Accept: application/vnd.github+json' https://api.github.com/repos/rustsec/advisory-db/contents/crates/nix
for crate in libc bitflags cfg-if cfg_aliases; do
  record "advisory-${crate}" curl -fsS -A 'open-ontology-a082 (supply-check)' -H 'Accept: application/vnd.github+json' "https://api.github.com/repos/rustsec/advisory-db/contents/crates/${crate}"
done

printf 'license-copy=deferred-to-offline-test\n' >"$OUT/license-files.txt"
printf 'fetch-end %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
exit 0
