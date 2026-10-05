#!/bin/bash
# Independent oracle on the frozen archive. The oracle is not copied into the source tree.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
cd /src

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show
uname -a
cat /etc/os-release
id
cat /proc/mounts > /evidence/proc-mounts.txt

echo "=== frozen pins ==="
sha256sum \
  crates/oo-builder/src/role_pack.rs \
  crates/oo-builder/src/bin/oo-builder.rs \
  crates/oo-builder/src/bin/role-pack-bundle.rs \
  crates/oo-builder/tests/role_pack_bundle.rs \
  contracts/builder/role-pack-bundle-v1.md \
  > /evidence/archive-sha256.txt
if [ -f crates/oo-builder/tests/role_pack_bundle_a238_review.rs ]; then
  echo "review overlay is inside the frozen archive" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit
if ! sha256sum -c /evidence/expect-sha256.txt > /evidence/hash-compare.txt 2>&1; then
  echo "archive hash mismatch" >&2
  printf '%s\n' 2 > /evidence/hash-compare.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/hash-compare.exit
if ! grep -q 'std::fs::hard_link(' crates/oo-builder/src/bin/role-pack-bundle.rs; then
  echo "frozen bundle does not hard link" >&2
  printf '%s\n' 2 > /evidence/no-rename.exit
  exit 2
fi
if grep -q 'rename(' crates/oo-builder/src/bin/role-pack-bundle.rs; then
  echo "frozen bundle still renames" >&2
  printf '%s\n' 2 > /evidence/no-rename.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/no-rename.exit
if [ ! -f /oracle/review_oracle.rs ]; then
  echo "review oracle missing" >&2
  printf '%s\n' 2 > /evidence/oracle-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/oracle-present.exit

echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.stdout /evidence/fetch.stdout
  cp /evidence/fetch-offline.stderr /evidence/fetch.stderr
fi
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/build.exit
  printf '%s\n' not_run > /evidence/oracle.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$fetch_exit"
fi

echo "=== cargo build role-pack binaries ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-builder \
  --bin role-pack-bundle --bin oo-builder \
  > /evidence/build.stdout 2> /evidence/build.stderr
build_exit=$?
set -e
printf '%s\n' "$build_exit" > /evidence/build.exit
echo "PHASE build_exit=$build_exit"
if [ "$build_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/oracle.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$build_exit"
fi

echo "=== rustc review oracle ==="
set +e
rustc --edition 2021 -D warnings /oracle/review_oracle.rs -o /target/a238-review-oracle \
  > /evidence/oracle-compile.stdout 2> /evidence/oracle-compile.stderr
oracle_compile_exit=$?
set -e
printf '%s\n' "$oracle_compile_exit" > /evidence/oracle-compile.exit
echo "PHASE oracle_compile_exit=$oracle_compile_exit"
if [ "$oracle_compile_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/oracle.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$oracle_compile_exit"
fi

echo "=== review oracle ==="
mkdir -p /tmp/oo-a238-work /evidence/fs-probe
set +e
/target/a238-review-oracle \
  /target/debug/oo-builder \
  /target/debug/role-pack-bundle \
  /tmp/oo-a238-work \
  /evidence/oracle.txt \
  /evidence/fs-probe \
  > /evidence/oracle.stdout 2> /evidence/oracle.stderr
oracle_exit=$?
set -e
printf '%s\n' "$oracle_exit" > /evidence/oracle.exit
echo "PHASE oracle_exit=$oracle_exit"
if [ "$oracle_exit" -eq 0 ]; then
  rm -rf /evidence/fs-probe
fi

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  worst=$component_exit
  if [ "$oracle_exit" -ne 0 ]; then worst=$oracle_exit; fi
  echo "worst_phase_exit=$worst"
  exit "$worst"
fi

echo "=== cargo clippy role-pack-bundle ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder \
  --bin role-pack-bundle --test role_pack_bundle -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

worst=0
if [ "$oracle_exit" -ne 0 ]; then worst=$oracle_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
echo "worst_phase_exit=$worst"
exit "$worst"
