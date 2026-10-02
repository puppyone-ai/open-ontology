#!/bin/bash
# Task-local scan harness for the fixed containment-supervisor helper graph.
# Runs inside the pinned linux/arm64 Rust 1.94 image. Does not change the toolchain.
set -u
mkdir -p /work/home /work/meta /work/tool-crates /work/tool-provenance /work/upstream-licenses /work/help /cargo-home /cargo-home/xdg /target /tmp
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=2
export CARGO_TERM_COLOR=never
export HOME=/work/home
export TMPDIR=/tmp
export GIT_TERMINAL_PROMPT=0
export XDG_CACHE_HOME=/cargo-home/xdg
UA='open-ontology-a101-helper-supply (local-audit)'
LOCK=/src/testing/harness/containment-supervisor/Cargo.lock
MANIFEST=/src/testing/harness/containment-supervisor/Cargo.toml
DENY=/src/ci/deny.toml
AUDIT_CKSUM=700c2b240f7fd330c24b675fe429f73a5b676531fcc6300400b2b67f155ba12a
DENY_CKSUM=e528dfcbe739af7ce37a77d3d6df1b29dd6887b1c701d888820c0f16b864f737
ok_to_install=1
msrv_block=0

echo "start $(date -u +%Y-%m-%dT%H:%M:%SZ)" >> /work/console.log

finish() {
  local ec=$?
  echo "trap_exit=$ec $(date -u +%Y-%m-%dT%H:%M:%SZ)" >> /work/console.log
  date -u +%Y-%m-%dT%H:%M:%SZ > /work/scan-done
  exit "$ec"
}
trap finish EXIT

record() {
  local name="$1"
  shift
  echo "BEGIN $name $(date -u +%Y-%m-%dT%H:%M:%SZ)" >> /work/console.log
  echo "CMD $name: $*" >> /work/commands.txt
  if command -v stdbuf >/dev/null 2>&1; then
    stdbuf -oL -eL "$@" >"/work/${name}.out" 2>"/work/${name}.err"
  else
    "$@" >"/work/${name}.out" 2>"/work/${name}.err"
  fi
  local status=$?
  printf '%s\n' "$status" > "/work/${name}.exit"
  echo "phase_exit=$status phase=$name" >> /work/console.log
  return 0
}

flag_ok() {
  grep -F -q -- "$2" "$1"
}

nth_json_string() {
  local key="$1" file="$2" nth="$3"
  local rest val i=0
  rest=$(cat "$file" 2>/dev/null || true)
  while [ "$i" -lt "$nth" ]; do
    case "$rest" in
      *"\"${key}\":\""*)
        rest=${rest#*\"${key}\":\"}
        i=$((i + 1))
        val=${rest%%\"*}
        ;;
      *)
        printf '%s' ""
        return 1
        ;;
    esac
  done
  printf '%s' "$val"
}

yanked_field() {
  local rest
  rest=$(cat "$1" 2>/dev/null || true)
  case "$rest" in
    *'"yanked":false'*) printf '%s' false ;;
    *'"yanked":true'*) printf '%s' true ;;
    *'"yanked": false'*) printf '%s' false ;;
    *'"yanked": true'*) printf '%s' true ;;
    *) printf '%s' unparsed ;;
  esac
}

version_gt() {
  local top
  top=$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n 1)
  [ "$top" = "$1" ] && [ "$1" != "$2" ]
}

{
  echo "date=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "airgap=false"
  echo "network=bridge_nat official crates.io and github.com/rustsec/advisory-db"
  uname -a
  cat /etc/os-release
  echo "=== id ==="
  id
  echo "=== rustc -vV ==="
  rustc -vV
  echo "=== cargo -V ==="
  cargo -V
  echo "=== rustup show ==="
  rustup show
  echo "=== rust-toolchain.toml ==="
  cat /src/rust-toolchain.toml
  echo "=== caps ==="
  grep -E '^Cap' /proc/1/status || true
  echo "=== lock ==="
  sha256sum "$LOCK"
  echo "=== deny ==="
  sha256sum "$DENY"
  echo "=== image cargo config ==="
  if [ -f /usr/local/cargo/config.toml ]; then
    sha256sum /usr/local/cargo/config.toml
    if grep -Eqi 'token|password|secret|authorization|bearer' /usr/local/cargo/config.toml; then
      echo "image_cargo_config_sensitive_marker=1 content_not_copied"
    else
      cp /usr/local/cargo/config.toml /work/image-cargo-config.toml
      echo "image_cargo_config_copied=1"
    fi
  else
    echo "image_cargo_config_absent"
  fi
  echo "=== git ==="
  command -v git
  git --version
  echo "=== curl ==="
  command -v curl
  curl --version | head -n 1
} > /work/identity.log 2>&1

rv=$(rustc --version 2>/work/rustc-version.err || true)
printf '%s\n' "$rv" > /work/rustc-version.txt
case "$rv" in
  "rustc 1.94.0 "*)
    printf '0\n' > /work/identity.exit
    ;;
  *)
    printf '3\n' > /work/identity.exit
    echo "TOOLCHAIN_REFUSED $rv" >> /work/identity.log
    echo "TOOLCHAIN_REFUSED" > /work/unverified.txt
    exit 3
    ;;
esac

record helper-fetch cargo fetch --locked --manifest-path "$MANIFEST"
record helper-metadata cargo metadata --format-version 1 --locked --manifest-path "$MANIFEST"
record helper-tree cargo tree --locked --manifest-path "$MANIFEST"
record helper-tree-features cargo tree --locked --manifest-path "$MANIFEST" -e features

mkdir -p /work/meta
fetch_json() {
  local name="$1" url="$2"
  local out="/work/meta/${name}.json"
  local code
  code=$(curl -sS -A "$UA" -o "$out" -w '%{http_code}' "$url" || echo curl_fail)
  printf '%s\n' "$code" > "/work/meta/${name}.http"
  echo "meta $name http=$code" >> /work/console.log
  sleep 1
}

fetch_json version-nix "https://crates.io/api/v1/crates/nix/0.31.3"
fetch_json version-libc "https://crates.io/api/v1/crates/libc/0.2.189"
fetch_json version-bitflags "https://crates.io/api/v1/crates/bitflags/2.13.2"
fetch_json version-cfg-if "https://crates.io/api/v1/crates/cfg-if/1.0.5"
fetch_json version-cfg_aliases "https://crates.io/api/v1/crates/cfg_aliases/0.2.2"
fetch_json version-cargo-audit "https://crates.io/api/v1/crates/cargo-audit/0.22.2"
fetch_json version-cargo-deny "https://crates.io/api/v1/crates/cargo-deny/0.20.2"
fetch_json crate-nix "https://crates.io/api/v1/crates/nix"
fetch_json crate-libc "https://crates.io/api/v1/crates/libc"
fetch_json crate-bitflags "https://crates.io/api/v1/crates/bitflags"
fetch_json crate-cfg-if "https://crates.io/api/v1/crates/cfg-if"
fetch_json crate-cfg_aliases "https://crates.io/api/v1/crates/cfg_aliases"

now_s=$(date -u +%s)
now_iso=$(date -u +%Y-%m-%dT%H:%M:%SZ)
{
  echo "as_of=$now_iso"
  echo "as_of_unix=$now_s"
  echo "floor_seconds=604800"
  echo "parser=version_endpoint_first_created_at"
  echo "crate_object_created_at_is_not_the_age_input=1"
} > /work/age.txt

age_one() {
  local spec="$1" expect_ck="$2"
  local file="/work/meta/version-${spec}.json"
  local http
  http=$(cat "/work/meta/version-${spec}.http" 2>/dev/null || echo missing)
  local created second yanked cksum
  created=$(nth_json_string created_at "$file" 1 || true)
  second=$(nth_json_string created_at "$file" 2 || true)
  yanked=$(yanked_field "$file" || true)
  cksum=$(nth_json_string checksum "$file" 1 || true)
  local base created_s elapsed age_ok reason
  base=${created%%Z*}
  base=${base%%.*}
  created_s=$(date -u -d "${base}Z" +%s 2>/dev/null || echo 0)
  elapsed=$((now_s - created_s))
  age_ok=0
  reason=ok
  case "$created" in
    2026-*) ;;
    *) reason=version_created_at_not_2026_refused_stale_parser; age_ok=0 ;;
  esac
  if [ "$http" != "200" ]; then
    reason=http_not_200
  elif [ "$cksum" != "$expect_ck" ]; then
    reason=checksum_mismatch
  elif [ "$yanked" != "false" ]; then
    reason=yanked_or_unparsed
  elif [ "$created" = "$second" ]; then
    reason=parser_collapsed_onto_second_created_at
  elif [ "$reason" = "ok" ]; then
    if [ $((elapsed - 1)) -ge 604800 ]; then
      age_ok=1
    else
      reason=younger_than_168h
    fi
  fi
  printf 'spec=%s http=%s version_created_at=%s second_created_at=%s yanked=%s checksum=%s elapsed_trunc_seconds=%s fraction_guard=subtract_1s age_ok=%s reason=%s\n' \
    "$spec" "$http" "$created" "$second" "$yanked" "$cksum" "$elapsed" "$age_ok" "$reason" >> /work/age.txt
}

age_one nix cf20d2fde8ff38632c426f1165ed7436270b44f199fc55284c38276f9db47c3d
age_one libc 3eaf3ede3fee6db1a4c2ee091bf8a8b4dccdc6d17f656fb07896ee72867612f2
age_one bitflags 3ded4057c258ba199e2d26386d3af3780957ecaee6c4ef4041c6b4b8b97c0b06
age_one cfg-if 4e7648175b45a9a48536d676f68d918270699102aa8dab5496df06904c914600
age_one cfg_aliases f079e83a288787bcd14a6aea84cee5c87a67c5a3e660c30f557a3d24761b3527

{
  echo "# crate object created_at is recorded so it is not reused as the version age"
  for spec in nix libc bitflags cfg-if cfg_aliases; do
    cfile="/work/meta/crate-${spec}.json"
    if [ -f "$cfile" ]; then
      ccreated=$(nth_json_string created_at "$cfile" 1 || true)
      echo "spec=$spec crate_object_created_at_not_used=$ccreated http=$(cat /work/meta/crate-${spec}.http)"
    fi
  done
} >> /work/age.txt

{
  echo "# lock checksum versus downloaded crate bytes"
  awk '
    $1=="name" && $2=="=" { name=$3; gsub(/"/,"",name); next }
    $1=="version" && $2=="=" { ver=$3; gsub(/"/,"",ver); next }
    $1=="checksum" && $2=="=" { ck=$3; gsub(/"/,"",ck); if (name!="") print name, ver, ck; name="" }
  ' "$LOCK"
} > /work/lock-packages.txt

while read -r name ver ck; do
  [ -n "$name" ] || continue
  crate=$(find /cargo-home/registry/cache -name "${name}-${ver}.crate" -print -quit 2>/dev/null || true)
  if [ -z "$crate" ]; then
    echo "MISSING_CRATE $name $ver" >> /work/crate-checksums.txt
    continue
  fi
  got=$(sha256sum "$crate" | awk '{print $1}')
  if [ "$got" = "$ck" ]; then
    match=1
  else
    match=0
  fi
  echo "$name $ver lock=$ck file=$got match=$match path=$crate" >> /work/crate-checksums.txt
  listing="/work/upstream-licenses/${name}-${ver}.listing.txt"
  tar -tzf "$crate" > "$listing"
  mkdir -p "/work/upstream-licenses/${name}-${ver}"
  while IFS= read -r entry; do
    base=${entry#${name}-${ver}/}
    case "$base" in
      LICENSE*|LICENCE*|COPYING*|NOTICE*|AUTHORS*|*/LICENSE*|*/LICENCE*|*/COPYING*|*/NOTICE*|*/AUTHORS*)
        dest="/work/upstream-licenses/${name}-${ver}/$(printf '%s' "$base" | tr '/' '_')"
        tar -xOf "$crate" "$entry" > "$dest" 2>/dev/null || echo "EXTRACT_FAIL $entry" >> /work/crate-checksums.txt
        ;;
    esac
  done < "$listing"
  toml_dest="/work/upstream-licenses/${name}-${ver}/Cargo.toml.head"
  tar -xOf "$crate" "${name}-${ver}/Cargo.toml" 2>/dev/null | awk 'NR<=80 { print }' > "$toml_dest" || true
done < /work/lock-packages.txt

echo "=== provenance license hashes ===" > /work/license-hashes.txt
sha256sum /src/testing/harness/containment-supervisor/provenance/licenses/* >> /work/license-hashes.txt || true
echo "=== upstream extracted hashes ===" >> /work/license-hashes.txt
find /work/upstream-licenses -type f -print | sort | while IFS= read -r p; do
  sha256sum "$p"
done >> /work/license-hashes.txt

download_tool() {
  local name="$1" ver="$2" expect="$3"
  local url="https://static.crates.io/crates/${name}/${name}-${ver}.crate"
  local out="/work/tool-crates/${name}-${ver}.crate"
  local code
  code=$(curl -sS -A "$UA" -o "$out" -w '%{http_code}' "$url" || echo curl_fail)
  printf '%s\n' "$code" > "/work/tool-crates/${name}-${ver}.http"
  if [ "$code" != "200" ] || [ ! -s "$out" ]; then
    echo "PREDOWNLOAD_HTTP $name $code" >> /work/tool-pin.txt
    rm -f "$out"
    return 0
  fi
  local got
  got=$(sha256sum "$out" | awk '{print $1}')
  if [ "$got" = "$expect" ]; then
    echo "PIN_MATCH $name $ver $got" >> /work/tool-pin.txt
  else
    echo "PIN_MISMATCH $name $ver got=$got expect=$expect" >> /work/tool-pin.txt
    ok_to_install=0
  fi
  local rvline
  rvline=$(tar -xOf "$out" "${name}-${ver}/Cargo.toml" | awk -F'"' '/^rust-version/ { print $2; exit }')
  echo "MSRV_FIELD $name rust-version=${rvline:-absent}" >> /work/tool-pin.txt
  if [ -n "$rvline" ] && version_gt "$rvline" "1.94.0"; then
    echo "MSRV_BLOCKS $name $rvline > 1.94.0" >> /work/tool-pin.txt
    msrv_block=1
  fi
  mkdir -p "/work/tool-provenance/${name}-${ver}"
  tar -xOf "$out" "${name}-${ver}/Cargo.lock" > "/work/tool-provenance/${name}-${ver}/Cargo.lock" 2>/dev/null || echo "NO_LOCK $name" >> /work/tool-pin.txt
  tar -xOf "$out" "${name}-${ver}/Cargo.toml" > "/work/tool-provenance/${name}-${ver}/Cargo.toml"
  tar -tzf "$out" | grep -Ei '(^|/)(LICENSE|LICENCE|COPYING|NOTICE|AUTHORS)([^/]*)$' > "/work/tool-provenance/${name}-${ver}.license-names.txt" || true
  while IFS= read -r entry; do
    [ -n "$entry" ] || continue
    base=$(basename "$entry")
    tar -xOf "$out" "$entry" > "/work/tool-provenance/${name}-${ver}/${base}"
  done < "/work/tool-provenance/${name}-${ver}.license-names.txt"
}

download_tool cargo-audit 0.22.2 "$AUDIT_CKSUM"
download_tool cargo-deny 0.20.2 "$DENY_CKSUM"

echo "=== harness license pin hashes ===" >> /work/license-hashes.txt
sha256sum /src/supply-chain/licenses/harness/* >> /work/license-hashes.txt || true
echo "=== tool crate license hashes ===" >> /work/license-hashes.txt
find /work/tool-provenance -type f -name 'LICENSE*' -print | sort | while IFS= read -r p; do
  sha256sum "$p"
done >> /work/license-hashes.txt

{
  echo "time=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "ok_to_install=$ok_to_install"
  echo "msrv_block=$msrv_block"
  echo "helper_fetch_exit=$(cat /work/helper-fetch.exit 2>/dev/null || echo missing)"
} > /work/fetch-done

if [ "$msrv_block" = "1" ]; then
  echo "MSRV requires newer than Rust 1.94. Toolchain was not changed. Scans not run." > /work/unverified.txt
  exit 4
fi
if [ "$ok_to_install" != "1" ]; then
  echo "Tool crate checksum did not match the audited pin. Install skipped. Scans not run." > /work/unverified.txt
  exit 5
fi

echo "WAITING_FOR_BUILD_GO $(date -u +%Y-%m-%dT%H:%M:%SZ)" >> /work/console.log
got_go=0
for _ in $(seq 1 120); do
  if [ -f /work/build-go ]; then
    got_go=1
    break
  fi
  sleep 10
done
if [ "$got_go" != "1" ]; then
  echo "BUILD_GO_TIMEOUT builder slot was not released to this task" > /work/unverified.txt
  exit 2
fi

rv=$(rustc --version 2>>/work/rustc-version.err || true)
printf '%s\n' "$rv" > /work/rustc-version-before-install.txt
case "$rv" in
  "rustc 1.94.0 "*) ;;
  *)
    echo "TOOLCHAIN_CHANGED_BEFORE_INSTALL $rv" > /work/unverified.txt
    exit 3
    ;;
esac

record install-cargo-audit cargo install cargo-audit --version 0.22.2 --locked --force
record install-cargo-deny cargo install cargo-deny --version 0.20.2 --locked --force

verify_installed_crate() {
  local name="$1" ver="$2" expect="$3"
  local crate got
  crate=$(find /cargo-home/registry/cache -name "${name}-${ver}.crate" -print -quit 2>/dev/null || true)
  if [ -z "$crate" ]; then
    echo "INSTALLED_CRATE_MISSING $name" >> /work/tool-pin.txt
    return 1
  fi
  got=$(sha256sum "$crate" | awk '{print $1}')
  echo "INSTALLED_CRATE $name $got path=$crate" >> /work/tool-pin.txt
  [ "$got" = "$expect" ]
}

audit_pin_ok=0
deny_pin_ok=0
if verify_installed_crate cargo-audit 0.22.2 "$AUDIT_CKSUM"; then
  audit_pin_ok=1
fi
if verify_installed_crate cargo-deny 0.20.2 "$DENY_CKSUM"; then
  deny_pin_ok=1
fi

if [ -x /cargo-home/bin/cargo-audit ]; then
  record version-audit /cargo-home/bin/cargo-audit --version
  record help-audit /cargo-home/bin/cargo-audit --help
  record help-audit-sub /cargo-home/bin/cargo-audit audit --help
  od -An -t u1 -N 20 /cargo-home/bin/cargo-audit > /work/audit-elf-header.txt
  sha256sum /cargo-home/bin/cargo-audit > /work/audit-binary.sha256
else
  echo "cargo-audit binary absent" >> /work/tool-pin.txt
fi
if [ -x /cargo-home/bin/cargo-deny ]; then
  record version-deny /cargo-home/bin/cargo-deny --version
  record help-deny /cargo-home/bin/cargo-deny --help
  record help-deny-check /cargo-home/bin/cargo-deny check --help
  od -An -t u1 -N 20 /cargo-home/bin/cargo-deny > /work/deny-elf-header.txt
  sha256sum /cargo-home/bin/cargo-deny > /work/deny-binary.sha256
else
  echo "cargo-deny binary absent" >> /work/tool-pin.txt
fi

run_audit=0
if [ "$audit_pin_ok" = "1" ] && [ -x /cargo-home/bin/cargo-audit ]; then
  if flag_ok /work/help-audit-sub.out "--json" && flag_ok /work/help-audit-sub.out "--file"; then
    run_audit=1
  elif flag_ok /work/help-audit.out "--json" && flag_ok /work/help-audit.out "--file"; then
    run_audit=2
  else
    echo "AUDIT_HELP_MISSING_JSON_OR_FILE" > /work/scan-audit.note
    printf '127\n' > /work/scan-audit.exit
  fi
else
  echo "AUDIT_NOT_RUN pin_ok=$audit_pin_ok" > /work/scan-audit.note
  printf '127\n' > /work/scan-audit.exit
fi

if [ "$run_audit" = "1" ]; then
  record scan-audit /cargo-home/bin/cargo-audit audit --json --file "$LOCK"
elif [ "$run_audit" = "2" ]; then
  record scan-audit /cargo-home/bin/cargo-audit --json --file "$LOCK"
fi

record_advisory_rev() {
  local label="$1"
  {
    echo "label=$label"
    echo "time=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    find /cargo-home /work/home -name 'RUSTSEC-2021-0119.md' -print 2>/dev/null || true
    find /cargo-home /work/home -type d -name .git -print 2>/dev/null || true
  } > "/work/advisory-search-${label}.txt"
  local repo=""
  repo=$(find /cargo-home /work/home -type d -name advisory-db -print -quit 2>/dev/null || true)
  if [ -z "$repo" ]; then
    local gitdir
    gitdir=$(find /cargo-home /work/home -type d -name .git -print -quit 2>/dev/null || true)
    if [ -n "$gitdir" ]; then
      repo=$(dirname "$gitdir")
    fi
  fi
  if [ -z "$repo" ] || [ ! -d "$repo/.git" ]; then
    echo "ADVISORY_DB_NOT_FOUND label=$label" >> /work/advisory-rev.txt
    return 0
  fi
  {
    echo "label=$label"
    echo "repo=$repo"
    echo "time=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    git -C "$repo" rev-parse HEAD
    git -C "$repo" log -1 --format='%H%n%cI%n%s'
    git -C "$repo" status --porcelain=v1 | awk 'BEGIN{c=0} {c++} END{print "porcelain_count=" c}'
    git -C "$repo" remote -v
  } >> /work/advisory-rev.txt
  if [ -f "$repo/crates/nix/RUSTSEC-2021-0119.md" ]; then
    cp "$repo/crates/nix/RUSTSEC-2021-0119.md" "/work/RUSTSEC-2021-0119.${label}.md"
    sha256sum "/work/RUSTSEC-2021-0119.${label}.md" >> /work/advisory-rev.txt
  else
    echo "RUSTSEC-2021-0119.md absent in $repo" >> /work/advisory-rev.txt
  fi
}

record_advisory_rev after-audit

run_deny=0
if [ "$deny_pin_ok" = "1" ] && [ -x /cargo-home/bin/cargo-deny ]; then
  if flag_ok /work/help-deny.out "--format" && flag_ok /work/help-deny.out "--config" && flag_ok /work/help-deny.out "--locked" && flag_ok /work/help-deny.out "--manifest-path"; then
    run_deny=1
  else
    echo "DENY_HELP_MISSING_REQUIRED_FLAG" > /work/scan-deny.note
    printf '127\n' > /work/scan-deny.exit
  fi
else
  echo "DENY_NOT_RUN pin_ok=$deny_pin_ok" > /work/scan-deny.note
  printf '127\n' > /work/scan-deny.exit
fi

if [ "$run_deny" = "1" ]; then
  record scan-deny /cargo-home/bin/cargo-deny --format json --config "$DENY" --locked --manifest-path "$MANIFEST" check
  if flag_ok /work/help-deny-check.out "advisories"; then
    record scan-deny-advisories /cargo-home/bin/cargo-deny --format json --config "$DENY" --locked --manifest-path "$MANIFEST" check advisories
  else
    echo "no advisories check name in help" > /work/scan-deny-advisories.note
  fi
  if flag_ok /work/help-deny-check.out "licenses"; then
    record scan-deny-licenses /cargo-home/bin/cargo-deny --format json --config "$DENY" --locked --manifest-path "$MANIFEST" check licenses
  else
    echo "no licenses check name in help" > /work/scan-deny-licenses.note
  fi
  if flag_ok /work/help-deny-check.out "bans"; then
    record scan-deny-bans /cargo-home/bin/cargo-deny --format json --config "$DENY" --locked --manifest-path "$MANIFEST" check bans
  else
    echo "no bans check name in help" > /work/scan-deny-bans.note
  fi
  if flag_ok /work/help-deny-check.out "sources"; then
    record scan-deny-sources /cargo-home/bin/cargo-deny --format json --config "$DENY" --locked --manifest-path "$MANIFEST" check sources
  else
    echo "no sources check name in help" > /work/scan-deny-sources.note
  fi
fi

record_advisory_rev after-deny

rv=$(rustc --version 2>>/work/rustc-version.err || true)
printf '%s\n' "$rv" > /work/rustc-version-after-scan.txt

if command -v ps >/dev/null 2>&1; then
  ps -eo pid,ppid,etime,comm > /work/container-processes.txt 2>/work/container-processes.err || true
fi

{
  echo "identity_exit=$(cat /work/identity.exit 2>/dev/null || echo missing)"
  echo "helper_fetch_exit=$(cat /work/helper-fetch.exit 2>/dev/null || echo missing)"
  echo "helper_metadata_exit=$(cat /work/helper-metadata.exit 2>/dev/null || echo missing)"
  echo "helper_tree_exit=$(cat /work/helper-tree.exit 2>/dev/null || echo missing)"
  echo "helper_tree_features_exit=$(cat /work/helper-tree-features.exit 2>/dev/null || echo missing)"
  echo "install_audit_exit=$(cat /work/install-cargo-audit.exit 2>/dev/null || echo missing)"
  echo "install_deny_exit=$(cat /work/install-cargo-deny.exit 2>/dev/null || echo missing)"
  echo "scan_audit_exit=$(cat /work/scan-audit.exit 2>/dev/null || echo missing)"
  echo "scan_deny_exit=$(cat /work/scan-deny.exit 2>/dev/null || echo missing)"
  echo "scan_deny_advisories_exit=$(cat /work/scan-deny-advisories.exit 2>/dev/null || echo missing)"
  echo "scan_deny_licenses_exit=$(cat /work/scan-deny-licenses.exit 2>/dev/null || echo missing)"
  echo "scan_deny_bans_exit=$(cat /work/scan-deny-bans.exit 2>/dev/null || echo missing)"
  echo "scan_deny_sources_exit=$(cat /work/scan-deny-sources.exit 2>/dev/null || echo missing)"
  echo "audit_pin_ok=$audit_pin_ok"
  echo "deny_pin_ok=$deny_pin_ok"
  echo "airgap=false"
  echo "toolchain=1.94.0"
} > /work/summary.env

echo "SCRIPT_COMPLETE $(date -u +%Y-%m-%dT%H:%M:%SZ)" >> /work/console.log
exit 0
