#!/bin/bash
# Build and test session-store against the fixed reducer tree and one private PostgreSQL.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1

echo "=== identity ==="
rustc -vV
cargo -V
uname -a
id

echo "=== archive gate ==="
if [ ! -f /src/tools/session-transition/src/reduce.rs ]; then
  echo "fixed archive missing the reducer" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ ! -f /src/tools/session-store/src/store.rs ]; then
  echo "session-store overlay missing" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ -f /src/rust-toolchain.toml ]; then
  echo "toolchain file must stay out of this archive" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/archive.exit

echo "=== rustc version gate ==="
if ! rustc -vV | grep -q '^release: 1.94.0$'; then
  echo "toolchain is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
if ! rustc -vV | grep -q '^host: aarch64-unknown-linux-gnu$'; then
  echo "host is not linux arm64" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/version.exit

rm -rf /work
mkdir -p /work
cp -R /src/tools/session-transition /work/session-transition
cp -R /src/tools/session-store /work/session-store
cd /work/session-store
find Cargo.toml CONTRACT.md migrations src tests -type f | sort | xargs sha256sum > /evidence/source-sha256.txt

parse_registry() {
  awk 'BEGIN{RS=""; FS="\n"} {
    name=""; ver=""; sum=""; src="";
    for (i = 1; i <= NF; i++) {
      line = $i
      if (line ~ /^name = "/) { name = line; sub(/^name = "/, "", name); sub(/"$/, "", name) }
      if (line ~ /^version = "/) { ver = line; sub(/^version = "/, "", ver); sub(/"$/, "", ver) }
      if (line ~ /^source = "/) { src = line }
      if (line ~ /^checksum = "/) { sum = line; sub(/^checksum = "/, "", sum); sub(/"$/, "", sum) }
    }
    if (src ~ /^source = "registry\+/) printf "%s\t%s\t%s\n", name, ver, sum
  }' "$1"
}

echo "=== lock ==="
parse_registry /src/root-Cargo.lock | sort -u > /tmp/root.tsv
if [ ! -f Cargo.lock ]; then
  set +e
  cargo generate-lockfile > /evidence/lockgen.stdout 2> /evidence/lockgen.stderr
  lockgen_exit=$?
  set -e
  printf '%s\n' "$lockgen_exit" > /evidence/lockgen.exit
  echo "PHASE lockgen_exit=$lockgen_exit"
  if [ "$lockgen_exit" -ne 0 ]; then
    exit "$lockgen_exit"
  fi
else
  printf '%s\n' 0 > /evidence/lockgen.exit
fi

round=0
while [ "$round" -lt 8 ]; do
  parse_registry Cargo.lock | sort -u > /tmp/new.tsv
  : > /tmp/drift.tsv
  while IFS=$(printf '\t') read -r name ver sum; do
    if ! awk -F '\t' -v s="$sum" '$3 == s { found = 1 } END { exit !found }' /tmp/root.tsv; then
      printf '%s\t%s\t%s\n' "$name" "$ver" "$sum" >> /tmp/drift.tsv
    fi
  done < /tmp/new.tsv
  if [ ! -s /tmp/drift.tsv ]; then
    break
  fi
  round=$((round + 1))
  echo "PHASE lock_reconcile_round=$round"
  while IFS=$(printf '\t') read -r name ver sum; do
    if ! awk -F '\t' -v n="$name" '$1 == n { found = 1 } END { exit !found }' /tmp/root.tsv; then
      echo "crate absent from the fixed archive lock: $name $ver" >&2
      cp Cargo.lock /evidence/Cargo.lock
      printf '%s\n' 4 > /evidence/lock.exit
      exit 4
    fi
    precise=$(awk -F '\t' -v n="$name" '$1 == n { print $2 }' /tmp/root.tsv | sort -V | tail -1)
    echo "pin $name from $ver to $precise"
    if ! cargo update -p "$name" --precise "$precise"; then
      cp Cargo.lock /evidence/Cargo.lock
      printf '%s\n' 4 > /evidence/lock.exit
      exit 4
    fi
  done < /tmp/drift.tsv
done
parse_registry Cargo.lock | sort -u > /tmp/new.tsv
if [ -s /tmp/drift.tsv ]; then
  echo "lock still drifts from the fixed archive" >&2
  cp /tmp/drift.tsv /evidence/lock-drift.tsv
  cp Cargo.lock /evidence/Cargo.lock
  printf '%s\n' 4 > /evidence/lock.exit
  exit 4
fi

expect_pin() {
  if ! awk -F '\t' -v n="$1" -v v="$2" -v s="$3" '$1 == n && $2 == v && $3 == s { found = 1 } END { exit !found }' /tmp/new.tsv; then
    echo "direct pin mismatch $1" >&2
    printf '%s\n' 4 > /evidence/lock.exit
    exit 4
  fi
}
expect_pin tokio 1.53.1 202caea871b69668250d242070849eb495be178ed697a3e98aebce5bc81a0bed
expect_pin tokio-postgres 0.7.18 a528f7d280f6d5b9cd149635c8705b0dd049754bc67d81d31fa25169a93809d3
expect_pin rustls 0.23.45 0d41d731c7d2f962d1ccc364cec258de3c0e93b38c2fb3ba97ac74513048d634
expect_pin tokio-rustls 0.26.5 b0c85f2c3ef0b1cd58b36682f4b17aaa995f0e5db534d85692b4903abce21f67
expect_pin tokio-postgres-rustls 0.14.0 4c2ad44aa0ae96db89c4742212ed41645b2f597311ff6e1945542a4d9fadc2fb
expect_pin postgres-types 0.2.14 851ca9db4932932d69f3ea811b1abe63087a0f740a47692619dd40d4899b68be
expect_pin postgres-protocol 0.6.12 08808e3c483c46e999108051c78334f473d5adb59d78bb80a1268c7e6aa6c514
expect_pin serde 1.0.229 4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba
expect_pin serde_json 1.0.151 c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14
expect_pin serde_core 1.0.229 67dca2c9c51e58a4791a4b1ed58308b39c64224d349a935ab5039aa360942a48
expect_pin serde_derive 1.0.229 e7a5d71263a5a7d47b41f6b3f06ba276f10cc18b0931f1799f710578e2309348
cp Cargo.lock /evidence/Cargo.lock
printf '%s\n' 0 > /evidence/lock.exit
echo "PHASE lock_exit=0"

echo "=== cargo fetch locked ==="
set +e
cargo fetch --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
set -e
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi

echo "=== cargo test locked offline ==="
set +e
cargo test --locked --offline -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

echo "=== clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-add.stdout 2> /evidence/clippy-add.stderr
add_exit=$?
set -e
printf '%s\n' "$add_exit" > /evidence/clippy-add.exit
echo "PHASE clippy_add_exit=$add_exit"
if [ "$add_exit" -ne 0 ]; then
  exit "$add_exit"
fi
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
exit "$clippy_exit"
