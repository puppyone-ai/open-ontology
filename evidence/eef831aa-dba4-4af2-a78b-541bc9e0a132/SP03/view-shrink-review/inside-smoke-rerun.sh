#!/bin/bash
# Rerun of lib and scoped clippy after the archive gained the empty-catalog
# fixture the lib tests include. Same volumes, same toolchain. No probe.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
cd /src
mkdir -p /evidence/smoke-rerun
echo "=== identity ==="
uname -m
rustc -Vv
cargo -V
if [ ! -f contracts/source/pgsql/empty-catalog.json ]; then
  echo "fixture still absent" >&2
  printf '%s\n' 4 > /evidence/smoke-rerun/lib.exit
  exit 4
fi
sha256sum contracts/source/pgsql/empty-catalog.json > /evidence/smoke-rerun/fixture.sha256

run_phase() {
  local name="$1"
  shift
  echo "=== $name ==="
  set +e
  "$@"
  local phase_exit=$?
  set +e
  echo "${name}_exit=$phase_exit"
  printf '%s\n' "$phase_exit" > "/evidence/smoke-rerun/${name}.exit"
}

run_phase lib cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --lib -- --test-threads=1
run_phase clippy cargo clippy --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --all-targets -- -D warnings
lib_exit=$(cat /evidence/smoke-rerun/lib.exit)
clippy_exit=$(cat /evidence/smoke-rerun/clippy.exit)
worst=0
for phase_exit in "$lib_exit" "$clippy_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /evidence/smoke-rerun/worst.exit
exit "$worst"
