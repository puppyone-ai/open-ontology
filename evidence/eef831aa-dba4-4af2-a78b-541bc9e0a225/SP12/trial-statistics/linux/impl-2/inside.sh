#!/bin/bash
# Generate the standalone lock from the cached registry and test trial statistics.
# Origin: a212 impl-5 inside 0ae42f942e075be4171646d373c49dcfe1029817
# evidence/eef831aa-dba4-4af2-a78b-541bc9e0a212/SP12/result-comparator/linux/impl-5/inside.sh
# sha256 525ca23b801420a8c9cf4764f6fc8856d631cf2d00ccfee3ec10eb5886260fb6
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
cd /src

echo "=== identity ==="
rustc -vV
cargo -V
rustup show
printf '%s\n' "$(rustc -vV | awk '/^release:/{print $2}')" > /evidence/rustc-release.txt
if [ "$(cat /evidence/rustc-release.txt)" != "1.94.0" ]; then
  echo "rustc is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/rustc-release.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/rustc-release.exit
if [ -f rust-toolchain.toml ]; then
  echo "repo toolchain file still active" >&2
  exit 2
fi

cd /src/benchmarks/trial-statistics
if ! sha256sum -c /evidence/source-sha256.txt > /evidence/source-sha256.check 2>&1; then
  echo "copied sources drifted" >&2
  printf '%s\n' 2 > /evidence/source-sha256.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/source-sha256.exit

echo "=== cargo generate-lockfile ==="
set +e
cargo generate-lockfile --offline > /evidence/lock.stdout 2> /evidence/lock.stderr
lock_exit=$?
set -e
if [ "$lock_exit" -ne 0 ]; then
  echo "=== cargo generate-lockfile online ==="
  set +e
  cargo generate-lockfile > /evidence/lock-online.stdout 2> /evidence/lock-online.stderr
  lock_exit=$?
  set -e
fi
printf '%s\n' "$lock_exit" > /evidence/lock.exit
echo "PHASE lock_exit=$lock_exit"
if [ "$lock_exit" -ne 0 ]; then
  exit "$lock_exit"
fi

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
' Cargo.lock | sort > /tmp/crate-pins.txt
comm -13 /tmp/root-pins.txt /tmp/crate-pins.txt > /evidence/lock-drift.txt
if [ -s /evidence/lock-drift.txt ]; then
  echo "trial-statistics lock drifted from the fixed registry pins" >&2
  printf '%s\n' 2 > /evidence/lock-drift.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/lock-drift.exit
grep 'serde 1.0.229 4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba' /tmp/crate-pins.txt
grep 'serde_json 1.0.151 c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14' /tmp/crate-pins.txt

echo "=== cargo test ==="
set +e
cargo test --locked --offline -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"

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
  exit "$component_exit"
fi

echo "=== cargo fmt ==="
set +e
cargo fmt -- --check > /evidence/fmt.stdout 2> /evidence/fmt.stderr
fmt_exit=$?
set -e
printf '%s\n' "$fmt_exit" > /evidence/fmt.exit
echo "PHASE fmt_exit=$fmt_exit"

echo "=== cargo clippy ==="
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

echo "=== cli fixtures ==="
BIN=/target/debug/trial-statistics
mkdir -p /evidence/cli
cli_exit=0
run_report() {
  local name="$1"
  local want_exit="$2"
  set +e
  "$BIN" < "fixtures/${name}.json" > "/evidence/cli/${name}.stdout" 2> "/evidence/cli/${name}.stderr"
  local code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/cli/${name}.exit"
  if [ "$code" -ne "$want_exit" ]; then
    echo "exit $code on $name" >&2
    cli_exit=1
  fi
  if [ -s "/evidence/cli/${name}.stderr" ]; then
    echo "stderr on $name" >&2
    cli_exit=1
  fi
  if ! cmp -s "/evidence/cli/${name}.stdout" "fixtures/${name}.report.json"; then
    echo "stdout mismatch $name" >&2
    cli_exit=1
  fi
}
run_config() {
  local name="$1"
  local want="$2"
  set +e
  "$BIN" < "fixtures/${name}.json" > "/evidence/cli/${name}.stdout" 2> "/evidence/cli/${name}.stderr"
  local code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/cli/${name}.exit"
  if [ "$code" -ne 2 ]; then
    echo "exit $code on $name" >&2
    cli_exit=1
  fi
  if [ -s "/evidence/cli/${name}.stdout" ]; then
    echo "stdout on config $name" >&2
    cli_exit=1
  fi
  printf '%s\n' "$want" > "/evidence/cli/${name}.want"
  if ! cmp -s "/evidence/cli/${name}.stderr" "/evidence/cli/${name}.want"; then
    echo "stderr mismatch $name" >&2
    cli_exit=1
  fi
}
if [ ! -x "$BIN" ]; then
  echo "binary missing" >&2
  cli_exit=2
else
  run_report k3-passed 0
  run_report k3-reannotation 1
  run_report k3-not-pass-at 0
  set +e
  "$BIN" < fixtures/k3-passed.json > /evidence/cli/k3-passed-again.stdout 2> /evidence/cli/k3-passed-again.stderr
  again=$?
  set -e
  printf '%s\n' "$again" > /evidence/cli/k3-passed-again.exit
  if [ "$again" -ne 0 ] || ! cmp -s /evidence/cli/k3-passed.stdout /evidence/cli/k3-passed-again.stdout; then
    echo "reproducibility failed" >&2
    cli_exit=1
  fi
  run_config mixed-tuple "trial-statistics: tuple /variants/1/tuple/model"
  run_config missing-trial "trial-statistics: trials /variants/1/questions/1/trials"
  run_config duplicate-key "trial-statistics: json"
fi
printf '%s\n' "$cli_exit" > /evidence/cli.exit
echo "PHASE cli_exit=$cli_exit"

worst=0
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$fmt_exit" -ne 0 ]; then worst=$fmt_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
if [ "$cli_exit" -ne 0 ]; then worst=$cli_exit; fi
echo "worst_phase_exit=$worst"
exit "$worst"
