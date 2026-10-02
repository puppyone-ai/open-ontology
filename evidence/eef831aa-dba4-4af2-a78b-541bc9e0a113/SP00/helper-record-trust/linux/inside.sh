#!/bin/bash
# One a113 checker smoke inside Linux arm64 Rust 1.94. No workspace build.
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
  sha256sum /archive/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-audit.out
} | tee "$OUT/phases/identity.out"
echo 0 >"$OUT/phases/identity.exit"

LOCK=$(sha256sum /archive/testing/harness/containment-supervisor/Cargo.lock | awk '{print $1}')
AUDIT=$(sha256sum /archive/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-audit.out | awk '{print $1}')
if [ "$LOCK" != "14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1" ]; then
  echo "lock pin mismatch $LOCK" | tee "$OUT/phases/pin.err"
  echo 3 >"$OUT/phases/pin.exit"
  exit 3
fi
if [ "$AUDIT" != "f64e43716f72c173879cb30130f73df0f1c802ac32960eb6c3955c15c2209f81" ]; then
  echo "audit pin mismatch $AUDIT" | tee "$OUT/phases/pin.err"
  echo 3 >"$OUT/phases/pin.exit"
  exit 3
fi
if grep -q '"status"' /archive/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-audit.out; then
  echo "retained audit gained a status field" | tee "$OUT/phases/pin.err"
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
export OO_A113_CASE_LOG=/review/linux/probe/trust-cases.tsv
mkdir -p /work/home "$OUT/probe"
: >"$OO_A113_CASE_LOG"

MANIFEST=/archive/testing/harness/helper-evidence-check/Cargo.toml
phase cargo-test-offline cargo test --locked --offline --manifest-path "$MANIFEST" -- --test-threads=4 --nocapture
if [ "$(cat "$OUT/phases/cargo-test-offline.exit")" != "0" ]; then
  phase cargo-test-online cargo test --locked --manifest-path "$MANIFEST" -- --test-threads=4 --nocapture
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
phase clippy-tests-offline cargo clippy --locked --offline --manifest-path "$MANIFEST" --test trust --test closure -- -D warnings
if [ "$(cat "$OUT/phases/clippy-tests-offline.exit")" != "0" ]; then
  phase clippy-tests-online cargo clippy --locked --manifest-path "$MANIFEST" --test trust --test closure -- -D warnings
fi

BIN=/target/checker/debug/helper-evidence-check
if [ -x "$BIN" ]; then
  sha256sum "$BIN" | tee "$OUT/phases/checker-binary.sha256"
else
  echo "checker binary missing" | tee "$OUT/phases/checker-binary.sha256"
fi

smoke=0
for name in identity pin cargo-test-offline clippy-component clippy-offline clippy-tests-offline; do
  code=$(tr -d '[:space:]' <"$OUT/phases/${name}.exit")
  if [ "$name" = "cargo-test-offline" ] && [ "$code" != "0" ] && [ -f "$OUT/phases/cargo-test-online.exit" ]; then
    code=$(tr -d '[:space:]' <"$OUT/phases/cargo-test-online.exit")
  fi
  if [ "$name" = "clippy-offline" ] && [ "$code" != "0" ] && [ -f "$OUT/phases/clippy-online.exit" ]; then
    code=$(tr -d '[:space:]' <"$OUT/phases/clippy-online.exit")
  fi
  if [ "$name" = "clippy-tests-offline" ] && [ "$code" != "0" ] && [ -f "$OUT/phases/clippy-tests-online.exit" ]; then
    code=$(tr -d '[:space:]' <"$OUT/phases/clippy-tests-online.exit")
  fi
  if [ "$code" != "0" ]; then
    smoke=$code
  fi
done
echo "$smoke" >"$OUT/phases/smoke.exit"
echo "PHASE smoke exit $smoke"
exit "$smoke"
