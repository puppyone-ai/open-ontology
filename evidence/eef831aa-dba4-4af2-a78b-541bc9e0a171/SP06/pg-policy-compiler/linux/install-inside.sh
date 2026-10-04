#!/bin/bash
# Resolve the new gateway crates, reject lock churn, then run the install tests.
# Repo is writable only so cargo can rename Cargo.lock. This script does not edit other files.
set -u
echo "oo_task=sp06-pg-policy-compiler-install"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a171"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
echo "fetch_argv=cargo fetch --manifest-path Cargo.toml"
echo "test_argv=cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --test policy_pg_live -- --test-threads=1"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
mkdir -p /evidence/linux/install-phases /evidence/supply /evidence/live
cp Cargo.lock /evidence/supply/Cargo.lock.before
awk '
  $1 == "name" && $2 == "=" { gsub(/"/, "", $3); name=$3; next }
  $1 == "version" && $2 == "=" && name != "" { gsub(/"/, "", $3); print name "\t" $3; name=""; next }
' Cargo.lock | sort > /evidence/supply/lock-pairs.before
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0
  clippy_add=$?
  set -e
  printf '%s\n' "$clippy_add" > /evidence/linux/install-phases/clippy-component.exit
  if [ "$clippy_add" -ne 0 ]; then
    exit "$clippy_add"
  fi
else
  printf '%s\n' 0 > /evidence/linux/install-phases/clippy-component.exit
fi
echo "=== cargo fetch ==="
set +e
cargo fetch --manifest-path Cargo.toml
fetch_exit=$?
set -e
printf '%s\n' "$fetch_exit" > /evidence/linux/install-phases/fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
cp Cargo.lock /evidence/supply/Cargo.lock.after
awk '
  $1 == "name" && $2 == "=" { gsub(/"/, "", $3); name=$3; next }
  $1 == "version" && $2 == "=" && name != "" { gsub(/"/, "", $3); print name "\t" $3; name=""; next }
' Cargo.lock | sort > /evidence/supply/lock-pairs.after
comm -23 /evidence/supply/lock-pairs.before /evidence/supply/lock-pairs.after > /evidence/supply/lock-removed.txt
comm -13 /evidence/supply/lock-pairs.before /evidence/supply/lock-pairs.after > /evidence/supply/lock-added.txt
if [ -s /evidence/supply/lock-removed.txt ]; then
  echo "lock churn: an existing package version disappeared" >&2
  printf '%s\n' 1 > /evidence/linux/install-phases/churn.exit
  exit 1
fi
printf '%s\n' 0 > /evidence/linux/install-phases/churn.exit
echo "=== cargo clippy oo-gateway ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --all-targets -- -D warnings
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/linux/install-phases/clippy.exit
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi
echo "=== arch focused ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-arch-check --lib -- gateway_postgres_stack_is_allowed gateway_sync_postgres_and_sqlx_are_forbidden dev_only_gateway_pipeline_is_forbidden
arch_exit=$?
set -e
printf '%s\n' "$arch_exit" > /evidence/linux/install-phases/arch.exit
if [ "$arch_exit" -ne 0 ]; then
  exit "$arch_exit"
fi
echo "=== cargo test oo-gateway lib and policy_pg_live ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --test policy_pg_live -- --test-threads=1
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/linux/install-phases/test.exit
exit "$test_exit"
