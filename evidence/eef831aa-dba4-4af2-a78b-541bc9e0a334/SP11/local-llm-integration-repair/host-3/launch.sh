#!/bin/bash
# Host Mach-O build for the 2026-10-08 readiness request.
# Origin: host-2/launch.sh, which came from a324 host/launch.sh at fixed 63566b60.
# The pinned Linux image cannot emit a Mach-O binary, and the private key stays on the host.
# Changes from host-2: new private directories. Same locked build. No cargo update.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a334/SP11/local-llm-integration-repair/host-3
ROOT=/var/tmp/oo-a334-eef831aa-dba4-4af2-a78b-541bc9e0a334/host-3
CRATE=${SRC}/testing/real-db/local-llm
A324=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a324/SP11/local-llm/host/launch.sh
mkdir -p "$EVIDENCE" "$ROOT/cargo" "$ROOT/target" "$ROOT/tmp"
chmod 755 "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite host-3 console.log" >&2
  exit 2
fi
own=$(docker ps -q --filter label=oo.owner=a334 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/build.exit"
  exit 3
fi
{
  echo "a324 host launch 63566b60aabdf29a29bfe55c4866edf3c4081cfd evidence/eef831aa-dba4-4af2-a78b-541bc9e0a324/SP11/local-llm/host/launch.sh $(shasum -a 256 "$A324" | awk '{print $1}')"
  echo "changes from host-2: host-3 paths private_cargo private_target jobs=1 locked no-update"
} > "$EVIDENCE/recipe-origins.txt"
export CARGO_HOME="$ROOT/cargo"
export CARGO_TARGET_DIR="$ROOT/target"
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export TMPDIR="$ROOT/tmp"
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY http_proxy https_proxy all_proxy NO_PROXY no_proxy
unset OPENAI_API_KEY OPENAI_BASE_URL OPENAI_API_BASE
if rustup toolchain list 2>/dev/null | awk '{print $1}' | grep -qx '1.94.0'; then
  export RUSTUP_TOOLCHAIN=1.94.0
else
  unset RUSTUP_TOOLCHAIN
fi
{
  echo "toolchain=${RUSTUP_TOOLCHAIN:-host-default}"
  rustc -vV
} > "$EVIDENCE/rustc-version.txt"
set +e
cargo build --manifest-path "$CRATE/Cargo.toml" --locked --offline --bins > "$EVIDENCE/offline-build.log" 2>&1
code=$?
printf '%s\n' "$code" > "$EVIDENCE/offline-build.exit"
if [ "$code" -ne 0 ]; then
  cargo fetch --manifest-path "$CRATE/Cargo.toml" --locked > "$EVIDENCE/fetch.log" 2>&1
  fetch_code=$?
  printf '%s\n' "$fetch_code" > "$EVIDENCE/fetch.exit"
  if [ "$fetch_code" -eq 0 ]; then
    cargo build --manifest-path "$CRATE/Cargo.toml" --locked --offline --bins > "$EVIDENCE/console.log" 2>&1
    code=$?
  else
    cp "$EVIDENCE/offline-build.log" "$EVIDENCE/console.log"
  fi
else
  cp "$EVIDENCE/offline-build.log" "$EVIDENCE/console.log"
fi
printf '%s\n' "$code" > "$EVIDENCE/build.exit"
exit "$code"
