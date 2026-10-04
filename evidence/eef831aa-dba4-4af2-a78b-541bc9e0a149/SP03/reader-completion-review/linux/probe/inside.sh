#!/bin/bash
# Archive of 16ca2969 plus the independent review test. No production edit.
set -u
echo "oo_task=reader-completion-review-probe"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=16ca2969_archive_plus_review_overlay"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a149-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a149-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a149-probe
git config --file "$GIT_CONFIG_GLOBAL" user.email a149-probe@invalid.example
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
check_hash contracts/source/pgsql/bounded-stream-v1.md /evidence/expect-bounded.sha256
check_hash crates/oo-connectors/tests/pg_reader_r4_review.rs /evidence/expect-review-test.sha256
echo "overlay_hashes_match=1"
printf '%s\n' 0 > /evidence/overlay.exit
sed -n '9p' contracts/source/pgsql/bounded-stream-v1.md > /evidence/bounded-stream-line9.txt

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

export OO_PG_R4_FIXTURE=/secrets/pointer.json
export OO_PG_R4_OUTCOME=/evidence/review-outcome.txt

echo "=== cargo test pg_reader_r4_review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_reader_r4_review -- --test-threads=1 --nocapture
note pg_reader_r4_review $?

echo "=== private construction must fail to compile ==="
cp /overlay/run_complete_private_fail.rs crates/oo-connectors/tests/run_complete_private_fail.rs
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test run_complete_private_fail -- --test-threads=1 > /evidence/private-construct.stdout 2> /evidence/private-construct.stderr
priv=$?
set +e
printf '%s\n' "$priv" > /evidence/private-construct.cargo.exit
if [ "$priv" -eq 0 ]; then
  echo "private construction compiled" >&2
  note private-construct 1
  if [ "$fail" -eq 0 ]; then
    fail=1
  fi
else
  if grep -E -q 'private|E0451|E0603|E0599' /evidence/private-construct.stderr; then
    echo "private_construction_rejected=1 cargo_exit=$priv"
    note private-construct 0
  else
    echo "private construction failed without a visibility error" >&2
    note private-construct 1
    if [ "$fail" -eq 0 ]; then
      fail=1
    fi
  fi
fi
rm -f crates/oo-connectors/tests/run_complete_private_fail.rs
check_hash crates/oo-connectors/src/snapshot.rs /evidence/expect-snapshot.sha256
check_hash crates/oo-connectors/src/stream.rs /evidence/expect-stream.sha256
check_hash crates/oo-connectors/src/lib.rs /evidence/expect-lib.sha256
check_hash crates/oo-connectors/src/session.rs /evidence/expect-session.sha256
echo "production_files_unchanged_after_probe=1"

if [ -f /pglog/postgresql.log ]; then
  cp /pglog/postgresql.log /evidence/postgresql.log
fi

echo "elapsed_seconds=$SECONDS"
exit "$fail"
