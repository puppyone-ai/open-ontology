#!/bin/bash
# Real inspect and apply-reviewed against one private PostgreSQL 17.10.
# Product bins come from the immutable archive. The fixture driver is separate.
# No cargo update. No apt. No hand-written target rows.
set -u
echo "oo_task=sp12-product-benchmark-chain"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a389"
echo "oo_fixed=6f87942f1c66ba1a9787b7d7194e58a507af63ef"
echo "phase=chain"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TERM_COLOR=never
export HOME=/tmp
export TMPDIR=/cargo-home/tmp
export CARGO_BUILD_JOBS=1
export CARGO_TARGET_DIR=/target/pg-e2e
mkdir -p /evidence/linux/chain/phases /evidence/live/work /evidence/live/fixture /evidence/live/measurements \
  /cargo-home/tmp /cargo-home/a317-bins /cargo-home/bin /target/pg-e2e
BINS=/cargo-home/a317-bins
SECRETS=/secrets
RUN=/evidence/live/run
WORK=/evidence/live/work
READY=/ready/runtime.ready

record() {
  mkdir -p /evidence/linux/chain/phases
  printf '%s\n' "$2" > "/evidence/linux/chain/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/chain/chain.exit
    exit "$2"
  fi
}

run_log() {
  name=$1
  shift
  set +e
  "$@" > "/evidence/linux/chain/${name}.log" 2>&1
  code=$?
  set +e
  record "$name" "$code"
}

wait_flag() {
  name=$1
  ok=$2
  bad=$3
  limit=$4
  n=0
  while [ ! -f "$ok" ]; do
    if [ -n "$bad" ] && [ -f "$bad" ]; then
      echo "flag failed: $name" >&2
      record "$name" 1
    fi
    n=$((n + 1))
    if [ "$n" -ge "$limit" ]; then
      echo "timeout: $name" >&2
      record "$name" 1
    fi
    sleep 1
  done
  record "$name" 0
}

got=$(rustc --version)
printf '%s\n' "$got" > /evidence/linux/chain/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0
sha=$(cat /src/.fixed-base-sha)
printf '%s\n' "$sha" > /evidence/linux/chain/fixed-base.txt
if [ "$sha" != "6f87942f1c66ba1a9787b7d7194e58a507af63ef" ]; then
  record source 2
fi
if ! grep -q '| "numeric")' /src/crates/oo-gateway/src/reviewed_policy.rs; then
  echo "archive policy does not admit numeric" >&2
  record source 2
fi
if ! grep -q "const AEXPR_IN: i32 = pb::AExprKind::AexprIn as i32;" /src/crates/oo-gateway/src/sql_guard.rs; then
  echo "archive guard does not admit a bounded IN list" >&2
  record source 2
fi
if ! grep -q 'changes_may_have_been_applied' /src/tools/pg-onboarding/src/outcome.rs; then
  echo "archive tool is not the repair" >&2
  record source 2
fi
record source 0
printf '%s\n' "no" > /evidence/linux/chain/cargo-update-invoked.txt

run_log driver-test cargo test --manifest-path /fixture/Cargo.toml --locked --offline -- --test-threads=1
cp /target/pg-e2e/debug/pg-e2e "$BINS/pg-e2e"
"$BINS/pg-e2e" emit /evidence/live/fixture
record emit $?
: > /ready/fixture.ready
wait_flag load-wait /ready/loaded.ready /ready/load.failed 1800

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
printf 'abc' > /cargo-home/sha256-abc.txt
/cargo-home/bin/sha256 file /cargo-home/sha256-abc.txt > /evidence/linux/chain/sha256-self.txt
if ! grep -q 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad' /evidence/linux/chain/sha256-self.txt; then
  echo "sha256 abc vector mismatch" >&2
  record sha256-vector 1
fi
record sha256-vector 0
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
record select-debs-tool $?
rustc --edition 2021 /tools/sha256_manifest.rs -o /cargo-home/bin/sha256_manifest
record sha256-manifest-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ ! -d /cargo-home/a274-clang-root/usr/lib ]; then
  bash /extract-libclang.sh > /evidence/linux/chain/libclang-extract.log 2>&1
  record libclang-extract $?
else
  echo "libclang_root=reused" > /evidence/linux/chain/libclang-extract.log
  record libclang-extract 0
fi
libdir=$(find /cargo-home/a274-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit)
if [ -z "$libdir" ]; then
  echo "libclang shared object is absent" >&2
  record libclang 1
fi
libdir=$(dirname "$libdir")
multiarch=/cargo-home/a274-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a274-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a274-clang-root/usr/bin:${PATH}"
record libclang 0

echo "=== cargo fetch workspace --locked ==="
set +e
cargo fetch --manifest-path /src/Cargo.toml --locked > /evidence/linux/chain/fetch-workspace.log 2>&1
fetch_exit=$?
set +e
record fetch-workspace "$fetch_exit"

build_bin() {
  pkg=$1
  bin=$2
  set +e
  CARGO_TARGET_DIR=/target cargo build --manifest-path /src/Cargo.toml --locked --offline -p "$pkg" --bin "$bin" \
    > "/evidence/linux/chain/bins-build-${bin}.log" 2>&1
  bins_exit=$?
  set +e
  record "bins-${bin}" "$bins_exit"
  cp "/target/debug/${bin}" "${BINS}/${bin}"
}

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
set +e
CARGO_TARGET_DIR=/target/onboarding cargo build --manifest-path /src/tools/pg-onboarding/Cargo.toml --locked --offline --bin pg-onboarding \
  > /evidence/linux/chain/build-onboarding.log 2>&1
onboard_exit=$?
set +e
record build-onboarding "$onboard_exit"
cp /target/onboarding/debug/pg-onboarding "$BINS/pg-onboarding"

if [ -e "$RUN" ]; then
  echo "run directory already exists" >&2
  record run-dir 2
fi
record run-dir 0

date +%s > /evidence/live/measurements/inspect-start.epoch
"$BINS/pg-e2e" inspect-config "$BINS" "$SECRETS" "$READY" "$WORK/inspect.json"
record inspect-config $?
set +e
"$BINS/pg-onboarding" inspect --config "$WORK/inspect.json" --run "$RUN" > "$WORK/inspect.stdout" 2> "$WORK/inspect.stderr"
inspect_exit=$?
set +e
printf '%s\n' "$inspect_exit" > "$WORK/inspect.exit"
date +%s > /evidence/live/measurements/inspect-end.epoch
if [ "$inspect_exit" -eq 0 ]; then
  : > /ready/inspect.done
fi
record inspect "$inspect_exit"

"$BINS/pg-e2e" check-inspect "$RUN"
record check-inspect $?
"$BINS/pg-e2e" record-measurements "$RUN/import-result.json" "$RUN/inspect-manifest.json" /evidence/live/measurements/import.txt
record measurements $?
"$BINS/pg-e2e" bind "$RUN/captured-draft.json" "$WORK"
record bind $?
"$BINS/pg-e2e" apply-config "$BINS" "$SECRETS" "$WORK" "$WORK/apply.json"
record apply-config $?

date +%s > /evidence/live/measurements/apply-start.epoch
set +e
"$BINS/pg-onboarding" apply-reviewed --config "$WORK/apply.json" --run "$RUN" > "$WORK/apply.stdout" 2> "$WORK/apply.stderr"
apply_exit=$?
set +e
printf '%s\n' "$apply_exit" > "$WORK/apply.exit"
date +%s > /evidence/live/measurements/apply-end.epoch
record apply "$apply_exit"

"$BINS/pg-e2e" check-apply "$RUN" "$SECRETS"
record check-apply $?
wait_flag priv-wait /ready/priv.done /ready/side.failed 300

for table in party account entry; do
  "$BINS/pg-e2e" copy-sql "$RUN/import-result.json" "$table" "$WORK/copy-${table}.sql"
  record "copy-sql-${table}" $?
done
: > /ready/copy.wanted
wait_flag copy-wait /ready/copy.ready /ready/copy.failed 600
for table in party account entry; do
  "$BINS/pg-e2e" check-copy "$table" "/evidence/live/actual/${table}.copy"
  record "check-copy-${table}" $?
done

mkdir -p "$RUN/audit"
"$BINS/pg-e2e" fresh-request "$RUN/inspect-manifest.json" "$RUN/steps/reviewed-policy-wide.stdout" "$SECRETS/login.pass" "$RUN/audit/fresh-party.jsonl" "$WORK/fresh.request.json"
record fresh-request $?
set +e
"$BINS/query-pg" --request "$WORK/fresh.request.json" > "$WORK/fresh.stdout" 2> "$WORK/fresh.stderr"
fresh_exit=$?
set +e
printf '%s\n' "$fresh_exit" > "$WORK/fresh.exit"
record fresh "$fresh_exit"
"$BINS/pg-e2e" check-fresh "$WORK/fresh.stdout" "$RUN/audit/fresh-party.jsonl" "$SECRETS"
record check-fresh $?

"$BINS/pg-e2e" probe-request "$RUN/inspect-manifest.json" "$RUN/steps/reviewed-policy-wide.stdout" "$SECRETS/login.pass" "$RUN/audit/probe-amount.jsonl" "$WORK/probe.request.json"
record probe-request $?
set +e
"$BINS/query-pg" --request "$WORK/probe.request.json" > "$WORK/probe.stdout" 2> "$WORK/probe.stderr"
probe_exit=$?
set +e
printf '%s\n' "$probe_exit" > "$WORK/probe.exit"
printf '%s\n' "$probe_exit" > /evidence/linux/chain/phases/probe.exit
"$BINS/pg-e2e" note-probe "$WORK/probe.exit" "$WORK/probe.stdout" /evidence/live/measurements/probe.txt
record note-probe $?

"$BINS/pg-e2e" pack-digest "$RUN/bundles/wide.json" > /evidence/live/measurements/pack-wide.before
"$BINS/pg-e2e" pack-digest "$RUN/bundles/narrow.json" > /evidence/live/measurements/pack-narrow.before
: > /ready/side.wanted
wait_flag restart-wait /ready/restarted.ready /ready/restart.failed 180
"$BINS/pg-e2e" pack-digest "$RUN/bundles/wide.json" > /evidence/live/measurements/pack-wide.after
"$BINS/pg-e2e" pack-digest "$RUN/bundles/narrow.json" > /evidence/live/measurements/pack-narrow.after
cmp /evidence/live/measurements/pack-wide.before /evidence/live/measurements/pack-wide.after
record pack-wide $?
cmp /evidence/live/measurements/pack-narrow.before /evidence/live/measurements/pack-narrow.after
record pack-narrow $?

"$BINS/pg-e2e" fresh-request "$RUN/inspect-manifest.json" "$RUN/steps/reviewed-policy-wide.stdout" "$SECRETS/login.pass" "$RUN/audit/fresh-party-restart.jsonl" "$WORK/fresh-restart.request.json"
record fresh-restart-request $?
set +e
"$BINS/query-pg" --request "$WORK/fresh-restart.request.json" > "$WORK/fresh-restart.stdout" 2> "$WORK/fresh-restart.stderr"
restart_query_exit=$?
set +e
printf '%s\n' "$restart_query_exit" > "$WORK/fresh-restart.exit"
record fresh-restart "$restart_query_exit"
"$BINS/pg-e2e" check-fresh "$WORK/fresh-restart.stdout" "$RUN/audit/fresh-party-restart.jsonl" "$SECRETS"
record check-fresh-restart $?

mkdir -p /evidence/source/blobs
cp /src/crates/oo-gateway/src/sql_guard.rs /evidence/source/blobs/sql_guard.rs
cp /src/crates/oo-gateway/src/reviewed_policy.rs /evidence/source/blobs/reviewed_policy.rs
cp /src/crates/oo-gateway/src/query_executor.rs /evidence/source/blobs/query_executor.rs
cp /src/crates/oo-gateway/src/bin/query-pg.rs /evidence/source/blobs/query-pg.rs
record source-blobs 0
rustc --edition 2021 -D warnings /evidence/oracle/oracle.rs -o /cargo-home/bin/oo-a389-oracle
record oracle-build $?
set +e
/cargo-home/bin/oo-a389-oracle /evidence > /evidence/linux/chain/oracle.log 2>&1
oracle_exit=$?
set +e
printf '%s\n' "$oracle_exit" > /evidence/linux/chain/phases/oracle.exit
if [ "$oracle_exit" -ne 0 ]; then
  echo "independent oracle failed" >&2
  printf '%s\n' "$oracle_exit" > /evidence/linux/chain/chain.exit
  exit "$oracle_exit"
fi
record oracle 0

if grep -F 'cargo update' /evidence/linux/chain/fetch-workspace.log /evidence/linux/chain/build-onboarding.log /evidence/linux/chain/driver-test.log >/dev/null 2>&1; then
  echo "cargo update text appeared" >&2
  record cargo-update-text 1
fi
record cargo-update-text 0
printf '%s\n' 0 > /evidence/linux/chain/chain.exit
echo "chain=ok"
exit 0
