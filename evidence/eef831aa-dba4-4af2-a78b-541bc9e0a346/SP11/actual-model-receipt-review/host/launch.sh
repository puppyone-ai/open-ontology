#!/bin/bash
# Host Mach-O relay from the unmodified 8c825d89 archive, then the observer bin
# from a private copy. No cargo test, no cargo update, no host clippy.
# The 28-test Linux proof is reused only when all 21 proof-3 source hashes match.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a346/SP11/actual-model-receipt-review/host
ROOT=/var/tmp/oo-a346-eef831aa-dba4-4af2-a78b-541bc9e0a346/host
FIXED=8c825d8961e93ac44751b6e262cb50ab50afca78
EXPECTED_TREE=8c687ef0a60d8a7c1843c1c2f0c7e6ce9a442a93
PROOF=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a334/SP11/local-llm-integration-repair/linux/proof-3/source-after.sha256
A334=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a334/SP11/local-llm-integration-repair/host-3/launch.sh
OBSERVE_RS=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a346/SP11/actual-model-receipt-review/oracle/observe.rs

mkdir -p "$EVIDENCE" "$ROOT/cargo" "$ROOT/target" "$ROOT/tmp" "$ROOT/bin"
chmod 755 "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite host console.log" >&2
  exit 2
fi

own_docker=$(docker ps -q --filter label=oo.owner=a346 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_docker=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
own_host=0
global_host=0
while read -r pid comm; do
  [ -n "${pid:-}" ] || continue
  global_host=$((global_host + 1))
  cwd=$(lsof -a -p "$pid" -d cwd -Fn 2>/dev/null | awk '/^n/ {print substr($0,2); exit}' || true)
  case "$cwd" in
    *oo-a346*) own_host=$((own_host + 1)) ;;
  esac
done <<EOF
$(ps -axo pid=,comm= | awk '$2=="cargo" || $2=="rustc" || $2=="rustdoc" || $2=="rustup" {print $1, $2}')
EOF
own=$((own_docker + own_host))
global=$((global_docker + global_host))
printf 'own=%s global=%s own_docker=%s own_host=%s global_docker=%s global_host=%s cap_global=10 cap_own=1\n' \
  "$own" "$global" "$own_docker" "$own_host" "$global_docker" "$global_host" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global" -ge 10 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/build.exit"
  exit 3
fi

{
  echo "a334 host-3 launch ${A334} $(shasum -a 256 "$A334" | awk '{print $1}')"
  echo "fixed=${FIXED}"
  echo "changes archive-only private cargo target jobs=1 locked no-update no-test no-host-clippy observer-separate-target"
} > "$EVIDENCE/recipe-origins.txt"

cleared=no
for name in HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY http_proxy https_proxy all_proxy no_proxy OPENAI_API_KEY OPENAI_BASE_URL OPENAI_API_BASE; do
  if eval "[ -n \"\${${name}+x}\" ]"; then
    unset "$name"
    cleared=yes
  fi
done
printf 'proxy_or_openai_env_cleared=%s\n' "$cleared" > "$EVIDENCE/env-cleared.txt"

export CARGO_HOME="$ROOT/cargo"
export CARGO_TARGET_DIR="$ROOT/target"
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export TMPDIR="$ROOT/tmp"
unset RUSTUP_TOOLCHAIN
{
  echo "host_toolchain=rustc-1.98.1-homebrew"
  echo "rustup_1.94=absent"
  echo "linux_smoke_toolchain=1.94.0"
  rustc -vV
} > "$EVIDENCE/rustc-version.txt"

ARCHIVE=$ROOT/relay-src
rm -rf "$ARCHIVE"
mkdir -p "$ARCHIVE"
git -C "$SRC" archive "$FIXED" testing/real-db/local-llm | tar -x -C "$ARCHIVE" --strip-components=3
test -f "$ARCHIVE/Cargo.toml"
test -f "$ARCHIVE/Cargo.lock"
test -f "$ARCHIVE/src/main.rs"
if [ -f "$ARCHIVE/rust-toolchain.toml" ]; then
  echo "relay archive contained rust-toolchain.toml" > "$EVIDENCE/toolchain-note.txt"
  printf '%s\n' 1 > "$EVIDENCE/build.exit"
  exit 1
fi
printf '%s\n' "relay archive has no rust-toolchain.toml" > "$EVIDENCE/toolchain-note.txt"
git -C "$SRC" rev-parse "${FIXED}:testing/real-db/local-llm" > "$EVIDENCE/relay-tree.txt"
grep -q "^${EXPECTED_TREE}$" "$EVIDENCE/relay-tree.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
shasum -a 256 "$PROOF" > "$EVIDENCE/proof3-listing.sha256"

match=0
mismatch=0
: > "$EVIDENCE/relay-source-mismatch.txt"
while read -r sum path; do
  [ -n "${sum:-}" ] || continue
  [ -n "${path:-}" ] || continue
  if [ ! -f "$ARCHIVE/$path" ]; then
    printf 'missing %s\n' "$path" >> "$EVIDENCE/relay-source-mismatch.txt"
    mismatch=$((mismatch + 1))
    continue
  fi
  got=$(shasum -a 256 "$ARCHIVE/$path" | awk '{print $1}')
  if [ "$got" = "$sum" ]; then
    match=$((match + 1))
  else
    printf 'mismatch %s\n' "$path" >> "$EVIDENCE/relay-source-mismatch.txt"
    mismatch=$((mismatch + 1))
  fi
done < "$PROOF"
archive_files=$(find "$ARCHIVE" -type f | wc -l | tr -d ' ')
printf 'match=%s mismatch=%s archive_files=%s expected=21 relay_tests_rerun=no host_clippy=no\n' \
  "$match" "$mismatch" "$archive_files" > "$EVIDENCE/relay-source-match.txt"
if [ "$mismatch" -ne 0 ] || [ "$match" -ne 21 ] || [ "$archive_files" -ne 21 ]; then
  echo "relay source does not match the proof-3 listing" >&2
  printf '%s\n' 1 > "$EVIDENCE/build.exit"
  exit 1
fi

set +e
cargo build --manifest-path "$ARCHIVE/Cargo.toml" --locked --offline --bins > "$EVIDENCE/offline-build.log" 2>&1
code=$?
printf '%s\n' "$code" > "$EVIDENCE/offline-build.exit"
if [ "$code" -ne 0 ]; then
  cargo fetch --manifest-path "$ARCHIVE/Cargo.toml" --locked > "$EVIDENCE/fetch.log" 2>&1
  fetch_code=$?
  printf '%s\n' "$fetch_code" > "$EVIDENCE/fetch.exit"
  if [ "$fetch_code" -eq 0 ]; then
    cargo build --manifest-path "$ARCHIVE/Cargo.toml" --locked --offline --bins > "$EVIDENCE/console.log" 2>&1
    code=$?
  else
    cp "$EVIDENCE/offline-build.log" "$EVIDENCE/console.log"
  fi
else
  printf '%s\n' 0 > "$EVIDENCE/fetch.exit"
  cp "$EVIDENCE/offline-build.log" "$EVIDENCE/console.log"
fi
printf '%s\n' "$code" > "$EVIDENCE/relay-build.exit"
if [ "$code" -ne 0 ]; then
  printf '%s\n' "$code" > "$EVIDENCE/build.exit"
  exit "$code"
fi
set -e
test -x "$ROOT/target/debug/oo-local-llm-relay"
test -x "$ROOT/target/debug/oo-local-llm-models"
cp "$ROOT/target/debug/oo-local-llm-relay" "$ROOT/bin/oo-local-llm-relay"
cp "$ROOT/target/debug/oo-local-llm-models" "$ROOT/bin/oo-local-llm-models"
chmod 755 "$ROOT/bin/oo-local-llm-relay" "$ROOT/bin/oo-local-llm-models"
shasum -a 256 "$ROOT/bin/oo-local-llm-relay" "$ROOT/bin/oo-local-llm-models" > "$EVIDENCE/relay-bin.sha256"
file "$ROOT/bin/oo-local-llm-relay" "$ROOT/bin/oo-local-llm-models" > "$EVIDENCE/relay-bin.file.txt"
if grep -q 'a346-observe' "$ARCHIVE/Cargo.toml"; then
  echo "unmodified archive gained the observer bin" >&2
  printf '%s\n' 1 > "$EVIDENCE/build.exit"
  exit 1
fi

OBSERVE=$ROOT/observe-src
rm -rf "$OBSERVE"
mkdir -p "$OBSERVE"
cp -a "$ARCHIVE"/. "$OBSERVE"/
mkdir -p "$OBSERVE/src/bin"
cp "$OBSERVE_RS" "$OBSERVE/src/bin/observe.rs"
printf '\n[[bin]]\nname = "a346-observe"\npath = "src/bin/observe.rs"\n' >> "$OBSERVE/Cargo.toml"
if grep -q 'a346-observe' "$ARCHIVE/Cargo.toml"; then
  echo "observer edit leaked into the relay archive" >&2
  printf '%s\n' 1 > "$EVIDENCE/build.exit"
  exit 1
fi
export CARGO_TARGET_DIR="$ROOT/target"
set +e
cargo build --manifest-path "$OBSERVE/Cargo.toml" --locked --offline --bin a346-observe > "$EVIDENCE/observe-build.log" 2>&1
observe_code=$?
set -e
printf '%s\n' "$observe_code" > "$EVIDENCE/observe-build.exit"
if [ "$observe_code" -ne 0 ]; then
  printf '%s\n' "$observe_code" > "$EVIDENCE/build.exit"
  exit "$observe_code"
fi
cp "$ROOT/target/debug/a346-observe" "$ROOT/bin/a346-observe"
chmod 755 "$ROOT/bin/a346-observe"
shasum -a 256 "$ROOT/bin/a346-observe" > "$EVIDENCE/observe-bin.sha256"
file "$ROOT/bin/a346-observe" > "$EVIDENCE/observe-bin.file.txt"
printf '%s\n' 0 > "$EVIDENCE/build.exit"
exit 0
