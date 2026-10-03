#!/bin/bash
# Fixed-commit Linux 1.94 library and scoped Clippy smoke.
# The tree is git archive bd77b252 only. No ACL overlay.
set -u
echo "oo_task=stream-acl-smoke"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
echo "composition=fixed_commit_bd77b252_no_overlay"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a136-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a136-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a136-smoke
git config --file "$GIT_CONFIG_GLOBAL" user.email a136-smoke@invalid.example
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

if [ ! -f crates/oo-connectors/src/snapshot.rs ]; then
  echo "fixed archive is missing snapshot.rs" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if [ -f crates/oo-connectors/tests/pg_stream_acl_live.rs ]; then
  echo "smoke archive contains the ACL overlay" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
sha256sum crates/oo-connectors/src/snapshot.rs crates/oo-connectors/src/sql.rs crates/oo-connectors/src/stream.rs crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs > /evidence/archive-sha256.txt
for pair in snapshot.rs:expect-snapshot.sha256 sql.rs:expect-sql.sha256 stream.rs:expect-stream.sha256 buffered.rs:expect-buffered.sha256; do
  file=${pair%%:*}
  wantfile=${pair##*:}
  got=$(awk -v f="$file" '$2 ~ f {print $1}' /evidence/archive-sha256.txt)
  want=$(awk '{print $1}' "/evidence/$wantfile")
  if [ "$got" != "$want" ] || [ -z "$got" ]; then
    echo "archive sha mismatch for $file" >&2
    printf '%s\n' 2 > /evidence/snapshot.exit
    exit 2
  fi
done
printf '%s\n' 0 > /evidence/snapshot.exit

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

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked
fetch_exit=$?
set +e
note fetch "$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fetch_exit"
fi

echo "=== cargo test --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
note lib $?
if [ "$(cat /evidence/lib.exit)" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fail"
fi

echo "=== cargo clippy -p oo-connectors --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
note clippy $?

echo "elapsed_seconds=$SECONDS"
exit "$fail"
