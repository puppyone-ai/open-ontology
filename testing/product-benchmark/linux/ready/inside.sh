#!/bin/bash
# Ready-check the pinned compiler and dialogue. No model call. No PostgreSQL.
set -u
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export TMPDIR=/cargo-home/tmp
export CARGO_BUILD_JOBS=1
export RUSTUP_TOOLCHAIN=1.94.0
unset http_proxy https_proxy HTTP_PROXY HTTPS_PROXY ALL_PROXY all_proxy
mkdir -p /cargo-home/tmp /cargo-home/bin /evidence/phases
printf '%s\n' "1.94.0" > /evidence/expect-rustc.txt
rustc --version > /evidence/rustc-version.txt 2>&1
rustc_exit=$?
printf '%s\n' "$rustc_exit" > /evidence/phases/rustc.exit
grep -q 'rustc 1.94.0' /evidence/rustc-version.txt || exit 1

run_log() {
  local name=$1
  shift
  "$@" > "/evidence/${name}.log" 2>&1
  local code=$?
  printf '%s\n' "$code" > "/evidence/phases/${name}.exit"
  echo "phase_exit=$code phase=$name"
  return 0
}

run_log fetch-builder cargo fetch --manifest-path /src/Cargo.toml --locked
run_log build-bundle cargo build --manifest-path /src/Cargo.toml --locked --offline -p oo-builder --bin role-pack-bundle
if [ "$(cat /evidence/phases/build-bundle.exit)" != "0" ]; then
  printf '%s\n' 1 > /evidence/ready.exit
  exit 1
fi
cp /target/debug/role-pack-bundle /cargo-home/bin/role-pack-bundle

run_log compile-a399 /cargo-home/bin/role-pack-bundle --request /a399/pack-command.json --output /evidence/a399-bundle.json
run_log compile-shop /cargo-home/bin/role-pack-bundle --request /evidence/shop-command.json --output /evidence/shop-bundle.json

run_log fetch-dialogue cargo fetch --manifest-path /src/tools/ontology-dialogue/Cargo.toml --locked
run_log test-identity cargo test --manifest-path /src/tools/ontology-dialogue/Cargo.toml --locked --offline --test identity_compat -- --test-threads=1

if [ "$(cat /evidence/phases/compile-a399.exit)" = "0" ] && [ "$(cat /evidence/phases/test-identity.exit)" = "0" ]; then
  printf '%s\n' 0 > /evidence/ready.exit
else
  printf '%s\n' 1 > /evidence/ready.exit
fi
exit "$(cat /evidence/ready.exit)"
