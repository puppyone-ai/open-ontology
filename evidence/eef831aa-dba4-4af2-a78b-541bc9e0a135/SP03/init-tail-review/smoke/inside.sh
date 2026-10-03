#!/bin/bash
# Immutable product archive of bd77b252. Lib and scoped clippy only.
# No source edits. No probes.
set -u
echo "oo_task=init-tail-review-smoke"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=bd77b252 product paths, includes Arrow stream.rs from 75b993d0, no review overlay"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a135-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a135-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
cd /src

rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got" > /evidence/smoke/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2
     printf '%s\n' 2 > /evidence/smoke/toolchain.exit
     exit 2 ;;
esac
host_line=$(rustc -Vv | awk '/^host:/{print $2}')
printf 'rustc_host=%s\n' "$host_line" | tee /evidence/smoke/rustc-host.txt
case "$host_line" in
  aarch64-unknown-linux-gnu) ;;
  *) echo "refusing host: $host_line" >&2
     printf '%s\n' 2 > /evidence/smoke/toolchain.exit
     exit 2 ;;
esac
printf '%s\n' 0 > /evidence/smoke/toolchain.exit

if [ ! -f contracts/source/pgsql/empty-catalog.json ]; then
  echo "empty-catalog fixture absent" >&2
  printf '%s\n' 4 > /evidence/smoke/snapshot.exit
  exit 4
fi
if [ ! -f crates/oo-connectors/src/stream.rs ]; then
  echo "archive is missing Arrow stream.rs" >&2
  printf '%s\n' 4 > /evidence/smoke/snapshot.exit
  exit 4
fi
if [ -f crates/oo-connectors/tests/a135_tail_probe.rs ]; then
  echo "smoke archive already contains the review overlay" >&2
  printf '%s\n' 4 > /evidence/smoke/snapshot.exit
  exit 4
fi
sha256sum contracts/source/pgsql/empty-catalog.json > /evidence/smoke/empty-catalog.sha256
sha256sum crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs > /evidence/smoke/buffered.sha256
sha256sum crates/oo-connectors/src/stream.rs > /evidence/smoke/stream.sha256
buf=$(awk '{print $1}' /evidence/smoke/buffered.sha256)
want=$(awk '{print $1}' /evidence/smoke/expect-buffered.sha256)
if [ "$buf" != "$want" ]; then
  echo "buffered.rs sha $buf != $want" >&2
  printf '%s\n' 4 > /evidence/smoke/snapshot.exit
  exit 4
fi
if ! grep -q 'fn committed_init_len' crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs; then
  echo "product blob has no committed_init_len" >&2
  printf '%s\n' 4 > /evidence/smoke/snapshot.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/smoke/snapshot.exit
echo "fixture_present=yes"
echo "product_blob=$buf"

run_phase() {
  name="$1"
  shift
  echo "=== $name ==="
  set +e
  "$@" > "/evidence/smoke/${name}.stdout" 2> "/evidence/smoke/${name}.stderr"
  phase_exit=$?
  set +e
  echo "${name}_exit=$phase_exit"
  printf '%s\n' "$phase_exit" > "/evidence/smoke/${name}.exit"
  cat "/evidence/smoke/${name}.stdout"
  cat "/evidence/smoke/${name}.stderr" >&2
}

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  run_phase clippy-add rustup component add clippy --toolchain 1.94.0
else
  echo "clippy_add_exit=already_installed"
  printf '%s\n' 0 > /evidence/smoke/clippy-add.exit
fi
add_exit=$(cat /evidence/smoke/clippy-add.exit)
if [ "$add_exit" -ne 0 ]; then
  printf '%s\n' "$add_exit" > /evidence/smoke/worst.exit
  exit "$add_exit"
fi

run_phase fetch cargo fetch --locked --manifest-path Cargo.toml
fetch_exit=$(cat /evidence/smoke/fetch.exit)
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' "$fetch_exit" > /evidence/smoke/worst.exit
  exit "$fetch_exit"
fi

echo "=== SMOKE lib ==="
run_phase lib cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --lib -- --test-threads=1
lib_exit=$(cat /evidence/smoke/lib.exit)

echo "=== SMOKE clippy ==="
run_phase clippy cargo clippy --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --all-targets -- -D warnings
clippy_exit=$(cat /evidence/smoke/clippy.exit)

if [ -f /evidence/smoke/lib.stdout ]; then
  if grep -q 'empty_catalog_fixture_matches_the_frozen_hash ... ok' /evidence/smoke/lib.stdout; then
    echo "empty_catalog_fixture=ok" | tee /evidence/smoke/empty-catalog-test.txt
  else
    echo "empty_catalog_fixture=not_ok" | tee /evidence/smoke/empty-catalog-test.txt
  fi
fi
worst=0
for phase_exit in "$lib_exit" "$clippy_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
echo "elapsed_seconds=$SECONDS"
printf '%s\n' "$worst" > /evidence/smoke/worst.exit
exit "$worst"
