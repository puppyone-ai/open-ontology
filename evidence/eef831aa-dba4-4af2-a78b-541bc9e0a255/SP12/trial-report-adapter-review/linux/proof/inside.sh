#!/bin/bash
# Locked offline checks, clippy, and the independent runner-adapter-statistics driver.
# Product sources stay on the readonly archive. No cargo update. No PostgreSQL.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
cd /src
printf '%s\n' no > /evidence/cargo-update-invoked.txt

echo "=== identity ==="
rustc -vV
cargo -V
rustup show
rustc_version=$(rustc -vV | awk '/^release:/{print $2}')
printf '%s\n' "$rustc_version" > /evidence/rustc-release.txt
if [ "$rustc_version" != "1.94.0" ]; then
  echo "rustc is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/rustc-release.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/rustc-release.exit
if [ -f rust-toolchain.toml ]; then
  echo "repo toolchain file still active" >&2
  printf '%s\n' 2 > /evidence/toolchain-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/toolchain-absent.exit
if grep -R -q 'A255-PASS' benchmarks/trial-report-adapter; then
  echo "review marker leaked into the sealed adapter" >&2
  printf '%s\n' 2 > /evidence/harness-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/harness-absent.exit

echo "=== source hash ==="
cd /src/benchmarks/trial-report-adapter
if ! sha256sum -c /evidence/source-sha256.txt > /evidence/source-sha256.check 2>&1; then
  echo "sealed sources drifted" >&2
  printf '%s\n' 2 > /evidence/source-sha256.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/source-sha256.exit
cd /src

echo "=== lock pins ==="
awk '
  function flush() {
    if (checksum != "") print name " " version " " checksum
    name = ""
    version = ""
    checksum = ""
  }
  /^\[\[package\]\]/ { flush(); next }
  $1 == "name" { gsub(/"/, "", $3); name = $3 }
  $1 == "version" { gsub(/"/, "", $3); version = $3 }
  $1 == "checksum" { gsub(/"/, "", $3); checksum = $3 }
  END { flush() }
' /src/Cargo.lock | sort > /tmp/root-pins.txt
awk '
  function flush() {
    if (checksum != "") print name " " version " " checksum
    name = ""
    version = ""
    checksum = ""
  }
  /^\[\[package\]\]/ { flush(); next }
  $1 == "name" { gsub(/"/, "", $3); name = $3 }
  $1 == "version" { gsub(/"/, "", $3); version = $3 }
  $1 == "checksum" { gsub(/"/, "", $3); checksum = $3 }
  END { flush() }
' benchmarks/trial-report-adapter/Cargo.lock | sort > /tmp/crate-pins.txt
comm -13 /tmp/root-pins.txt /tmp/crate-pins.txt > /evidence/lock-drift.txt
if [ -s /evidence/lock-drift.txt ]; then
  echo "adapter lock drifted from the fixed registry pins" >&2
  printf '%s\n' 2 > /evidence/lock-drift.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/lock-drift.exit
grep 'serde 1.0.229 4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba' /tmp/crate-pins.txt || exit 2
grep 'serde_json 1.0.151 c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14' /tmp/crate-pins.txt || exit 2

build_one() {
  local name="$1"
  local manifest="$2"
  local bin="$3"
  echo "=== cargo build $name ==="
  set +e
  cargo build --locked --offline --manifest-path "$manifest" --bin "$bin" \
    > "/evidence/${name}-build.stdout" 2> "/evidence/${name}-build.stderr"
  local code=$?
  set +e
  printf '%s\n' "$code" > "/evidence/${name}-build.exit"
  echo "PHASE ${name}_build_exit=$code"
  return "$code"
}

worst=0
note() {
  local code="$1"
  if [ "$worst" -eq 0 ] && [ "$code" -ne 0 ]; then
    worst=$code
  fi
}

build_one sql-runner benchmarks/sql-runner/Cargo.toml sql-runner
note $?
build_one trial-statistics benchmarks/trial-statistics/Cargo.toml trial-statistics
note $?
build_one adapter benchmarks/trial-report-adapter/Cargo.toml trial-report-adapter
note $?

echo "=== cargo test ==="
mkdir -p /evidence/sealed-proof
export OO_SQL_RUNNER=/target/debug/sql-runner
export OO_TRIAL_STATISTICS=/target/debug/trial-statistics
export OO_PROOF_DIR=/evidence/sealed-proof
set +e
cargo test --locked --offline --manifest-path benchmarks/trial-report-adapter/Cargo.toml -- --test-threads=1 \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
note "$test_exit"

echo "=== rustup components ==="
set +e
rustup component add rustfmt clippy --toolchain 1.94.0 > /evidence/component.stdout 2> /evidence/component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/component.exit
echo "PHASE component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/fmt.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  note "$component_exit"
else
  echo "=== cargo fmt ==="
  set +e
  cargo fmt --manifest-path benchmarks/trial-report-adapter/Cargo.toml -- --check \
    > /evidence/fmt.stdout 2> /evidence/fmt.stderr
  fmt_exit=$?
  set -e
  printf '%s\n' "$fmt_exit" > /evidence/fmt.exit
  echo "PHASE fmt_exit=$fmt_exit"
  note "$fmt_exit"

  echo "=== cargo clippy ==="
  set +e
  cargo clippy --locked --offline --manifest-path benchmarks/trial-report-adapter/Cargo.toml --all-targets -- -D warnings \
    > /evidence/clippy.stdout 2> /evidence/clippy.stderr
  clippy_exit=$?
  set -e
  printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
  echo "PHASE clippy_exit=$clippy_exit"
  note "$clippy_exit"
fi

echo "=== independent drive ==="
mkdir -p /evidence/independent
if [ ! -x /target/debug/sql-runner ] || [ ! -x /target/debug/trial-statistics ] || [ ! -x /target/debug/trial-report-adapter ]; then
  echo "a consumed binary is missing" >&2
  printf '%s\n' 2 > /evidence/drive.exit
  note 2
else
  set +e
  rustc --edition 2021 -D unsafe_code -C debuginfo=0 -o /target/a255-drive /review/drive.rs \
    > /evidence/drive-rustc.stdout 2> /evidence/drive-rustc.stderr
  rustc_exit=$?
  set -e
  printf '%s\n' "$rustc_exit" > /evidence/drive-rustc.exit
  if [ "$rustc_exit" -ne 0 ]; then
    printf '%s\n' "$rustc_exit" > /evidence/drive.exit
    note "$rustc_exit"
  else
    set +e
    OO_SQL_RUNNER=/target/debug/sql-runner \
      OO_TRIAL_STATISTICS=/target/debug/trial-statistics \
      OO_ADAPTER=/target/debug/trial-report-adapter \
      OO_PROOF_DIR=/evidence/independent \
      OO_SCRATCH=/target/a255-scratch \
      OO_FIXTURES=/src/benchmarks/sql-runner/fixtures \
      OO_STUB=/review/stub.rs \
      /target/a255-drive > /evidence/drive.stdout 2> /evidence/drive.stderr
    drive_exit=$?
    set -e
    printf '%s\n' "$drive_exit" > /evidence/drive.exit
    echo "PHASE drive_exit=$drive_exit"
    note "$drive_exit"
  fi
fi

echo "worst_phase_exit=$worst"
exit "$worst"
