#!/bin/bash
# Pre-edit MSRV smoke of the frozen dialogue command. No live test, no model call.
# The archive is readonly. This script does not write a proposal or start query-pg.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export HOME=/tmp
export TMPDIR=/tmp
mkdir -p /evidence/linux/smoke/phases /tmp
cd /src
rustc -vV | tee /evidence/linux/smoke/rustc-version.txt
got=$(rustc --version)
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
test -f tools/ontology-dialogue/Cargo.toml
test -f tools/ontology-dialogue/src/lib.rs
test -f tools/ontology-dialogue/src/propose.rs
if [ -f rust-toolchain.toml ]; then
  echo "toolchain file left in place; archive mount is readonly" > /evidence/linux/smoke/toolchain-note.txt
fi
find tools/ontology-dialogue -type f | sort | xargs sha256sum > /tmp/dialogue-before.sha256

phase() {
  local name="$1"
  shift
  set +e
  "$@" > "/evidence/linux/smoke/${name}.stdout" 2> "/evidence/linux/smoke/${name}.stderr"
  local code=$?
  printf '%s\n' "$code" > "/evidence/linux/smoke/phases/${name}.exit"
  echo "phase_exit=${code} phase=${name}"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
}

phase fetch cargo fetch --manifest-path tools/ontology-dialogue/Cargo.toml --locked
phase test cargo test --manifest-path tools/ontology-dialogue/Cargo.toml --locked --offline \
  --test propose --test execute_approval
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  phase clippy-component rustup component add clippy --toolchain 1.94.0
else
  echo "already installed" > /evidence/linux/smoke/clippy-component.stdout
  printf '%s\n' 0 > /evidence/linux/smoke/phases/clippy-component.exit
fi
phase clippy cargo clippy --manifest-path tools/ontology-dialogue/Cargo.toml --locked --offline \
  --all-targets -- -D warnings
find tools/ontology-dialogue -type f | sort | xargs sha256sum > /tmp/dialogue-after.sha256
cp /tmp/dialogue-before.sha256 /evidence/linux/smoke/dialogue-before.sha256
cp /tmp/dialogue-after.sha256 /evidence/linux/smoke/dialogue-after.sha256
if ! cmp -s /tmp/dialogue-before.sha256 /tmp/dialogue-after.sha256; then
  echo "dialogue bytes changed during smoke" >&2
  printf '%s\n' 1 > /evidence/linux/smoke/phases/unchanged.exit
  exit 1
fi
printf '%s\n' 0 > /evidence/linux/smoke/phases/unchanged.exit
printf '%s\n' "no" > /evidence/linux/smoke/cargo-update-invoked.txt
printf '%s\n' "no" > /evidence/linux/smoke/python-invoked.txt
echo "smoke_ok"
