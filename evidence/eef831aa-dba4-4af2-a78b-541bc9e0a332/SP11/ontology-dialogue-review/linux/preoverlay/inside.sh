#!/bin/bash
# Pre-overlay: nine committed tests and Clippy. No PostgreSQL client. No source overlay.
set -u
echo "oo_task=sp11-ontology-dialogue-review-preoverlay"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a332"
echo "oo_fixed=63566b60aabdf29a29bfe55c4866edf3c4081cfd"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export HOME=/tmp
unset OO_A300_LIVE || true
mkdir -p /evidence/linux/preoverlay/phases /cargo-home/bin /tmp
cd /src

record() {
  printf '%s\n' "$2" > "/evidence/linux/preoverlay/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    exit "$2"
  fi
}

if [ -f rust-toolchain.toml ]; then
  echo "archive contains rust-toolchain.toml; refusing a moving toolchain" >&2
  record toolchain-file 2
fi
printf '%s\n' "absent" > /evidence/linux/preoverlay/toolchain-file.txt
if [ ! -f tools/ontology-dialogue/src/lib.rs ] || [ ! -f tools/ontology-dialogue/Cargo.lock ]; then
  echo "archive is missing the committed dialogue crate" >&2
  record source 2
fi
if [ -d /src/evidence ]; then
  echo "archive contains the evidence tree" >&2
  record source 2
fi
record source 0

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/preoverlay/rustc-version.txt
rustc -Vv > /evidence/linux/preoverlay/rustc-verbose.txt
cargo -V | tee /evidence/linux/preoverlay/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

rustc --edition 2021 -O -o /cargo-home/bin/sha256 /review/sha256.rs
record sha256-tool $?
rustc --edition 2021 -O -o /cargo-home/bin/manifest_check /review/manifest_check.rs
record manifest-check-tool $?
rustc --edition 2021 -O -o /cargo-home/bin/lock-assemble /review/lock-assemble.rs
record lock-assemble-tool $?
export PATH="/cargo-home/bin:${PATH}"

set +e
/cargo-home/bin/sha256 dir /tmp/dialogue-before.sha256 /src/tools/ontology-dialogue
hash_before=$?
set -e
record dialogue-hash "$hash_before"
cp /tmp/dialogue-before.sha256 /evidence/linux/preoverlay/dialogue-before.sha256

set +e
/cargo-home/bin/manifest_check listed /review-author/source.sha256 /src \
  > /evidence/linux/preoverlay/source-listed.stdout
listed=$?
set -e
record source-listed "$listed"
if ! grep -q '^mismatch=0$' /evidence/linux/preoverlay/source-listed.stdout \
  || ! grep -q '^missing=0$' /evidence/linux/preoverlay/source-listed.stdout \
  || ! grep -q '^entries=18$' /evidence/linux/preoverlay/source-listed.stdout; then
  echo "author source.sha256 does not match the archive" >&2
  record source-assert 1
fi
record source-assert 0

set +e
/cargo-home/bin/lock-assemble --check \
  tools/description-provider/Cargo.lock \
  Cargo.lock \
  tools/answer-citations/Cargo.lock \
  tools/ontology-dialogue/Cargo.lock \
  > /evidence/linux/preoverlay/lock-check.stdout 2> /evidence/linux/preoverlay/lock-check.stderr
lock_exit=$?
set -e
record lock-check "$lock_exit"

echo "=== cargo fetch ontology-dialogue ==="
set +e
cargo fetch --manifest-path tools/ontology-dialogue/Cargo.toml --locked \
  > /evidence/linux/preoverlay/fetch-dialogue.stdout 2> /evidence/linux/preoverlay/fetch-dialogue.stderr
fetch_exit=$?
set -e
record fetch-dialogue "$fetch_exit"

echo "=== nine local tests ==="
set +e
cargo test --manifest-path tools/ontology-dialogue/Cargo.toml --locked --offline \
  --test propose --test execute_approval \
  > /evidence/linux/preoverlay/test-nine.stdout 2> /evidence/linux/preoverlay/test-nine.stderr
test_exit=$?
set -e
record test-nine "$test_exit"

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 \
    > /evidence/linux/preoverlay/clippy-component.stdout 2> /evidence/linux/preoverlay/clippy-component.stderr
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  printf '%s\n' "already installed" > /evidence/linux/preoverlay/clippy-component.stdout
  record clippy-component 0
fi

echo "=== cargo clippy ontology-dialogue ==="
set +e
cargo clippy --manifest-path tools/ontology-dialogue/Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/linux/preoverlay/clippy-dialogue.stdout 2> /evidence/linux/preoverlay/clippy-dialogue.stderr
clippy_exit=$?
set -e
record clippy-dialogue "$clippy_exit"

/cargo-home/bin/sha256 dir /tmp/dialogue-after.sha256 /src/tools/ontology-dialogue
cp /tmp/dialogue-after.sha256 /evidence/linux/preoverlay/dialogue-after.sha256
if ! cmp -s /tmp/dialogue-before.sha256 /tmp/dialogue-after.sha256; then
  echo "dialogue bytes changed during preoverlay" >&2
  record unchanged 1
fi
record unchanged 0
printf '%s\n' "no" > /evidence/linux/preoverlay/cargo-update-invoked.txt
printf '%s\n' "no" > /evidence/linux/preoverlay/python-invoked.txt
echo "preoverlay_inside_done"
