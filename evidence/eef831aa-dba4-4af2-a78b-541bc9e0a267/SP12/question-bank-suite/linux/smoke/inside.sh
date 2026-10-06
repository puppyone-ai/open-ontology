#!/bin/bash
# Focused question-bank and sql-runner smoke on the sealed 567d621e archive.
# question-bank-suite is not in this archive. No PostgreSQL.
# Origin: a256 smoke inside, which records a204/a199.
# evidence/eef831aa-dba4-4af2-a78b-541bc9e0a256/SP12/question-bank/linux/smoke/inside.sh
# sha256 975b2623df4b8db47b693343482518a22ce0da3c7cc178e08da861794dcb6ef5
# Delta: fixed commit already contains question-bank and sql-runner. One decimal
# bank test and the sql-runner FIPS vectors. No new crate.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
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
echo "=== toolchain file absent ==="
if [ -f rust-toolchain.toml ]; then
  echo "repo toolchain file still active" >&2
  printf '%s\n' 2 > /evidence/toolchain-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/toolchain-absent.exit
if [ ! -f rust-toolchain.toml.repo ]; then
  echo "repo toolchain file missing from archive copy" >&2
  printf '%s\n' 2 > /evidence/toolchain-absent.exit
  exit 2
fi

rustc_version=$(rustc -vV | awk '/^release:/{print $2}')
printf '%s\n' "$rustc_version" > /evidence/rustc-release.txt
if [ "$rustc_version" != "1.94.0" ]; then
  echo "rustc is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/rustc-release.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/rustc-release.exit

echo "=== pins ==="
sha256sum \
  benchmarks/question-bank/src/lib.rs \
  benchmarks/question-bank/Cargo.toml \
  benchmarks/question-bank/Cargo.lock \
  benchmarks/sql-runner/src/lib.rs \
  benchmarks/sql-runner/Cargo.toml \
  benchmarks/sql-runner/Cargo.lock \
  crates/oo-core/Cargo.toml \
  > /evidence/archive-sha256.txt
if [ -e benchmarks/question-bank-suite ]; then
  echo "question-bank-suite present during smoke" >&2
  printf '%s\n' 2 > /evidence/suite-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/suite-absent.exit
pin_lock() {
  local file="$1"
  local tag="$2"
  awk -v tag="$tag" '
    $0=="name = \"serde\"" {p=1}
    $0=="name = \"serde_json\"" {p=2}
    p==1 && $0 ~ /^version = / {print tag, "serde", $0}
    p==1 && $0 ~ /^checksum = / {print tag, "serde", $0; p=0}
    p==2 && $0 ~ /^version = / {print tag, "serde_json", $0}
    p==2 && $0 ~ /^checksum = / {print tag, "serde_json", $0; p=0}
  ' "$file"
}
pin_lock benchmarks/question-bank/Cargo.lock question-bank > /evidence/lock-pins.txt
pin_lock benchmarks/sql-runner/Cargo.lock sql-runner >> /evidence/lock-pins.txt
if ! grep -q 'question-bank serde version = "1.0.229"' /evidence/lock-pins.txt \
  || ! grep -q 'question-bank serde checksum = "4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba"' /evidence/lock-pins.txt \
  || ! grep -q 'question-bank serde_json version = "1.0.151"' /evidence/lock-pins.txt \
  || ! grep -q 'question-bank serde_json checksum = "c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14"' /evidence/lock-pins.txt \
  || ! grep -q 'sql-runner serde version = "1.0.229"' /evidence/lock-pins.txt \
  || ! grep -q 'sql-runner serde_json version = "1.0.151"' /evidence/lock-pins.txt
then
  echo "lock pins drifted" >&2
  printf '%s\n' 2 > /evidence/lock-pins.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/lock-pins.exit

fetch_one() {
  local manifest="$1"
  local name="$2"
  echo "=== cargo fetch $name locked offline ==="
  set +e
  cargo fetch --manifest-path "$manifest" --locked --offline > "/evidence/fetch-${name}-offline.stdout" 2> "/evidence/fetch-${name}-offline.stderr"
  local offline_exit=$?
  set -e
  printf '%s\n' "$offline_exit" > "/evidence/fetch-${name}-offline.exit"
  echo "PHASE ${name}_offline_fetch_exit=$offline_exit"
  local fetch_exit=$offline_exit
  if [ "$offline_exit" -ne 0 ]; then
    echo "=== cargo fetch $name locked online ==="
    set +e
    cargo fetch --manifest-path "$manifest" --locked > "/evidence/fetch-${name}.stdout" 2> "/evidence/fetch-${name}.stderr"
    fetch_exit=$?
    set -e
  else
    cp "/evidence/fetch-${name}-offline.stdout" "/evidence/fetch-${name}.stdout"
    cp "/evidence/fetch-${name}-offline.stderr" "/evidence/fetch-${name}.stderr"
  fi
  printf '%s\n' "$fetch_exit" > "/evidence/fetch-${name}.exit"
  echo "PHASE ${name}_fetch_exit=$fetch_exit"
  if [ "$fetch_exit" -ne 0 ]; then
    return "$fetch_exit"
  fi
}

fetch_one benchmarks/question-bank/Cargo.toml question-bank || exit $?
fetch_one benchmarks/sql-runner/Cargo.toml sql-runner || exit $?

echo "=== question-bank decimal test ==="
set +e
cargo test --manifest-path benchmarks/question-bank/Cargo.toml --locked --offline --test bank approved_decimal_stays_lexical_and_rescales_exactly -- --test-threads=1 \
  > /evidence/question-bank.stdout 2> /evidence/question-bank.stderr
bank_exit=$?
set -e
printf '%s\n' "$bank_exit" > /evidence/question-bank.exit
echo "PHASE question_bank_exit=$bank_exit"
if [ "$bank_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/sql-runner.exit
  exit "$bank_exit"
fi

echo "=== sql-runner fips vectors ==="
set +e
cargo test --manifest-path benchmarks/sql-runner/Cargo.toml --locked --offline --test sha256 -- --test-threads=1 \
  > /evidence/sql-runner.stdout 2> /evidence/sql-runner.stderr
runner_exit=$?
set -e
printf '%s\n' "$runner_exit" > /evidence/sql-runner.exit
echo "PHASE sql_runner_exit=$runner_exit"
exit "$runner_exit"
