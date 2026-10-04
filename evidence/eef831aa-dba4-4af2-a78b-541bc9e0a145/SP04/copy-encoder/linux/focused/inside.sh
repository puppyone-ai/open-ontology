#!/bin/bash
# Fixed commit 6b95d6f2 plus the copy-encoder overlay. Package tests, Clippy, live COPY.
set -u
echo "oo_task=copy-encoder-focused"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=6b95d6f2_archive_plus_copy_encoder_overlay"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a145-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a145-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a145-focused
git config --file "$GIT_CONFIG_GLOBAL" user.email a145-focused@invalid.example
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
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/src/copy_encoder.rs \
  crates/oo-pipeline/tests/copy_encoder_live.rs \
  crates/oo-pipeline/src/physical_mapping.rs \
  crates/oo-pipeline/Cargo.toml \
  contracts/pipeline/copy-encoder-v1.md \
  contracts/source/pgsql/type-conversion-v1.md \
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
echo "archive_hashes_match=1"
printf '%s\n' 0 > /evidence/snapshot.exit

ensure_component() {
  name=$1
  if rustup component list --toolchain 1.94.0 --installed | grep -q "^${name}"; then
    echo "${name}_component=already_installed"
    note "${name}-component" 0
    return 0
  fi
  echo "=== rustup component add ${name} --toolchain 1.94.0 ==="
  set +e
  rustup component add "$name" --toolchain 1.94.0
  code=$?
  set +e
  note "${name}-component" "$code"
  if [ "$code" -ne 0 ]; then
    echo "elapsed_seconds=$SECONDS"
    exit "$code"
  fi
}
ensure_component rustfmt
ensure_component clippy

echo "=== rustfmt --check --edition 2024 ==="
set +e
rustfmt --check --edition 2024 \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/src/copy_encoder.rs \
  crates/oo-pipeline/tests/copy_encoder_live.rs
note rustfmt $?
if [ "$(cat /evidence/rustfmt.exit)" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fail"
fi

echo "=== cargo fetch --locked --offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline
note fetch $?
if [ "$(cat /evidence/fetch.exit)" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fail"
fi

echo "=== cargo test -p oo-pipeline ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline
note test $?
if [ "$(cat /evidence/test.exit)" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fail"
fi

echo "=== cargo clippy -p oo-pipeline --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings
note clippy $?
if [ "$(cat /evidence/clippy.exit)" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fail"
fi

echo "=== cargo test --test copy_encoder_live -- --ignored ==="
ready=0
for _ in $(seq 1 30); do
  if timeout 2 bash -c "cat < /dev/null > /dev/tcp/${OO_COPY_ENCODER_HOST}/${OO_COPY_ENCODER_PORT}"; then
    ready=1
    break
  fi
  sleep 1
done
if [ "$ready" -ne 1 ]; then
  echo "postgres tcp was not ready" >&2
  note live 1
  echo "elapsed_seconds=$SECONDS"
  exit 1
fi
set +e
cargo test --manifest-path Cargo.toml --locked --offline --test copy_encoder_live -- --ignored
note live $?

echo "elapsed_seconds=$SECONDS"
exit "$fail"
