#!/bin/bash
# Smoke, clippy, the author's live test, and the independent CSRF probe.
# No cargo update. No lock generation. No login shell.
set -u
export PATH="/usr/local/cargo/bin:${PATH}"
export RUSTUP_HOME=/usr/local/rustup
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
RAW=/evidence/linux/raw
mkdir -p "$RAW" /evidence/linux/smoke /evidence/linux/proof /evidence/linux/csrf
phase() {
  local name="$1"
  shift
  set +e
  "$@" > "$RAW/${name}.log" 2>&1
  local code=$?
  set -e
  printf '%s\n' "$code" > "$RAW/${name}.exit"
  printf '%s %s\n' "$name" "$code" >> "$RAW/raw-exits.txt"
  return 0
}

phase rustc-version rustc --version
phase cargo-version cargo --version
phase rustup-show rustup show active-toolchain
{
  echo "smoke before clippy and tests"
  cat "$RAW/rustc-version.log"
} > /evidence/linux/smoke/console.log
cp "$RAW/rustc-version.exit" /evidence/linux/smoke/exit.txt
if [ "$(cat "$RAW/rustc-version.exit")" != 0 ]; then
  echo SMOKE_FAILED
  exit 1
fi

cd /repo/tools/release-control-api
if [ ! -f Cargo.lock ]; then
  echo "child lock missing; refuse generate" > "$RAW/lock-present.log"
  printf '%s\n' 2 > "$RAW/lock-present.exit"
  exit 2
fi
printf '%s\n' 0 > "$RAW/lock-present.exit"
phase lock-checksum awk '
  FNR==NR { if ($0 ~ /checksum = /) root[$0]=1; next }
  $0 ~ /checksum = / && !($0 in root) { print "lock_checksum_not_in_root " $0; bad=1 }
  END { exit bad }
' /repo/Cargo.lock Cargo.lock
if [ "$(cat "$RAW/lock-checksum.exit")" != 0 ]; then
  echo LOCK_CHECK_FAILED
  exit 1
fi

phase cargo-fetch cargo fetch --locked
if [ "$(cat "$RAW/cargo-fetch.exit")" != 0 ]; then
  echo FETCH_FAILED
  exit 1
fi
phase rustup-clippy rustup component add clippy --toolchain 1.94.0
cp "$RAW/rustup-clippy.log" /evidence/linux/build-1/components-before.txt 2>/dev/null || mkdir -p /evidence/linux/build-1
rustup component list --toolchain 1.94.0 > /evidence/linux/build-1/components-before.txt || true
if [ "$(cat "$RAW/rustup-clippy.exit")" != 0 ]; then
  echo CLIPPY_COMPONENT_FAILED
  exit 1
fi

rm -rf /target/id-probe
cp -a /repo/tools/control-identity /target/id-probe
cp /evidence/probe/a404_csrf_probe.rs /target/id-probe/tests/a404_csrf_probe.rs
phase identity-fetch cargo fetch --locked --manifest-path /target/id-probe/Cargo.toml
if [ "$(cat "$RAW/identity-fetch.exit")" != 0 ]; then
  echo IDENTITY_FETCH_FAILED
  exit 1
fi

export CARGO_NET_OFFLINE=true
printf '%s\n' 'cargo-update-invoked=no' > "$RAW/cargo-update.txt"
phase cargo-clippy cargo clippy --locked --offline --all-targets -- -D warnings
phase csrf-probe cargo test --locked --offline --manifest-path /target/id-probe/Cargo.toml --test a404_csrf_probe -- --test-threads=1 --exact csrf_rotation_and_consume_side_effect
phase cargo-test cargo test --locked --offline -- --test-threads=1
phase observe cargo test --locked --offline --manifest-path /target/id-probe/Cargo.toml --test a404_csrf_probe -- --test-threads=1 --exact observe_product_database

failed=0
for name in cargo-clippy csrf-probe cargo-test observe; do
  code=$(cat "$RAW/${name}.exit")
  if [ "$code" != 0 ]; then
    failed=1
  fi
done
if [ "$failed" != 0 ]; then
  echo BUILD_FAILED
  exit 1
fi
echo BUILD_OK
