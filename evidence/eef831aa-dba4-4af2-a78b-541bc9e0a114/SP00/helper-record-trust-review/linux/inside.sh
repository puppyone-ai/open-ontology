#!/bin/bash
# Focused Linux arm64 Rust 1.94 smoke of the fixed archive. No workspace build.
set -u
OUT=/review/linux
mkdir -p "$OUT/phases" "$OUT/probe" /work/home
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
  command -v rustc
  command -v cargo
  sha256sum "$(command -v rustc)" || true
  sha256sum "$(command -v cargo)" || true
  command -v sha256sum
  sha256sum /usr/bin/sha256sum || true
  sha256sum /archive/testing/harness/containment-supervisor/Cargo.lock
  sha256sum /archive/ci/deny.toml
  sha256sum /archive/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-audit.out
  sha256sum /review/probe/src/main.rs
  sha256sum /review/expected-outcomes.tsv
  sha256sum /review/age-floor-preregistration.tsv
} | tee "$OUT/phases/identity.out"
echo 0 >"$OUT/phases/identity.exit"

pin_ok=0
while read -r hash path; do
  [ -n "$hash" ] || continue
  got=$(sha256sum "/archive/$path" | awk '{print $1}')
  if [ "$got" != "$hash" ]; then
    echo "mismatch $path got=$got want=$hash"
    pin_ok=3
  fi
done <"$OUT/source-pins.sha256"
LOCK=$(sha256sum /archive/testing/harness/containment-supervisor/Cargo.lock | awk '{print $1}')
AUDIT=$(sha256sum /archive/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-audit.out | awk '{print $1}')
if [ "$LOCK" != "14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1" ]; then
  echo "lock pin mismatch $LOCK"
  pin_ok=3
fi
if [ "$AUDIT" != "f64e43716f72c173879cb30130f73df0f1c802ac32960eb6c3955c15c2209f81" ]; then
  echo "audit pin mismatch $AUDIT"
  pin_ok=3
fi
if grep -F -q '"status"' /archive/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a101/SP00/helper-supply-tools/raw/scan-audit.out; then
  echo "retained audit contains a status field"
  pin_ok=3
fi
echo "$pin_ok" >"$OUT/phases/pin.exit"
if [ "$pin_ok" != "0" ]; then
  echo "PHASE pin exit $pin_ok"
fi

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

phase probe-build cargo build --locked --offline --manifest-path /review/probe/Cargo.toml --target-dir /target/probe
PROBE=/target/probe/debug/helper-trust-probe
if [ -x "$PROBE" ]; then
  sha256sum "$PROBE" | tee "$OUT/phases/probe-binary.sha256"
  phase probe "$PROBE" \
    --archive /archive \
    --checker "$BIN" \
    --expected /review/expected-outcomes.tsv \
    --ages /review/age-floor-preregistration.tsv \
    --out /review/linux/probe
else
  echo "probe binary missing" | tee "$OUT/phases/probe.err"
  echo 101 >"$OUT/phases/probe.exit"
fi

phase clippy-all-targets cargo clippy --locked --offline --manifest-path "$MANIFEST" --all-targets -- -D warnings

smoke=0
for name in identity pin cargo-test-offline clippy-component clippy-offline clippy-tests-offline probe; do
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
if [ -f "$OUT/phases/clippy-all-targets.exit" ]; then
  echo "PHASE clippy-all-targets exit $(tr -d '[:space:]' <"$OUT/phases/clippy-all-targets.exit") not-in-smoke"
fi
exit "$smoke"
