#!/bin/bash
# Focused Linux arm64 Rust 1.94 test and clippy of oo-local-llm.
# The fixed archive is mounted only to compare lock package versions.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export TMPDIR=/target/tmp
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY http_proxy https_proxy all_proxy NO_PROXY no_proxy
unset OPENAI_API_KEY OPENAI_BASE_URL OPENAI_API_BASE
mkdir -p /target/tmp /evidence/phases
cd /crate

phase() {
  local name="$1"
  shift
  set +e
  "$@" > "/evidence/${name}.stdout" 2> "/evidence/${name}.stderr"
  local code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/phases/${name}.exit"
  echo "${name}_exit=${code}"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
}

echo "=== identity ==="
rustc -vV | tee /evidence/rustc-version.txt
cargo -V | tee /evidence/cargo-version.txt
if ! grep -q '1.94.0' /evidence/rustc-version.txt; then
  echo "msrv mismatch" >&2
  exit 1
fi
if [ -f /archive/tools/ontology-dialogue/Cargo.toml ]; then
  echo "archive unexpectedly has dialogue" >&2
  exit 1
fi
test -f /archive/Cargo.lock
test -f /archive/tools/description-provider/Cargo.lock

echo "=== source hash before cargo ==="
find src tests Cargo.toml CONTRACT.md .gitignore -type f 2>/dev/null | sort | xargs sha256sum > /evidence/source-before.sha256 || true
find src Cargo.toml CONTRACT.md .gitignore -type f | sort | xargs sha256sum > /evidence/source-before.sha256

if ! rustup component list --installed | grep -q '^clippy'; then
  phase rustup-clippy rustup component add clippy
else
  echo "clippy already installed" | tee /evidence/rustup-clippy.stdout
  printf '%s\n' 0 > /evidence/phases/rustup-clippy.exit
fi

set +e
cargo generate-lockfile --manifest-path /crate/Cargo.toml --offline > /evidence/lock-generate-offline.stdout 2> /evidence/lock-generate-offline.stderr
offline_code=$?
set -e
printf '%s\n' "$offline_code" > /evidence/phases/lock-generate-offline.exit
echo "lock_generate_offline_exit=${offline_code}"
if ! grep -q 'name = "oo-local-llm"' /crate/Cargo.lock 2>/dev/null; then
  phase lock-generate cargo generate-lockfile --manifest-path /crate/Cargo.toml
else
  printf '%s\n' 0 > /evidence/phases/lock-generate.exit
fi

pairs() {
  awk '$1=="name" && $2=="=" { gsub(/"/,"",$3); name=$3; next }
       $1=="version" && $2=="=" && name!="" { gsub(/"/,"",$3); print name, $3; name="" }' "$1"
}
pairs /archive/Cargo.lock > /tmp/allowed.pairs
pairs /archive/tools/description-provider/Cargo.lock >> /tmp/allowed.pairs
pairs /crate/Cargo.lock | awk '$1 != "oo-local-llm"' > /tmp/new.pairs
sort -u /tmp/allowed.pairs -o /tmp/allowed.pairs
sort -u /tmp/new.pairs -o /tmp/new.pairs
comm -13 /tmp/allowed.pairs /tmp/new.pairs > /evidence/lock-extra.txt
if [ -s /evidence/lock-extra.txt ]; then
  echo "lock contains package versions outside the fixed archive" >&2
  wc -l /evidence/lock-extra.txt
  exit 1
fi
echo "lock_versions=subset" | tee /evidence/lock-versions.txt
cp /crate/Cargo.lock /evidence/Cargo.lock

phase fetch cargo fetch --manifest-path /crate/Cargo.toml --locked
phase test cargo test --manifest-path /crate/Cargo.toml --locked --offline --all-targets
phase clippy cargo clippy --manifest-path /crate/Cargo.toml --locked --offline --all-targets -- -D warnings
find src Cargo.toml Cargo.lock CONTRACT.md .gitignore -type f | sort | xargs sha256sum > /evidence/source-after.sha256
echo "proof_ok"
