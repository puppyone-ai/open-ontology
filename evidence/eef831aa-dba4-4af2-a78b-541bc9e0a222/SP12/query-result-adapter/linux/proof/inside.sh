#!/bin/bash
# Overlay proof. Fixed archive plus the query-result-adapter crate only.
# One Rust 1.94 builder. No cargo update. No apt.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/adapter
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
cd /src

phase() {
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "PHASE $1=$2"
}

mkdir -p /evidence/phases /cargo-home/bin /target/adapter /target/workspace
printf '%s\n' no > /evidence/cargo-update-invoked.txt

echo "=== identity ==="
rustc -vV
cargo -V
rustup show
uname -a
printf '%s\n' "$(rustc -vV | awk '/^release:/{print $2}')" > /evidence/rustc-release.txt
if [ "$(cat /evidence/rustc-release.txt)" != "1.94.0" ]; then
  echo "rustc is not 1.94.0" >&2
  phase rustc 2
  exit 2
fi
phase rustc 0
if [ -f rust-toolchain.toml ]; then
  echo "repo toolchain file still active" >&2
  phase toolchain 2
  exit 2
fi
phase toolchain 0

echo "=== tree ==="
diff -rq /archive /src > /evidence/tree-diff.txt || true
if grep -v -e 'Only in /archive: rust-toolchain.toml' \
  -e 'Only in /src: rust-toolchain.toml.repo' \
  -e 'Only in /src/benchmarks: query-result-adapter' \
  /evidence/tree-diff.txt | grep -q .; then
  echo "working tree is not the fixed archive plus the adapter" >&2
  phase tree 2
  exit 2
fi
phase tree 0

echo "=== source hash ==="
(
  cd /src/benchmarks/query-result-adapter
  find . -type f ! -name Cargo.lock ! -name '.DS_Store' | sort | xargs sha256sum
) > /evidence/source-sha256.txt
phase source-hash 0

if ! ( cd /src/benchmarks/query-result-adapter && sha256sum -c /evidence/source-sha256.txt > /evidence/source-sha256.check 2>&1 ); then
  echo "source hash could not be checked" >&2
  phase source-check 2
  exit 2
fi
phase source-check 0

echo "=== cargo generate-lockfile ==="
cd /src/benchmarks/query-result-adapter
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
phase lock "$lock_exit"
if [ "$lock_exit" -ne 0 ]; then
  exit "$lock_exit"
fi
cp Cargo.lock /evidence/Cargo.lock

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
  echo "adapter lock is not inside the fixed registry closure" >&2
  phase lock-drift 2
  exit 2
fi
phase lock-drift 0
grep 'serde 1.0.229 4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba' /tmp/crate-pins.txt
grep 'serde_json 1.0.151 c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14' /tmp/crate-pins.txt

echo "=== cargo test adapter ==="
set +e
cargo test --locked --offline -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
phase test "$test_exit"

echo "=== rustup components ==="
set +e
rustup component add rustfmt clippy --toolchain 1.94.0 > /evidence/component.stdout 2> /evidence/component.stderr
component_exit=$?
set -e
phase component "$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/phases/fmt.exit
  printf '%s\n' not_run > /evidence/phases/clippy.exit
  exit "$component_exit"
fi

echo "=== cargo fmt ==="
set +e
cargo fmt -- --check > /evidence/fmt.stdout 2> /evidence/fmt.stderr
fmt_exit=$?
set -e
phase fmt "$fmt_exit"

echo "=== cargo clippy ==="
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
phase clippy "$clippy_exit"

if [ "$test_exit" -ne 0 ] || [ "$fmt_exit" -ne 0 ] || [ "$clippy_exit" -ne 0 ]; then
  echo "adapter gate failed; query-pg was not compiled" >&2
  worst=1
  if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
  if [ "$fmt_exit" -ne 0 ]; then worst=$fmt_exit; fi
  if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
  exit "$worst"
fi

echo "=== cargo fetch workspace locked ==="
cd /src
set +e
cargo fetch --manifest-path /src/Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
set -e
phase fetch "$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi

echo "=== libclang ==="
rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
tool_exit=$?
phase sha256-tool "$tool_exit"
if [ "$tool_exit" -ne 0 ]; then
  exit "$tool_exit"
fi
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
tool_exit=$?
phase select-debs-tool "$tool_exit"
if [ "$tool_exit" -ne 0 ]; then
  exit "$tool_exit"
fi
export PATH="/cargo-home/bin:${PATH}"
# The redirect below fails if this directory is missing. extract-libclang.sh
# creates it only after the shell has already opened the log.
mkdir -p /evidence/libclang
if [ ! -s /evidence/libclang/libclang-dir.txt ] || [ ! -d /cargo-home/a222-clang-root/usr/lib ]; then
  set +e
  bash /extract-libclang.sh > /evidence/libclang/extract.log 2>&1
  extract_exit=$?
  set -e
  phase libclang "$extract_exit"
  if [ "$extract_exit" -ne 0 ]; then
    exit "$extract_exit"
  fi
else
  echo "libclang_root=reused" > /evidence/libclang/extract.log
  phase libclang 0
fi
libdir=$(cat /evidence/libclang/libclang-dir.txt)
multiarch=/cargo-home/a222-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a222-clang-root/usr/lib/llvm-19/lib/clang/19/include
real=$(find /cargo-home/a222-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a222-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a222-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  echo "LD_LIBRARY_PATH=$LD_LIBRARY_PATH"
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/libclang/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ] || [ -z "$real" ]; then
  phase libclang-load 1
  exit 1
fi
/lib/ld-linux-aarch64.so.1 --list "$real" > /evidence/libclang/libclang-ldd.txt 2>&1 || true
if grep -Eq 'not found|error while loading' /evidence/libclang/libclang-ldd.txt; then
  phase libclang-load 1
  exit 1
fi
phase libclang-load 0

echo "=== compile query-pg and comparator ==="
export CARGO_TARGET_DIR=/target/workspace
set +e
cargo build --manifest-path /src/benchmarks/result-comparator/Cargo.toml --locked --offline --bin result-comparator \
  > /evidence/comparator-build.stdout 2> /evidence/comparator-build.stderr
comparator_build=$?
set -e
phase comparator-build "$comparator_build"
if [ "$comparator_build" -ne 0 ]; then
  exit "$comparator_build"
fi
if [ ! -x /target/workspace/debug/result-comparator ]; then
  echo "comparator binary missing" >&2
  phase comparator-build 2
  exit 2
fi
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline \
  -p oo-gateway --test query_pg_cli --no-run \
  > /evidence/norun.stdout 2> /evidence/norun.stderr
norun_exit=$?
set -e
phase norun "$norun_exit"
if [ "$norun_exit" -ne 0 ]; then
  exit "$norun_exit"
fi
# Cargo 1.94 writes the Executable line to stderr. Stdout was empty in attempt 004.
exe=$(awk -F'[()]' '/Executable tests\/query_pg_cli.rs \(/ {print $2; exit}' /evidence/norun.stdout /evidence/norun.stderr)
exe=$(printf '%s' "$exe" | tr -d '[:space:]')
printf '%s\n' "$exe" > /evidence/query-pg-test-exe.txt
if [ ! -x "$exe" ] || [ ! -x /target/workspace/debug/query-pg ]; then
  {
    echo "exe=${exe:-empty}"
    ls -l /target/workspace/debug/query-pg /target/workspace/debug/deps/query_pg_cli-* 2>&1 || true
  } > /evidence/query-pg-bin-listing.txt
  echo "query-pg test binary missing" >&2
  phase tee 2
  exit 2
fi
# Cargo hardlinks debug/query-pg to the deps artifact. A later --no-run
# restores that link, which can be the same inode as query-pg.real, and mv
# then refuses. Copy the ELF aside before installing the wrapper.
bin=/target/workspace/debug/query-pg
real=/target/workspace/query-pg.real
if ! cmp -s "$bin" /tee.sh; then
  cp -a "$bin" "${real}.next"
  cp /tee.sh "${bin}.next"
  mv -f "${real}.next" "$real"
  mv -f "${bin}.next" "$bin"
fi
chmod 755 "$bin" "$real"
phase tee 0

echo "=== live query-pg ==="
mkdir -p /evidence/live/captures
set +e
env OO_A198_LIVE=1 "$exe" --exact live_query_pg_against_private_postgres --test-threads=1 --nocapture \
  > /evidence/live/test.stdout 2> /evidence/live/test.stderr
live_exit=$?
set -e
phase live "$live_exit"
if [ "$live_exit" -ne 0 ]; then
  exit "$live_exit"
fi
if [ ! -s /evidence/live/desk-explicit.stdout ]; then
  echo "desk stdout was not saved" >&2
  phase desk 2
  exit 2
fi
phase desk 0

echo "=== secret scan ==="
rustc --edition 2021 /tools/secret-scan.rs -o /cargo-home/bin/secret-scan
tool_exit=$?
phase secret-scan-tool "$tool_exit"
if [ "$tool_exit" -ne 0 ]; then
  exit "$tool_exit"
fi
find /evidence/live -type f | sort > /evidence/live-files.txt
set +e
/cargo-home/bin/secret-scan \
  /run/oo-a198/fixture/admin.pass \
  /run/oo-a198/fixture/login.pass \
  /evidence/secret-scan.txt \
  $(cat /evidence/live-files.txt) \
  > /evidence/secret-scan.stdout 2> /evidence/secret-scan.stderr
scan_exit=$?
set -e
phase secret-scan "$scan_exit"
if [ "$scan_exit" -ne 0 ]; then
  exit "$scan_exit"
fi

echo "=== adapter proof ==="
export CARGO_TARGET_DIR=/target/adapter
set +e
env OO_A222_PROOF=1 \
  OO_A222_CAPTURE=/evidence/live/captures \
  OO_A222_DESK=/evidence/live/desk-explicit.stdout \
  OO_A222_COMPARATOR=/target/workspace/debug/result-comparator \
  cargo test --manifest-path /src/benchmarks/query-result-adapter/Cargo.toml \
  --locked --offline --test proof -- \
  --exact live_query_pg_stdout_matches_hand_gold_and_mutations_refuse \
  --test-threads=1 --nocapture \
  > /evidence/proof.stdout 2> /evidence/proof.stderr
proof_exit=$?
set -e
phase proof "$proof_exit"

echo "=== product unchanged ==="
diff -rq /archive /src > /evidence/tree-diff-after.txt || true
if grep -v -e 'Only in /archive: rust-toolchain.toml' \
  -e 'Only in /src: rust-toolchain.toml.repo' \
  -e 'Only in /src/benchmarks: query-result-adapter' \
  /evidence/tree-diff-after.txt | grep -q .; then
  echo "product tree changed" >&2
  phase product 2
  proof_exit=2
else
  phase product 0
fi
(
  cd /src/benchmarks/query-result-adapter
  find . -type f ! -name Cargo.lock ! -name '.DS_Store' | sort | xargs sha256sum
) > /evidence/source-sha256.after
if ! cmp -s /evidence/source-sha256.txt /evidence/source-sha256.after; then
  echo "adapter sources changed during the proof" >&2
  phase source-after 2
  proof_exit=2
else
  phase source-after 0
fi

exit "$proof_exit"
