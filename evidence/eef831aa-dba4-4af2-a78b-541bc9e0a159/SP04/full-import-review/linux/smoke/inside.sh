#!/bin/bash
# git archive of cc764cbc. Package lib tests and scoped Clippy. No overlay, no PostgreSQL.
set -u
echo "oo_task=full-import-review-smoke"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=cc764cbc_archive_no_overlay"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a159-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a159-gitsystem
unset RUST_LOG || true
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a159-smoke
git config --file "$GIT_CONFIG_GLOBAL" user.email a159-smoke@invalid.example
git config --file "$GIT_CONFIG_GLOBAL" init.defaultBranch ephemeral
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/toolchain.exit; exit 2 ;;
esac
host_line=$(rustc -Vv | awk '/^host:/{print $2}')
printf 'rustc_host=%s\n' "$host_line"
case "$host_line" in
  aarch64-unknown-linux-gnu) ;;
  *) echo "refusing host: $host_line" >&2; printf '%s\n' 2 > /evidence/toolchain.exit; exit 2 ;;
esac
printf '%s\n' 0 > /evidence/toolchain.exit
rustc -Vv > /evidence/rustc-version.txt
cargo -V > /evidence/cargo-version.txt

cd /src
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
  crates/oo-pipeline/Cargo.toml \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/src/full_import.rs \
  crates/oo-pipeline/src/copy_sink.rs \
  crates/oo-pipeline/src/copy_encoder.rs \
  crates/oo-pipeline/src/physical_mapping.rs \
  crates/oo-pipeline/src/target_installer.rs \
  crates/oo-pipeline/migrations/0000_bootstrap_roles.sql \
  crates/oo-pipeline/migrations/0001_target_fence_expand.sql \
  crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql \
  crates/oo-pipeline/migrations/0003_target_segment_preflight_expand.sql \
  crates/oo-pipeline/migrations/0004_physical_mapping.sql \
  crates/oo-pipeline/migrations/0005_full_import_expand.sql \
  contracts/pipeline/full-import-v1.md \
  contracts/pipeline/full-import-v1.json \
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
check_hash crates/oo-pipeline/Cargo.toml /evidence/expect-cargo.sha256
check_hash crates/oo-pipeline/src/lib.rs /evidence/expect-lib.sha256
check_hash crates/oo-pipeline/src/full_import.rs /evidence/expect-full-import.sha256
check_hash crates/oo-pipeline/src/copy_sink.rs /evidence/expect-sink.sha256
check_hash crates/oo-pipeline/src/copy_encoder.rs /evidence/expect-encoder.sha256
check_hash crates/oo-pipeline/src/physical_mapping.rs /evidence/expect-mapping.sha256
check_hash crates/oo-pipeline/src/target_installer.rs /evidence/expect-installer.sha256
check_hash crates/oo-pipeline/migrations/0005_full_import_expand.sql /evidence/expect-0005.sha256
check_hash contracts/pipeline/full-import-v1.json /evidence/expect-contract-json.sha256
if [ -e crates/oo-pipeline/tests/full_import_a159_review.rs ]; then
  echo "smoke archive contains the review overlay" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
echo "archive_hashes_match=1"
printf '%s\n' 0 > /evidence/snapshot.exit

echo "=== simple classifier: unsafe tokens in pipeline sources ==="
grep -n -H 'unsafe' \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/src/full_import.rs \
  crates/oo-pipeline/src/copy_sink.rs \
  crates/oo-pipeline/src/copy_encoder.rs \
  crates/oo-pipeline/src/physical_mapping.rs \
  crates/oo-pipeline/src/target_installer.rs \
  > /evidence/unsafe-lines.txt || true
unsafe_hits=$(grep -c . /evidence/unsafe-lines.txt || true)
printf 'unsafe_token_lines=%s\n' "$unsafe_hits" | tee /evidence/classifier.txt
if ! grep -q 'forbid(unsafe_code)' crates/oo-pipeline/src/lib.rs; then
  echo "missing forbid(unsafe_code)" >&2
  printf '%s\n' 2 > /evidence/classifier.exit
  exit 2
fi
if grep -n 'unsafe' crates/oo-pipeline/src/full_import.rs crates/oo-pipeline/src/copy_sink.rs | grep -v 'forbid(unsafe_code)'; then
  echo "new import sources contain unsafe" >&2
  printf '%s\n' 2 > /evidence/classifier.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/classifier.exit
echo "classifier=simple_grep_plus_forbid_unsafe_code"

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.stdout 2> /evidence/rustup-clippy.stderr
  clippy_add=$?
  set +e
  note rustup-clippy "$clippy_add"
  if [ "$clippy_add" -ne 0 ]; then
    echo "elapsed_seconds=$SECONDS"
    exit "$clippy_add"
  fi
else
  echo "clippy_component=already_installed"
  note rustup-clippy 0
fi

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
set +e
note fetch "$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fetch_exit"
fi

echo "=== cargo test --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --lib > /evidence/lib.stdout 2> /evidence/lib.stderr
note lib $?
if [ "$(cat /evidence/lib.exit)" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fail"
fi

echo "=== cargo clippy -p oo-pipeline --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
note clippy $?

echo "elapsed_seconds=$SECONDS"
exit "$fail"
