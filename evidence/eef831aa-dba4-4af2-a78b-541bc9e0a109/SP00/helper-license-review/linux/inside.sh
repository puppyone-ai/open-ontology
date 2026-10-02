#!/bin/bash
# Runs inside the single a109 Linux arm64 Rust 1.94 container.
set -u
OUT=/review/linux
mkdir -p "$OUT/phases" "$OUT/probe"
exec > >(tee "$OUT/inside-console.out") 2> >(tee "$OUT/inside-console.err" >&2)

phase() {
  local name="$1"
  shift
  echo "PHASE $name start $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  set +e
  "$@" >"$OUT/phases/${name}.out" 2>"$OUT/phases/${name}.err"
  local code=$?
  set -e
  echo "$code" >"$OUT/phases/${name}.exit"
  echo "PHASE $name exit $code"
  return 0
}

set -e
{
  echo "uname=$(uname -a)"
  echo "date=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  rustc --version --verbose
  cargo --version
  echo "RUSTUP_TOOLCHAIN=${RUSTUP_TOOLCHAIN-}"
  command -v sha256sum
  sha256sum /usr/bin/sha256sum || true
  sha256sum /archive/testing/harness/containment-supervisor/Cargo.lock
  sha256sum /archive/ci/deny.toml
} | tee "$OUT/phases/identity.out"
echo 0 >"$OUT/phases/identity.exit"

LOCK=$(sha256sum /archive/testing/harness/containment-supervisor/Cargo.lock | awk '{print $1}')
DENY=$(sha256sum /archive/ci/deny.toml | awk '{print $1}')
if [ "$LOCK" != "14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1" ]; then
  echo "lock pin mismatch $LOCK" | tee "$OUT/phases/pin.err"
  echo 3 >"$OUT/phases/pin.exit"
  exit 3
fi
if [ "$DENY" != "4320807bfe747fe339ce7bf539a1cff5604e8e0c7677760e2cbc7abc1868624b" ]; then
  echo "deny pin mismatch $DENY" | tee "$OUT/phases/pin.err"
  echo 3 >"$OUT/phases/pin.exit"
  exit 3
fi
echo 0 >"$OUT/phases/pin.exit"

export CARGO_TERM_COLOR=never
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/checker
export CARGO_BUILD_JOBS=2
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/work/home
export TMPDIR=/tmp
export GIT_TERMINAL_PROMPT=0
mkdir -p /work/home

MANIFEST=/archive/testing/harness/helper-evidence-check/Cargo.toml
phase cargo-test-offline cargo test --locked --offline --manifest-path "$MANIFEST" -- --test-threads=4
if [ "$(cat "$OUT/phases/cargo-test-offline.exit")" != "0" ]; then
  phase cargo-test-online cargo test --locked --manifest-path "$MANIFEST" -- --test-threads=4
fi

if rustup component list --toolchain 1.94.0 2>/dev/null | awk '$1=="clippy" && /installed/ {ok=1} END{exit ok?0:1}'; then
  echo 0 >"$OUT/phases/clippy-component.exit"
  echo "clippy already installed" >"$OUT/phases/clippy-component.out"
else
  phase clippy-component rustup component add clippy --toolchain 1.94.0
fi
phase clippy-offline cargo clippy --locked --offline --manifest-path "$MANIFEST" -- -D warnings
if [ "$(cat "$OUT/phases/clippy-offline.exit")" != "0" ]; then
  phase clippy-online cargo clippy --locked --manifest-path "$MANIFEST" -- -D warnings
fi

BIN=/target/checker/debug/helper-evidence-check
if [ ! -x "$BIN" ]; then
  echo "checker binary missing" | tee "$OUT/phases/oracle-build.err"
  echo 127 >"$OUT/phases/oracle-build.exit"
  exit 127
fi
sha256sum "$BIN" | tee "$OUT/phases/checker-binary.sha256"

export CARGO_TARGET_DIR=/target/oracle
phase oracle-build-offline cargo build --locked --offline --manifest-path /review/oracle/Cargo.toml
if [ "$(cat "$OUT/phases/oracle-build-offline.exit")" != "0" ]; then
  phase oracle-build-online cargo build --locked --manifest-path /review/oracle/Cargo.toml
fi
ORACLE=/target/oracle/debug/helper-license-oracle
if [ ! -x "$ORACLE" ]; then
  echo "oracle binary missing" | tee "$OUT/phases/oracle-run.err"
  echo 127 >"$OUT/phases/oracle-run.exit"
  exit 127
fi
phase oracle-run "$ORACLE" "$BIN" /archive /review/linux/probe
sha256sum "$ORACLE" | tee "$OUT/phases/oracle-binary.sha256"

worst=0
for f in "$OUT"/phases/*.exit; do
  code=$(tr -d '[:space:]' <"$f")
  case "$code" in
    ''|*[!0-9]*) worst=1 ;;
    *) if [ "$code" -gt "$worst" ]; then worst=$code; fi ;;
  esac
done
# Offline misses are recorded and are not the smoke result when the online retry is 0.
# worst above includes offline failures. Publish an explicit smoke result separately.
smoke=0
for name in identity pin cargo-test-offline clippy-component clippy-offline oracle-build-offline oracle-run; do
  if [ -f "$OUT/phases/${name}.exit" ]; then
    code=$(tr -d '[:space:]' <"$OUT/phases/${name}.exit")
    if [ "$name" = "cargo-test-offline" ] && [ "$code" != "0" ] && [ -f "$OUT/phases/cargo-test-online.exit" ]; then
      code=$(tr -d '[:space:]' <"$OUT/phases/cargo-test-online.exit")
    fi
    if [ "$name" = "clippy-offline" ] && [ "$code" != "0" ] && [ -f "$OUT/phases/clippy-online.exit" ]; then
      code=$(tr -d '[:space:]' <"$OUT/phases/clippy-online.exit")
    fi
    if [ "$name" = "oracle-build-offline" ] && [ "$code" != "0" ] && [ -f "$OUT/phases/oracle-build-online.exit" ]; then
      code=$(tr -d '[:space:]' <"$OUT/phases/oracle-build-online.exit")
    fi
    if [ "$code" != "0" ]; then smoke=1; fi
  else
    smoke=1
  fi
done
echo "$smoke" >"$OUT/phases/smoke.exit"
echo "$worst" >"$OUT/phases/worst-including-offline.exit"
echo "SMOKE $smoke"
exit "$smoke"
