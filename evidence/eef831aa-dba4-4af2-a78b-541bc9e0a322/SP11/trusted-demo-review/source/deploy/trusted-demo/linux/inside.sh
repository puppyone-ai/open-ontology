#!/bin/bash
# Inside the one arm64 builder. Archive /src is read-only.
# Onboarding smoke runs before demo binaries and before apply-reviewed.
# No cargo update. No apt. /tmp is noexec.
set -u
# shellcheck source=manifest.sh
. /deploy/linux/manifest.sh
if ! load_deployment /deployment.json; then
  exit 2
fi
echo "oo_task=sp11-trusted-demo"
echo "oo_owner=$OWNER"
echo "oo_run=$RUN_ID"
echo "oo_fixed=$FIXED_SOURCE"
MODE=${OO_DEMO_MODE:-}
PHASE=${OO_DEMO_PHASE:-}
if [ -z "$PHASE" ] || [ -z "$MODE" ]; then
  echo "missing OO_DEMO_MODE or OO_DEMO_PHASE" >&2
  exit 2
fi
echo "phase=$PHASE mode=$MODE"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p "$PHASE/phases" /cargo-home/demo-bins /cargo-home/bin /target/onboarding
cd /src

record() {
  mkdir -p "$PHASE/phases"
  printf '%s\n' "$2" > "$PHASE/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > "$PHASE/phase.exit"
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" > "$PHASE/rustc-version.txt"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0
sha=$(cat /src/.fixed-base-sha)
printf '%s\n' "$sha" > "$PHASE/fixed-base.txt"
if [ "$sha" != "$FIXED_SOURCE" ]; then
  record source 2
fi
if ! grep -q 'changes_may_have_been_applied' /src/tools/pg-onboarding/src/outcome.rs; then
  echo "archive tool is not the fixed outcome vocabulary" >&2
  record source 2
fi
record source 0
printf '%s\n' "no" > "$PHASE/cargo-update-invoked.txt"

export_drive() {
  export OO_DEMO_ONBOARDING=/cargo-home/demo-bins/pg-onboarding
  export OO_DEMO_BINS=/cargo-home/demo-bins
  export OO_DEMO_SECRETS=/secrets
  export OO_DEMO_RUN=/evidence/live/run
  export OO_DEMO_WORK=/evidence/live/work
  export OO_DEMO_BAD=/evidence/live/bad-run
  export OO_DEMO_DECLARED=/deploy/fixture/declared-review.json
  export OO_DEMO_EVIDENCE=/evidence
}

write_manifest() {
  rustup run 1.94.0 rustc --edition 2021 /deploy/tools/sha256_manifest.rs -o /cargo-home/bin/sha256-manifest > "$PHASE/manifest-build.log" 2>&1
  record manifest-build $?
  /cargo-home/bin/sha256-manifest /evidence "$PHASE/manifest.sha256" > "$PHASE/manifest.log" 2>&1
  record manifest $?
}

if [ "$MODE" = "smoke" ]; then
  if [ ! -x /cargo-home/bin/drive ] || [ ! -x /cargo-home/demo-bins/query-pg ] || [ ! -x /cargo-home/demo-bins/pg-onboarding ]; then
    echo "smoke requires the binaries left on the cargo volume" >&2
    record smoke-bins 1
  fi
  record smoke-bins 0
  export_drive
  /cargo-home/bin/drive smoke > "$PHASE/drive-smoke.log" 2>&1
  record drive-smoke $?
  if [ ! -f /evidence/live/smoke/smoke-ok.txt ]; then
    echo "smoke marker missing" >&2
    record smoke-marker 1
  fi
  record smoke-marker 0
  write_manifest
  printf '%s\n' 0 > "$PHASE/phase.exit"
  echo "smoke=ok"
  exit 0
fi

if [ "$MODE" != "public" ]; then
  echo "unknown mode" >&2
  record mode 2
fi

echo "=== cargo fetch workspace --locked ==="
cargo fetch --manifest-path /src/Cargo.toml --locked > "$PHASE/fetch-workspace.log" 2>&1
record fetch-workspace $?
echo "=== cargo fetch pg-onboarding --locked ==="
cargo fetch --manifest-path /src/tools/pg-onboarding/Cargo.toml --locked > "$PHASE/fetch-onboarding.log" 2>&1
record fetch-onboarding $?

echo "=== rustup component add clippy ==="
rustup component add clippy --toolchain 1.94.0 > "$PHASE/clippy-component.log" 2>&1
record clippy-component $?

echo "=== cargo test pg-onboarding --locked --offline ==="
CARGO_TARGET_DIR=/target/onboarding cargo test --manifest-path /src/tools/pg-onboarding/Cargo.toml --locked --offline -- --test-threads=1 \
  > "$PHASE/test.log" 2>&1
record test $?

sum_field() {
  grep '^test result:' "$PHASE/test.log" \
    | sed -n "s/.* \\([0-9][0-9]*\\) $1.*/\\1/p" \
    | awk '{s+=$1} END{print s+0}'
}
printf 'passed=%s failed=%s ignored=%s\n' "$(sum_field passed)" "$(sum_field failed)" "$(sum_field ignored)" \
  > "$PHASE/test-count.txt"
failed=$(sed -n 's/.*failed=\([0-9][0-9]*\).*/\1/p' "$PHASE/test-count.txt")
if [ "$failed" != "0" ]; then
  echo "onboarding tests failed" >&2
  record test-count 1
fi
if ! grep -F 'test live_chain ... ignored' "$PHASE/test.log" >/dev/null; then
  echo "live_chain was not ignored" >&2
  record test-count 1
fi
if ! grep -F 'test duplicate_decisions_refuse_before_review ... ok' "$PHASE/test.log" >/dev/null; then
  echo "duplicate decision test did not pass" >&2
  record test-count 1
fi
if ! grep -F 'test cli_failed_child_keeps_marker_and_static_stderr ... ok' "$PHASE/test.log" >/dev/null; then
  echo "CLI child-failure test did not pass" >&2
  record test-count 1
fi
record test-count 0

echo "=== cargo clippy pg-onboarding ==="
CARGO_TARGET_DIR=/target/onboarding cargo clippy --manifest-path /src/tools/pg-onboarding/Cargo.toml --locked --offline --all-targets -- -D warnings \
  > "$PHASE/clippy.log" 2>&1
record clippy $?

echo "=== drive tests ==="
rustup run 1.94.0 rustc --edition 2024 --test -D warnings /deploy/src/drive.rs -o /cargo-home/bin/drive-test \
  > "$PHASE/drive-test-build.log" 2>&1
record drive-test-build $?
/cargo-home/bin/drive-test --test-threads=1 > "$PHASE/drive-test.log" 2>&1
record drive-test $?

echo "=== drive clippy ==="
rustup run 1.94.0 clippy-driver --edition 2024 -D warnings /deploy/src/drive.rs -o /target/drive-clippy \
  > "$PHASE/drive-clippy.log" 2>&1
record drive-clippy $?
rustup run 1.94.0 rustc --edition 2024 -D warnings /deploy/src/drive.rs -o /cargo-home/bin/drive \
  > "$PHASE/drive-build.log" 2>&1
record drive-build $?

rustup run 1.94.0 rustc --edition 2021 /deploy/tools/sha256.rs -o /cargo-home/bin/sha256 > "$PHASE/sha256-tool.log" 2>&1
record sha256-tool $?
printf 'abc' > /cargo-home/sha256-abc.txt
/cargo-home/bin/sha256 file /cargo-home/sha256-abc.txt > "$PHASE/sha256-self.txt"
if ! grep -q 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad' "$PHASE/sha256-self.txt"; then
  echo "sha256 abc vector mismatch" >&2
  record sha256-vector 1
fi
record sha256-vector 0
rustup run 1.94.0 rustc --edition 2021 /deploy/tools/select-debs.rs -o /cargo-home/bin/select-debs > "$PHASE/select-debs-tool.log" 2>&1
record select-debs-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ ! -d /cargo-home/demo-clang-root/usr/lib ]; then
  bash /extract-libclang.sh > "$PHASE/libclang-extract.log" 2>&1
  record libclang-extract $?
else
  echo "libclang_root=reused" > "$PHASE/libclang-extract.log"
  record libclang-extract 0
fi
libdir=$(find /cargo-home/demo-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit)
if [ -z "$libdir" ]; then
  echo "libclang shared object is absent" >&2
  record libclang 1
fi
libdir=$(dirname "$libdir")
multiarch=/cargo-home/demo-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/demo-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/demo-clang-root/usr/bin:${PATH}"
record libclang 0
export CARGO_NET_OFFLINE=true

build_bin() {
  pkg=$1
  bin=$2
  CARGO_TARGET_DIR=/target cargo build --manifest-path /src/Cargo.toml --locked --offline -p "$pkg" --bin "$bin" \
    > "$PHASE/bins-build-${bin}.log" 2>&1
  record "bins-${bin}" $?
  cp "/target/debug/${bin}" "/cargo-home/demo-bins/${bin}" || record "bins-${bin}-copy" 1
}

echo "=== workspace bins ==="
build_bin oo-pipeline init-pg
build_bin oo-pipeline oo-pipeline
build_bin oo-pipeline analyze-pg
build_bin oo-builder oo-builder
build_bin oo-builder reviewed-effective-oos
build_bin oo-builder role-pack-bundle
build_bin oo-gateway reviewed-policy
build_bin oo-gateway install-column-acl
build_bin oo-gateway provision-mapping-reader
build_bin oo-gateway query-pg

echo "=== pg-onboarding ==="
CARGO_TARGET_DIR=/target/onboarding cargo build --manifest-path /src/tools/pg-onboarding/Cargo.toml --locked --offline --bin pg-onboarding \
  > "$PHASE/build-onboarding.log" 2>&1
record build-onboarding $?
cp /target/onboarding/debug/pg-onboarding /cargo-home/demo-bins/pg-onboarding || record build-onboarding-copy 1

{
  for bin in init-pg oo-pipeline analyze-pg oo-builder reviewed-effective-oos role-pack-bundle reviewed-policy install-column-acl provision-mapping-reader query-pg pg-onboarding; do
    /cargo-home/bin/sha256 file "/cargo-home/demo-bins/${bin}"
  done
  /cargo-home/bin/sha256 file /cargo-home/bin/drive
} > "$PHASE/bin-sha256.txt"

if [ -e /evidence/live/run ]; then
  echo "run directory already exists" >&2
  record run-dir 2
fi
record run-dir 0
export_drive
echo "=== drive public-fixture ==="
/cargo-home/bin/drive public-fixture > "$PHASE/drive.log" 2>&1
record drive $?
if ! grep -q 'ready_for_trusted_query' "$PHASE/drive.log"; then
  echo "drive did not report ready_for_trusted_query" >&2
  record drive-marker 1
fi
if ! grep -q '"status":"ready_for_trusted_query"' /evidence/live/receipt.json; then
  echo "receipt is not ready" >&2
  record drive-marker 1
fi
record drive-marker 0
write_manifest
printf '%s\n' 0 > "$PHASE/phase.exit"
echo "public=ok"
exit 0
