#!/bin/bash
# Fixed d80472d8 archive plus the a148 review test only.
set -u
echo "oo_task=a148-copy-encoder-oracle"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a148-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a148-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a148-oracle
git config --file "$GIT_CONFIG_GLOBAL" user.email a148-oracle@invalid.example
git config --file "$GIT_CONFIG_GLOBAL" init.defaultBranch ephemeral
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/toolchain.exit; exit 2 ;;
esac
host_line=$(rustc -Vv | awk '/^host:/{print $2}')
case "$host_line" in
  aarch64-unknown-linux-gnu) ;;
  *) echo "refusing host: $host_line" >&2; printf '%s\n' 2 > /evidence/toolchain.exit; exit 2 ;;
esac
printf '%s\n' 0 > /evidence/toolchain.exit
rustc -Vv > /evidence/rustc-version.txt
cargo -V > /evidence/cargo-version.txt

cd /src
test -f crates/oo-pipeline/tests/a148_copy_oracle.rs
fail=0
note() {
  name=$1
  code=$2
  printf '%s\n' "$code" > "/evidence/${name}.exit"
  echo "phase_exit=$code phase=$name"
  if [ "$code" -ne 0 ] && [ "$fail" -eq 0 ]; then
    fail=$code
  fi
}
sha256sum \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/src/copy_encoder.rs \
  crates/oo-pipeline/tests/copy_encoder_live.rs \
  crates/oo-pipeline/src/physical_mapping.rs \
  crates/oo-pipeline/Cargo.toml \
  contracts/pipeline/copy-encoder-v1.md \
  contracts/source/pgsql/type-conversion-v1.md \
  crates/oo-pipeline/tests/a148_copy_oracle.rs \
  > /evidence/archive-sha256.txt
check_hash() {
  rel=$1
  expect=$2
  got=$(sha256sum "$rel" | awk '{print $1}')
  want=$(awk '{print $1}' "$expect")
  if [ "$got" != "$want" ] || [ -z "$got" ]; then
    echo "archive sha mismatch for $rel got=$got want=$want" >&2
    printf '%s\n' 2 > /evidence/snapshot.exit
    exit 2
  fi
}
check_hash crates/oo-pipeline/src/lib.rs /evidence/expect-lib.sha256
check_hash crates/oo-pipeline/src/copy_encoder.rs /evidence/expect-encoder.sha256
check_hash crates/oo-pipeline/tests/copy_encoder_live.rs /evidence/expect-live.sha256
check_hash crates/oo-pipeline/src/physical_mapping.rs /evidence/expect-mapping.sha256
check_hash crates/oo-pipeline/Cargo.toml /evidence/expect-cargo.sha256
check_hash contracts/pipeline/copy-encoder-v1.md /evidence/expect-contract.sha256
check_hash contracts/source/pgsql/type-conversion-v1.md /evidence/expect-types.sha256
check_hash crates/oo-pipeline/tests/a148_copy_oracle.rs /evidence/expect-overlay.sha256
echo "archive_hashes_match=1"
printf '%s\n' 0 > /evidence/snapshot.exit

if rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy-'; then
  echo "clippy_component=already_installed"
  note clippy-component 0
else
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  rustup component add clippy --toolchain 1.94.0
  code=$?
  set +e
  note clippy-component "$code"
  if [ "$code" -ne 0 ]; then
    echo "elapsed_seconds=$SECONDS"
    exit "$code"
  fi
fi

echo "=== cargo test --test a148_copy_oracle ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline --test a148_copy_oracle -- --nocapture
note test $?
if [ "$(cat /evidence/test.exit)" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fail"
fi

echo "=== cargo clippy -p oo-pipeline --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings
note clippy $?
echo "elapsed_seconds=$SECONDS"
exit "$fail"
