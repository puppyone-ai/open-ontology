#!/bin/bash
# Fixed archive plus the independent review test. Production files stay read-only.
set -u
echo "oo_task=target-installer-review-probe"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=4ed22b8c_archive_plus_overlay"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a152-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a152-gitsystem
unset RUST_LOG || true
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a152-probe
git config --file "$GIT_CONFIG_GLOBAL" user.email a152-probe@invalid.example
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
check_hash() {
  rel=$1
  expect=$2
  got=$(sha256sum "$rel" | awk '{print $1}')
  want=$(awk '{print $1}' "$expect")
  if [ "$got" != "$want" ] || [ -z "$got" ]; then
    echo "sha mismatch for $rel got=$got want=$want" >&2
    printf '%s\n' 2 > /evidence/snapshot.exit
    exit 2
  fi
}
check_hash crates/oo-pipeline/src/target_installer.rs /evidence/expect-installer.sha256
check_hash crates/oo-pipeline/src/lib.rs /evidence/expect-lib.sha256
check_hash crates/oo-pipeline/src/physical_mapping.rs /evidence/expect-mapping.sha256
check_hash crates/oo-pipeline/src/copy_encoder.rs /evidence/expect-encoder.sha256
check_hash crates/oo-pipeline/migrations/0000_bootstrap_roles.sql /evidence/expect-0000.sha256
check_hash crates/oo-pipeline/migrations/0001_target_fence_expand.sql /evidence/expect-0001.sha256
check_hash crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql /evidence/expect-0002.sha256
check_hash crates/oo-pipeline/migrations/0003_target_segment_preflight_expand.sql /evidence/expect-0003.sha256
check_hash crates/oo-pipeline/migrations/0004_physical_mapping.sql /evidence/expect-0004.sha256
check_hash crates/oo-pipeline/tests/target_installer_a152_review.rs /evidence/expect-overlay.sha256
check_hash Cargo.lock /evidence/expect-lock.sha256
echo "archive_hashes_match=1"
printf '%s\n' 0 > /evidence/snapshot.exit

# Smoke already ran scoped Clippy on this archive. A new container does not
# keep that rustup install. This probe does not run Clippy.
if rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "clippy_component=present_not_used"
else
  echo "clippy_component=absent_smoke_already_ran_clippy"
fi
note clippy-component 0

echo "=== cargo test review oracle ==="
mkdir -p /evidence/live
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test target_installer_a152_review -- --include-ignored --nocapture --test-threads=1
note review $?

check_hash crates/oo-pipeline/src/target_installer.rs /evidence/expect-installer.sha256
check_hash Cargo.lock /evidence/expect-lock.sha256
echo "production_files_unchanged_after_probe=1"
echo "elapsed_seconds=$SECONDS"
exit "$fail"
