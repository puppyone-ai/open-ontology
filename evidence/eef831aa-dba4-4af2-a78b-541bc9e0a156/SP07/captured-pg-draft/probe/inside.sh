#!/bin/bash
# Build and run the evidence probe against the archive snapshot. No product install.
set -u
echo "oo_task=captured-pg-draft-probe"
echo "oo_phase=${PHASE}"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=4
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a156-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a156-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
cd /src
mkdir -p /out

got=$(rustc --version)
printf '%s\n' "$got" > "/out/${PHASE}-rustc-version.txt"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2
     exit 2 ;;
esac
host_line=$(rustc -Vv | awk '/^host:/{print $2}')
printf 'rustc_host=%s\n' "$host_line" | tee "/out/${PHASE}-rustc-host.txt"
case "$host_line" in
  aarch64-unknown-linux-gnu) ;;
  *) echo "refusing host: $host_line" >&2
     exit 2 ;;
esac
if [ ! -f crates/oo-builder/src/captured_pg.rs ] || [ ! -f probe/src/main.rs ]; then
  echo "probe snapshot is missing the overlay or the probe" >&2
  exit 4
fi
if [ "${PHASE}" != "capture" ] && [ "${PHASE}" != "stale" ]; then
  echo "PHASE must be capture or stale" >&2
  exit 2
fi

echo "=== build ==="
set +e
cargo build -p a156-captured-pg-probe > "/out/${PHASE}-build.stdout" 2> "/out/${PHASE}-build.stderr"
code=$?
set -e
printf '%s\n' "$code" > "/out/${PHASE}-build.exit"
echo "phase build exit ${code}"
if [ "$code" -ne 0 ]; then
  exit "$code"
fi

echo "=== ${PHASE} ==="
set +e
/target/debug/a156-captured-pg-probe "$PHASE" > "/out/${PHASE}.stdout" 2> "/out/${PHASE}.stderr"
code=$?
set -e
printf '%s\n' "$code" > "/out/${PHASE}.exit"
echo "phase ${PHASE} exit ${code}"
if [ "$code" -ne 0 ]; then
  exit "$code"
fi
echo "PROBE_PHASE_OK ${PHASE}"
