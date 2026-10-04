#!/bin/bash
# Archive of d80472d8 plus the completion overlay. Scoped Clippy, then the live tests.
set -u
echo "oo_task=reader-completion-probe-3"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=d80472d8_archive_plus_completion_overlay"
echo "prior_probe=probe clippy component missing in a fresh container"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a147-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a147-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a147-probe
git config --file "$GIT_CONFIG_GLOBAL" user.email a147-probe@invalid.example
git config --file "$GIT_CONFIG_GLOBAL" init.defaultBranch ephemeral
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
    echo "overlay sha mismatch for $rel got=$got want=$want" >&2
    printf '%s\n' 2 > /evidence/overlay.exit
    exit 2
  fi
}
check_hash crates/oo-connectors/src/snapshot.rs /evidence/expect-snapshot.sha256
check_hash crates/oo-connectors/src/stream.rs /evidence/expect-stream.sha256
check_hash crates/oo-connectors/src/lib.rs /evidence/expect-lib.sha256
check_hash crates/oo-connectors/src/session.rs /evidence/expect-session.sha256
check_hash contracts/source/pgsql/reader-lifecycle-v1.md /evidence/expect-lifecycle.sha256
check_hash crates/oo-connectors/tests/pg_reader_complete_live.rs /evidence/expect-complete-test.sha256
echo "overlay_hashes_match=1"
printf '%s\n' 0 > /evidence/overlay.exit

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  rustup component add clippy --toolchain 1.94.0
  clippy_add=$?
  set +e
  note clippy-component "$clippy_add"
  if [ "$clippy_add" -ne 0 ]; then
    echo "elapsed_seconds=$SECONDS"
    exit "$clippy_add"
  fi
else
  echo "clippy_component=already_installed"
  note clippy-component 0
fi

echo "=== cargo clippy -p oo-connectors --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
note clippy $?
if [ "$(cat /evidence/clippy.exit)" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fail"
fi

export OO_PG_COMPLETE_FIXTURE=/secrets/pointer.json
export OO_PG_COMPLETE_OUTCOME=/evidence/complete-outcome.txt
export OO_PG_FINISH_FIXTURE=/secrets/pointer.json
export OO_PG_FINISH_OUTCOME=/evidence/finish-outcome.txt

echo "=== cargo test pg_reader_complete_live ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_reader_complete_live -- --test-threads=1
note pg_reader_complete_live $?
if [ "$(cat /evidence/pg_reader_complete_live.exit)" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fail"
fi

echo "=== cargo test pg_reader_finish_live ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_reader_finish_live -- --test-threads=1
note pg_reader_finish_live $?

echo "elapsed_seconds=$SECONDS"
exit "$fail"
