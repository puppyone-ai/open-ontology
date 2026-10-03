#!/bin/bash
# Fixed-commit Linux 1.94 library and scoped Clippy smoke.
# The tree is git archive 75b993d0 only. No review overlay. No worktree vendor.
set -u
echo "oo_task=arrow-review-smoke"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
echo "composition=fixed_commit_75b993d0_includes_79515fcf_not_author_ccd93_build_not_a133_worktree"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a134-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a134-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a134-smoke
git config --file "$GIT_CONFIG_GLOBAL" user.email a134-smoke@invalid.example
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

if [ ! -f crates/oo-connectors/src/stream.rs ]; then
  echo "fixed archive is missing stream.rs" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if [ -f crates/oo-connectors/tests/pg_stream_review.rs ]; then
  echo "smoke archive contains the review overlay" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if [ ! -f contracts/source/pgsql/empty-catalog.json ]; then
  echo "fixed archive is missing empty-catalog.json" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
sha256sum contracts/source/pgsql/empty-catalog.json > /evidence/empty-catalog.sha256
sha256sum crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs > /evidence/buffered.sha256
buf=$(sha256sum crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs | awk '{print $1}')
want=$(awk '{print $1}' /evidence/expect-buffered.sha256)
ccd=$(awk '{print $1}' /evidence/forbid-ccd93-buffered.sha256)
live=$(awk '{print $1}' /evidence/forbid-worktree-buffered.sha256)
if [ "$buf" != "$want" ]; then
  echo "buffered.rs sha does not match fixed commit" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if [ "$buf" = "$ccd" ] || [ "$buf" = "$live" ]; then
  echo "buffered.rs is the author base or the live TLS tail" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
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
