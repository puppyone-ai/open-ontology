#!/bin/bash
# a108 independent review. Probe first, then the committed oo-pipeline smoke.
# The live test is the author's harness, recorded separately from the probe.
set -u
echo "oo_task=sp04-segment-preflight-review"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
echo "source_commit=6cd955af38ce426bb1102a6e078e8580febed776"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
echo "toolchain_file:"
cat /src/rust-toolchain.toml
rustc -Vv
cargo -V
rustup show
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
check_hash() {
  file=$1
  expect=$2
  got=$(sha256sum "$file" | awk '{print $1}')
  echo "sha256 $file $got"
  if [ "$got" != "$expect" ]; then
    echo "hash mismatch $file" >&2
    exit 2
  fi
}
check_hash crates/oo-pipeline/migrations/0000_bootstrap_roles.sql 37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208
check_hash crates/oo-pipeline/migrations/0001_target_fence_expand.sql 97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9
check_hash crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql 59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512
check_hash crates/oo-pipeline/migrations/0003_target_segment_preflight_expand.sql d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3
check_hash contracts/pipeline/target-fence-receipts/target-fence-receipts-v1.json 429faa9d32e4b42960e805e0883a2f67c8783d936e9094f0747e9c5e21a23521
check_hash contracts/pipeline/target-fence-receipts/target-fence-receipts-v2.json 3a6ca1f4db777f98e8b76f0bd6cc6b5039f2e82d72c478ef4c8fe37713254bac
mkdir -p /evidence/cases /phases/author-live
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  rustup component add clippy --toolchain 1.94.0
  clippy_add=$?
  set -e
  echo "phase_exit=$clippy_add phase=rustup_component_add_clippy"
  printf '%s\n' "$clippy_add" > /phases/clippy-component.exit
  if [ "$clippy_add" -ne 0 ]; then
    printf '%s\n' "$clippy_add" > /phases/worst.exit
    exit "$clippy_add"
  fi
else
  echo "clippy_component=already_installed"
  printf '%s\n' 0 > /phases/clippy-component.exit
fi

echo "=== rustc probe ==="
set +e
# /tmp is a noexec tmpfs under cap-drop ALL. The probe binary has to live on the target volume.
rustc --edition 2021 -D warnings -o /target/a108-probe /probe/src/main.rs
probe_build=$?
set -e
echo "phase_exit=$probe_build phase=rustc_probe"
printf '%s\n' "$probe_build" > /phases/probe-build.exit
if [ "$probe_build" -ne 0 ]; then
  printf '%s\n' "$probe_build" > /phases/worst.exit
  exit "$probe_build"
fi
echo "=== independent probe ==="
set +e
/target/a108-probe
probe_exit=$?
set -e
echo "phase_exit=$probe_exit phase=independent_probe"
printf '%s\n' "$probe_exit" > /phases/probe.exit

echo "=== cargo fetch --manifest-path Cargo.toml --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked
fetch_exit=$?
set -e
echo "phase_exit=$fetch_exit phase=cargo_fetch_locked"
printf '%s\n' "$fetch_exit" > /phases/fetch.exit

test_exit=9
clippy_exit=9
live_exit=9
if [ "$fetch_exit" -eq 0 ]; then
  echo "=== cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline ==="
  set +e
  cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline
  test_exit=$?
  set -e
  echo "phase_exit=$test_exit phase=cargo_test_oo_pipeline"
  printf '%s\n' "$test_exit" > /phases/test.exit
  echo "=== cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings ==="
  set +e
  cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings
  clippy_exit=$?
  set -e
  echo "phase_exit=$clippy_exit phase=cargo_clippy_oo_pipeline"
  printf '%s\n' "$clippy_exit" > /phases/clippy.exit
  if [ "$test_exit" -eq 0 ]; then
    host=$(tr -d '[:space:]' < /fixture/host.txt)
    export OO_SEGMENT_PREFLIGHT_HOST="$host"
    export OO_SEGMENT_PREFLIGHT_PORT=5432
    export OO_SEGMENT_PREFLIGHT_EVIDENCE=/phases/author-live
    echo "=== author live test on committed archive ==="
    set +e
    cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test segment_preflight_correction live_segment_preflight -- --ignored --exact
    live_exit=$?
    set -e
    echo "phase_exit=$live_exit phase=author_live_on_committed_tree"
    printf '%s\n' "$live_exit" > /phases/author-live.exit
  else
    echo "author live skipped because package test exit was $test_exit"
    printf '%s\n' "skipped" > /phases/author-live.exit
  fi
else
  printf '%s\n' "$fetch_exit" > /phases/test.exit
  printf '%s\n' "$fetch_exit" > /phases/clippy.exit
  printf '%s\n' "skipped" > /phases/author-live.exit
fi

worst=0
for code in "$probe_exit" "$fetch_exit" "$test_exit" "$clippy_exit" "$live_exit"; do
  if [ "$code" -ne 0 ]; then
    worst=$code
    break
  fi
done
printf '%s\n' "$worst" > /phases/worst.exit
echo "worst=$worst probe=$probe_exit fetch=$fetch_exit test=$test_exit clippy=$clippy_exit live=$live_exit"
if [ -f /inside.sh ]; then
  chown -R "$(stat -c '%u' /inside.sh)":"$(stat -c '%g' /inside.sh)" /evidence /phases || true
fi
exit "$worst"
